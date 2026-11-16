//! Stellar RPC calls the collector and contract endpoints need: the latest
//! ledger, one ledger's contract events, and ledger entries (contract
//! instances and code).
//!
//! Endpoints come from the chain registry's `stellar` entry, with
//! `RPC_URL_STELLAR` tried first when it is set, so a project can point
//! BlocRA at its own RPC.

use serde_json::{json, Value};
use std::time::Duration;

use crate::errors::AppError;
use crate::services::chains::{self, ChainInfo, ChainRpc};
use crate::services::contract_events::RpcEvent;

const CHAIN: &str = "stellar";
const TIMEOUT: Duration = Duration::from_secs(20);
/// getEvents page size (the RPC maximum).
const PAGE: usize = 10_000;

#[derive(Clone)]
pub struct SorobanRpc {
    rpc: std::sync::Arc<ChainRpc>,
    chain: &'static ChainInfo,
}

impl SorobanRpc {
    pub fn new() -> Self {
        Self {
            rpc: std::sync::Arc::new(ChainRpc::new()),
            chain: chains::chain(CHAIN).expect("the chain registry has stellar"),
        }
    }

    pub async fn latest_ledger(&self) -> Result<u64, AppError> {
        let r = self.rpc.call(CHAIN, self.chain, "getLatestLedger", json!({}), TIMEOUT).await?;
        r["sequence"].as_u64().ok_or_else(|| AppError::BadRequest("getLatestLedger returned no sequence".into()))
    }

    /// Every contract event (not system or diagnostic) emitted in `ledger`.
    pub async fn events_in_ledger(&self, ledger: u64) -> Result<Vec<RpcEvent>, AppError> {
        let mut events = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let pagination = match &cursor {
                Some(c) => json!({ "cursor": c, "limit": PAGE }),
                None => json!({ "limit": PAGE }),
            };
            let mut params = json!({
                "endLedger": ledger + 1,
                "filters": [{ "type": "contract" }],
                "pagination": pagination,
            });
            if cursor.is_none() {
                params["startLedger"] = json!(ledger);
            }
            // A node that hasn't reached the ledger yet would return nothing.
            let accept = |r: &Value| match r["latestLedger"].as_u64() {
                Some(latest) if latest >= ledger => Ok(()),
                _ => Err(format!("node has not reached ledger {ledger}")),
            };
            let r = self.rpc.call_checked(CHAIN, self.chain, "getEvents", params, TIMEOUT, accept).await?;
            let page: Vec<RpcEvent> = serde_json::from_value(r["events"].clone())
                .map_err(|e| AppError::BadRequest(format!("getEvents returned unexpected events: {e}")))?;
            let full = page.len() >= PAGE;
            events.extend(page.into_iter().filter(|e| e.ledger == ledger));
            match r["cursor"].as_str() {
                Some(c) if full && !c.is_empty() => cursor = Some(c.to_string()),
                _ => break,
            }
        }
        Ok(events)
    }

    /// Base64 `LedgerEntryData` for each key that exists, in key order.
    pub async fn ledger_entries(&self, keys: &[String]) -> Result<Vec<Option<String>>, AppError> {
        let r = self.rpc.call(CHAIN, self.chain, "getLedgerEntries", json!({ "keys": keys }), TIMEOUT).await?;
        let entries = r["entries"].as_array().cloned().unwrap_or_default();
        Ok(keys
            .iter()
            .map(|k| {
                entries
                    .iter()
                    .find(|e| e["key"].as_str() == Some(k.as_str()))
                    .and_then(|e| e["xdr"].as_str().map(String::from))
            })
            .collect())
    }
}
