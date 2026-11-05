//! BlocRA's AI analyst.
//!
//! Talks to any OpenAI-compatible chat-completions API, so the same code runs
//! against a local Ollama, a self-hosted vLLM serving gpt-oss, or a hosted
//! provider. Configure with:
//!
//!   AI_BASE_URL  e.g. http://localhost:11434/v1 (Ollama) or https://api.provider.com/v1
//!   AI_MODEL     e.g. qwen2.5:3b, gpt-oss:20b, openai/gpt-oss-120b
//!   AI_API_KEY   optional bearer token for hosted providers
//!
//! With AI_BASE_URL unset the AI endpoints answer 503.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Tables the in-app SQL engine understands (frontend utils/queryLimits.ts).
const SQL_SCHEMA: &str = "\
blocks(block_number INTEGER, timestamp INTEGER, transaction_count INTEGER, gas_used INTEGER)
transactions(transaction_hash TEXT, block_number INTEGER, sender_address TEXT, max_fee INTEGER, type TEXT, timestamp INTEGER)
contracts(address TEXT, class_hash TEXT, deployed_at_block INTEGER)";

const ANALYST_PROMPT: &str = "You are BlocRA's on-chain analyst. BlocRA is a blockchain analytics \
platform: users analyze smart contracts on 36 chains (Starknet, Ethereum and EVM networks, Solana), \
write SQL over blocks, transactions and contracts, and build dashboards. Answer concisely and \
concretely. Only state numbers that appear in the data you are given; if something isn't in the \
data, say so instead of guessing.";

#[derive(Clone)]
pub struct AiService {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("AI is not configured on this server (set AI_BASE_URL and AI_MODEL)")]
    NotConfigured,
    #[error("AI provider request failed: {0}")]
    Upstream(String),
    #[error("{0}")]
    BadRequest(String),
}

impl AiService {
    pub fn from_env() -> Self {
        let timeout = std::env::var("AI_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .build()
                .unwrap_or_default(),
            base_url: std::env::var("AI_BASE_URL")
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            model: std::env::var("AI_MODEL").unwrap_or_default(),
            api_key: std::env::var("AI_API_KEY").ok().filter(|k| !k.is_empty()),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.base_url.is_empty() && !self.model.is_empty()
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, messages: Vec<ChatMessage>, temperature: f32) -> Result<String, AiError> {
        if !self.is_configured() {
            return Err(AiError::NotConfigured);
        }
        let mut req = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&json!({
                "model": self.model,
                "messages": messages,
                "temperature": temperature,
                "stream": false,
            }));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(|e| AiError::Upstream(e.to_string()))?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| AiError::Upstream(e.to_string()))?;
        if !status.is_success() {
            let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(AiError::Upstream(format!("{status}: {msg}")));
        }
        body["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AiError::Upstream("empty response".into()))
    }

    /// Conversational assistant. `context` is optional page state (selected
    /// chain, contract stats) passed through as JSON.
    pub async fn chat(&self, history: Vec<ChatMessage>, context: Option<Value>) -> Result<String, AiError> {
        if history.is_empty() || history.len() > 40 {
            return Err(AiError::BadRequest("send between 1 and 40 messages".into()));
        }
        let mut messages = vec![system(ANALYST_PROMPT)];
        if let Some(ctx) = context {
            messages.push(system(&format!("Current page data (JSON):\n{}", truncate(&ctx.to_string(), 6_000))));
        }
        for m in history {
            if m.role != "user" && m.role != "assistant" {
                return Err(AiError::BadRequest("role must be user or assistant".into()));
            }
            messages.push(ChatMessage { role: m.role, content: truncate(&m.content, 4_000) });
        }
        self.complete(messages, 0.4).await
    }

    /// Written analysis of a contract from the stats the app computed, in the
    /// section layout the PDF/DOCX exporters expect.
    pub async fn contract_report(
        &self,
        contract_name: &str,
        contract_address: &str,
        chain: &str,
        stats: &Value,
    ) -> Result<String, AiError> {
        let prompt = format!(
            "Write an analysis report for the smart contract \"{contract_name}\" ({contract_address}) on {chain}.\n\
             Use only these on-chain statistics computed by BlocRA:\n{}\n\n\
             Format: these sections, each starting with the heading in bold capitals on its own line:\n\
             **EXECUTIVE SUMMARY**, **CONTRACT OVERVIEW**, **PERFORMANCE METRICS**, **BUSINESS INSIGHTS**, \
             **RISK ASSESSMENT**, **RECOMMENDATIONS**.\n\
             Short paragraphs and bullet points starting with \"• \". No other markdown. \
             Do not invent figures that are not in the statistics.",
            truncate(&stats.to_string(), 8_000)
        );
        self.complete(vec![system(ANALYST_PROMPT), user(&prompt)], 0.3).await
    }

    /// Natural language → one SQL query over BlocRA's tables.
    pub async fn generate_sql(&self, question: &str) -> Result<String, AiError> {
        if question.trim().is_empty() || question.len() > 1_000 {
            return Err(AiError::BadRequest("question must be 1-1000 characters".into()));
        }
        let prompt = format!(
            "Tables:\n{SQL_SCHEMA}\n\nWrite one read-only SQL SELECT query (SQLite dialect, with a LIMIT of \
             at most 100) that answers: {question}\nReply with the SQL only, no explanation, no code fences."
        );
        let raw = self.complete(vec![system(ANALYST_PROMPT), user(&prompt)], 0.0).await?;
        Ok(clean_sql(&raw))
    }
}

fn system(content: &str) -> ChatMessage {
    ChatMessage { role: "system".into(), content: content.into() }
}

fn user(content: &str) -> ChatMessage {
    ChatMessage { role: "user".into(), content: content.into() }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Strips code fences and anything after the first statement; models often
/// wrap SQL in ```sql blocks or add a sentence.
pub fn clean_sql(raw: &str) -> String {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("```") {
        s = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic()).trim_start();
    }
    let s = s.split("```").next().unwrap_or(s).trim();
    match s.find(';') {
        Some(i) => s[..=i].to_string(),
        None => format!("{s};"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_model_sql() {
        assert_eq!(clean_sql("```sql\nSELECT 1 FROM blocks;\n```"), "SELECT 1 FROM blocks;");
        assert_eq!(clean_sql("SELECT * FROM blocks LIMIT 5"), "SELECT * FROM blocks LIMIT 5;");
        assert_eq!(clean_sql("SELECT 1; DROP TABLE x;"), "SELECT 1;");
    }

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate("héllo", 2), "h…");
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn unconfigured_without_env() {
        let ai = AiService {
            client: reqwest::Client::new(),
            base_url: String::new(),
            model: String::new(),
            api_key: None,
        };
        assert!(!ai.is_configured());
    }
}
