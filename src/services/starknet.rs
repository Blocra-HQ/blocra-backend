//! Starknet JSON-RPC client with failover, used for the Starknet network
//! dashboard (one of the 36 chains BlocRA supports beside Stellar).

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use crate::errors::AppError;

// Starknet RPCs in priority order, from blocra-research/chains.json, where
// every endpoint is checked for a real head block (last verified 2026-11-05).
// STARKNET_RPC_URL, if set, is tried first.
const DEFAULT_RPC_ENDPOINTS: &[&str] = &[
    "https://starknet-rpc.publicnode.com",
    "https://starknet.api.onfinality.io/public",
    "https://api.zan.top/public/starknet-mainnet",
];

fn rpc_endpoints() -> &'static [String] {
    static ENDPOINTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    ENDPOINTS.get_or_init(|| {
        let mut list: Vec<String> = std::env::var("STARKNET_RPC_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())
            .into_iter()
            .collect();
        list.extend(DEFAULT_RPC_ENDPOINTS.iter().map(|s| s.to_string()));
        list
    })
}

static CURRENT_RPC_INDEX: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    pub id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
    pub id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

pub struct StarknetRpc {
    client: Client,
}

impl StarknetRpc {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // Mobile-friendly timeout
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn get_rpc_url(&self) -> &'static str {
        let endpoints = rpc_endpoints();
        let index = CURRENT_RPC_INDEX.load(Ordering::Relaxed);
        &endpoints[index % endpoints.len()]
    }

    fn switch_rpc(&self) {
        CURRENT_RPC_INDEX.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value, AppError> {
        let mut last_error = None;
        
        for attempt in 0..rpc_endpoints().len() {
            let url = self.get_rpc_url();
            
            log::info!("RPC call attempt {} to {}: {}", attempt + 1, url, method);
            
            let request = RpcRequest {
                jsonrpc: "2.0".to_string(),
                method: method.to_string(),
                params: params.clone(),
                id: 1,
            };

            match self.client.post(url)
                .json(&request)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        if let Ok(rpc_response) = response.json::<RpcResponse>().await {
                            if let Some(result) = rpc_response.result {
                                log::info!("RPC call successful on attempt {}", attempt + 1);
                                return Ok(result);
                            }
                            if let Some(error) = rpc_response.error {
                                log::warn!("RPC error: {} - {}", error.code, error.message);
                                last_error = Some(format!("RPC error: {}", error.message));
                            }
                        }
                    } else {
                        log::warn!("RPC HTTP error: {}", response.status());
                        last_error = Some(format!("HTTP {}", response.status()));
                    }
                }
                Err(e) => {
                    log::warn!("RPC request failed: {}", e);
                    last_error = Some(e.to_string());
                    self.switch_rpc();
                    continue;
                }
            }
            
            self.switch_rpc();
        }

        Err(AppError::BadRequest(
            last_error.unwrap_or_else(|| "All RPC endpoints failed".to_string())
        ))
    }

    pub async fn get_block_number(&self) -> Result<u64, AppError> {
        let result = self.rpc_call("starknet_blockNumber", json!([])).await?;
        
        if let Some(num_str) = result.as_str() {
            let num = u64::from_str_radix(num_str.trim_start_matches("0x"), 16)
                .map_err(|_| AppError::BadRequest("Invalid block number".to_string()))?;
            return Ok(num);
        }
        
        if let Some(num) = result.as_u64() {
            return Ok(num);
        }

        Err(AppError::BadRequest("Invalid block number format".to_string()))
    }

    /// Full block with each transaction's receipt (fee, status, gas).
    pub async fn get_block_with_receipts(&self, block_number: u64) -> Result<Value, AppError> {
        self.rpc_call(
            "starknet_getBlockWithReceipts",
            json!([{"block_number": block_number}]),
        )
        .await
    }

}

impl Default for StarknetRpc {
    fn default() -> Self {
        Self::new()
    }
}
