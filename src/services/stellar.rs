//! Stellar network data via Horizon.
//!
//! Slimmed from stellar-insights' `StellarRpcClient` (backend/src/rpc/stellar.rs):
//! the Horizon models and endpoints BlocRA's analytics need, without that
//! project's circuit breaker, rate limiter, tracing and mock layers. Requests
//! fail over across the configured Horizon servers in order.
//!
//! Configure with STELLAR_HORIZON_URL (tried first) and STELLAR_NETWORK
//! (mainnet by default, or testnet).

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::errors::AppError;

/// Horizon servers per network, checked in blocra-research (2026-11-05).
const MAINNET_HORIZON: &[&str] = &["https://horizon.stellar.org", "https://horizon.stellar.lobstr.co"];
const TESTNET_HORIZON: &[&str] = &["https://horizon-testnet.stellar.org"];

/// Horizon's maximum page size.
const PAGE_LIMIT: u32 = 200;
/// Pages read per ledger (a busy mainnet ledger has ~300-1,000 transactions).
const MAX_PAGES: usize = 10;

/// Records that carry Horizon's paging cursor.
pub trait Paged {
    fn cursor(&self) -> &str;
}
impl Paged for HorizonTransaction {
    fn cursor(&self) -> &str {
        &self.paging_token
    }
}
impl Paged for HorizonOperation {
    fn cursor(&self) -> &str {
        &self.paging_token
    }
}

// ── Horizon models (from stellar-insights) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerInfo {
    pub sequence: u64,
    pub hash: String,
    // Horizon reports successful and failed counts separately; there is no
    // `transaction_count` field (the upstream model required one).
    #[serde(default)]
    pub successful_transaction_count: u32,
    #[serde(default)]
    pub failed_transaction_count: u32,
    pub operation_count: u32,
    pub closed_at: String,
    pub total_coins: String,
    pub fee_pool: String,
    #[serde(rename = "base_fee_in_stroops")]
    pub base_fee: u32,
    #[serde(rename = "base_reserve_in_stroops")]
    pub base_reserve: u32,
    #[serde(default)]
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonTransaction {
    pub hash: String,
    pub paging_token: String,
    pub ledger: u64,
    pub created_at: String,
    pub source_account: String,
    pub fee_account: Option<String>,
    pub fee_charged: Option<String>,
    pub max_fee: Option<String>,
    pub operation_count: u32,
    pub successful: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonOperation {
    pub id: String,
    pub paging_token: String,
    pub transaction_hash: String,
    pub source_account: String,
    #[serde(rename = "type")]
    pub operation_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HorizonResponse<T> {
    #[serde(rename = "_embedded")]
    embedded: Option<EmbeddedRecords<T>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbeddedRecords<T> {
    records: Vec<T>,
}

// ── Client ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StellarService {
    client: reqwest::Client,
    horizon_urls: Vec<String>,
    pub network: String,
}

impl StellarService {
    pub fn from_env() -> Self {
        let network = match std::env::var("STELLAR_NETWORK").as_deref() {
            Ok("testnet") => "testnet",
            _ => "mainnet",
        };
        let defaults = if network == "testnet" { TESTNET_HORIZON } else { MAINNET_HORIZON };
        let mut horizon_urls: Vec<String> = std::env::var("STELLAR_HORIZON_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
            .into_iter()
            .collect();
        horizon_urls.extend(defaults.iter().map(|s| s.to_string()));
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("blocra-backend")
                .build()
                .unwrap_or_default(),
            horizon_urls,
            network: network.to_string(),
        }
    }

    /// GETs `path` from the first Horizon server that answers successfully.
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, AppError> {
        let mut last_error = String::from("no Horizon servers configured");
        for base in &self.horizon_urls {
            let url = format!("{}{}", base.trim_end_matches('/'), path);
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<T>().await {
                    Ok(v) => return Ok(v),
                    Err(e) => last_error = format!("{base}: bad response: {e}"),
                },
                Ok(resp) => last_error = format!("{base}: HTTP {}", resp.status()),
                Err(e) => last_error = format!("{base}: {e}"),
            }
            log::warn!("Horizon request failed: {last_error}");
        }
        Err(AppError::BadRequest(format!("Stellar Horizon unavailable ({last_error})")))
    }

    async fn records<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<Vec<T>, AppError> {
        let resp: HorizonResponse<T> = self.get(path).await?;
        Ok(resp.embedded.map(|e| e.records).unwrap_or_default())
    }

    pub async fn latest_ledger(&self) -> Result<LedgerInfo, AppError> {
        self.records::<LedgerInfo>("/ledgers?order=desc&limit=1")
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::BadRequest("Horizon returned no ledgers".into()))
    }

    /// Follows Horizon's cursor until the collection is exhausted (or
    /// MAX_PAGES pages have been read).
    async fn all_records<T: for<'de> Deserialize<'de> + Paged>(&self, base: &str) -> Result<Vec<T>, AppError> {
        let mut out: Vec<T> = Vec::new();
        for _ in 0..MAX_PAGES {
            let cursor = out.last().map(|r| format!("&cursor={}", r.cursor())).unwrap_or_default();
            let page: Vec<T> = self.records(&format!("{base}{cursor}")).await?;
            let full = page.len() as u32 == PAGE_LIMIT;
            out.extend(page);
            if !full {
                break;
            }
        }
        Ok(out)
    }

    /// Every transaction in a ledger, failed ones included.
    pub async fn ledger_transactions(&self, sequence: u64) -> Result<Vec<HorizonTransaction>, AppError> {
        self.all_records(&format!(
            "/ledgers/{sequence}/transactions?limit={PAGE_LIMIT}&include_failed=true"
        ))
        .await
    }

    /// Every operation in a ledger, failed ones included.
    pub async fn ledger_operations(&self, sequence: u64) -> Result<Vec<HorizonOperation>, AppError> {
        self.all_records(&format!(
            "/ledgers/{sequence}/operations?limit={PAGE_LIMIT}&include_failed=true"
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_horizon_ledger() {
        // Shape of GET /ledgers?order=desc&limit=1 on mainnet (trimmed).
        let json = r#"{"_embedded":{"records":[{
            "sequence": 65326029, "hash": "ab", "successful_transaction_count": 149,
            "failed_transaction_count": 12, "operation_count": 392,
            "closed_at": "2026-11-05T22:03:06Z", "total_coins": "105443902087.3472865",
            "fee_pool": "5410465.9551733", "base_fee_in_stroops": 100,
            "base_reserve_in_stroops": 5000000, "protocol_version": 23
        }]}}"#;
        let resp: HorizonResponse<LedgerInfo> = serde_json::from_str(json).unwrap();
        let ledger = &resp.embedded.unwrap().records[0];
        assert_eq!(ledger.successful_transaction_count, 149);
        assert_eq!(ledger.failed_transaction_count, 12);
        assert_eq!(ledger.base_fee, 100);
    }
}
