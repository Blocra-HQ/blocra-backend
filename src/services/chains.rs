//! Chain registry and a generic JSON-RPC client with endpoint failover.
//!
//! The registry is `src/config/chains.json`, copied from
//! blocra-research/chains.json, where every RPC is checked for a live head
//! block. It is embedded at compile time and parsed once.
//!
//! For each chain, `RPC_URL_<ID>` (the id uppercased, e.g. `RPC_URL_ETHEREUM`)
//! is tried before the registry's endpoints when it is set.

use crate::errors::AppError;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

const REGISTRY_JSON: &str = include_str!("../config/chains.json");

/// Default per-request timeout; large responses (Solana blocks) pass their own.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Deserialize)]
pub struct ChainInfo {
    pub name: String,
    /// `evm`, `solana`, `cometbft`, `stellar` or `starknet`.
    #[serde(rename = "type")]
    pub kind: String,
    pub currency: String,
    pub rpcs: Vec<String>,
}

/// Every chain in the registry, keyed by id.
pub fn registry() -> &'static BTreeMap<String, ChainInfo> {
    static REGISTRY: OnceLock<BTreeMap<String, ChainInfo>> = OnceLock::new();
    REGISTRY.get_or_init(|| serde_json::from_str(REGISTRY_JSON).expect("src/config/chains.json is valid"))
}

pub fn chain(id: &str) -> Option<&'static ChainInfo> {
    registry().get(id)
}

/// `RPC_URL_<ID>` first (when set), then the registry's endpoints in order.
fn endpoints(id: &str, chain: &ChainInfo) -> Vec<String> {
    let var = format!(
        "RPC_URL_{}",
        id.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
            .collect::<String>()
    );
    let mut list: Vec<String> = std::env::var(var)
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .into_iter()
        .collect();
    list.extend(chain.rpcs.iter().cloned());
    list
}

pub struct ChainRpc {
    client: Client,
}

impl ChainRpc {
    pub fn new() -> Self {
        Self {
            // Some Cloudflare-fronted RPCs reject requests without a User-Agent.
            client: Client::builder()
                .user_agent(concat!("blocra-backend/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Calls `method` on each of the chain's endpoints in turn and returns the
    /// first non-null result. `accept` can reject a result (e.g. a block from
    /// a lagging node) so the next endpoint is tried.
    pub async fn call_checked(
        &self,
        id: &str,
        chain: &ChainInfo,
        method: &str,
        params: Value,
        timeout: Duration,
        accept: impl Fn(&Value) -> Result<(), String>,
    ) -> Result<Value, AppError> {
        let mut errors = Vec::new();
        for url in endpoints(id, chain) {
            match self.call_one(&url, method, &params, timeout).await {
                Ok(result) => match accept(&result) {
                    Ok(()) => return Ok(result),
                    Err(e) => errors.push(format!("{}: {e}", host(&url))),
                },
                Err(e) => errors.push(format!("{}: {e}", host(&url))),
            }
            log::warn!("{id} {method} failed on {}", host(&url));
        }
        Err(AppError::BadRequest(format!(
            "{} RPC {method} failed on every endpoint ({})",
            chain.name,
            errors.join("; ")
        )))
    }

    pub async fn call(
        &self,
        id: &str,
        chain: &ChainInfo,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppError> {
        self.call_checked(id, chain, method, params, timeout, |_| Ok(())).await
    }

    async fn call_one(&self, url: &str, method: &str, params: &Value, timeout: Duration) -> Result<Value, String> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let response = self
            .client
            .post(url)
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "timed out".to_string() } else { e.to_string() })?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        let mut reply: Value = response.json().await.map_err(|e| format!("invalid JSON: {e}"))?;
        if let Some(err) = reply.get("error").filter(|e| !e.is_null()) {
            let message = err.get("message").and_then(|m| m.as_str()).map(str::to_string);
            return Err(message.unwrap_or_else(|| err.to_string()));
        }
        match reply.get_mut("result").map(Value::take) {
            Some(result) if !result.is_null() => Ok(result),
            _ => Err("empty result".to_string()),
        }
    }
}

impl Default for ChainRpc {
    fn default() -> Self {
        Self::new()
    }
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::{chain, registry};

    #[test]
    fn registry_parses_and_covers_every_type() {
        let reg = registry();
        assert_eq!(reg.len(), 40);
        for (id, c) in reg {
            assert!(!c.rpcs.is_empty(), "{id} has no RPCs");
            assert!(
                ["evm", "solana", "cometbft", "stellar", "starknet"].contains(&c.kind.as_str()),
                "{id} has unknown type {}",
                c.kind
            );
        }
        assert_eq!(chain("ethereum").unwrap().currency, "ETH");
        assert_eq!(chain("solana").unwrap().kind, "solana");
        assert!(chain("nope").is_none());
    }
}
