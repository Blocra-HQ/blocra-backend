use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::errors::AppError;

// RPC endpoints for Starknet
const RPC_ENDPOINTS: &[&str] = &[
    "https://rpc.starknet.lava.build",
    "https://starknet-mainnet.g.alchemy.com/v2/demo",
    "https://starknet-mainnet.public.blastapi.io",
    "https://free-rpc.nethermind.io/mainnet-juno",
];

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

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInfo {
    pub block_number: u64,
    pub timestamp: u64,
    pub transactions: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventData {
    pub block_number: u64,
    pub transaction_hash: String,
    pub keys: Vec<String>,
    pub data: Vec<String>,
    pub event_name: String,
    pub decoded_data: Value,
    pub timestamp: String,
    pub timestamp_raw: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractAnalysis {
    pub contract_address: String,
    pub status: String,
    pub transaction_count: usize,
    pub avg_fee: String,
    pub total_fees: String,
    pub unique_senders: usize,
    pub blocks_analyzed: usize,
    pub current_block: u64,
    pub from_block: u64,
    pub to_block: u64,
    pub transactions: Vec<TransactionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub block_number: u64,
    pub transaction_hash: String,
    pub sender_address: String,
    pub contract_address: String,
    pub max_fee: String,
    pub tx_type: String,
    pub timestamp: u64,
}

pub struct RpcService {
    client: Client,
}

impl RpcService {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    fn get_rpc_url(&self) -> &'static str {
        let index = CURRENT_RPC_INDEX.load(Ordering::Relaxed);
        RPC_ENDPOINTS[index % RPC_ENDPOINTS.len()]
    }

    fn switch_rpc(&self) {
        CURRENT_RPC_INDEX.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value, AppError> {
        for _ in 0..RPC_ENDPOINTS.len() {
            let url = self.get_rpc_url();
            
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
                    if let Ok(rpc_response) = response.json::<RpcResponse>().await {
                        if let Some(result) = rpc_response.result {
                            return Ok(result);
                        }
                    }
                }
                Err(_) => {
                    self.switch_rpc();
                    continue;
                }
            }
        }

        Err(AppError::BadRequest("All RPC endpoints failed".to_string()))
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

    pub async fn get_block_with_txs(&self, block_number: u64) -> Result<BlockInfo, AppError> {
        let result = self.rpc_call(
            "starknet_getBlockWithTxs",
            json!([{"block_number": block_number}])
        ).await?;

        let block_num = result.get("block_number")
            .and_then(|v| v.as_u64())
            .unwrap_or(block_number);
        
        let timestamp = result.get("timestamp")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        
        let transactions = result.get("transactions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.clone())
            .unwrap_or_default();

        Ok(BlockInfo {
            block_number: block_num,
            timestamp,
            transactions,
        })
    }

    pub async fn find_block_by_timestamp(&self, target_timestamp: u64) -> Result<u64, AppError> {
        let latest = self.get_block_number().await?;
        let mut low = 0u64;
        let mut high = latest;

        while low <= high {
            let mid = (low + high) / 2;
            
            match self.get_block_with_txs(mid).await {
                Ok(block) => {
                    if block.timestamp < target_timestamp {
                        low = mid + 1;
                    } else if block.timestamp > target_timestamp {
                        if mid == 0 {
                            break;
                        }
                        high = mid - 1;
                    } else {
                        return Ok(mid);
                    }
                }
                Err(_) => break,
            }
        }

        Ok(low)
    }

    pub async fn get_events(
        &self,
        contract_address: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<EventData>, AppError> {
        println!("🚀 UNLIMITED MODE: Fetching ALL events for contract: {} from block {} to {}", contract_address, from_block, to_block);
        
        let mut all_events = Vec::new();
        let mut continuation_token: Option<String> = None;
        let mut page_count = 0;
        let max_pages = 100; // Safety limit

        loop {
            page_count += 1;
            println!("📄 Fetching page {}...", page_count);

            let mut params = json!({
                "filter": {
                    "from_block": {"block_number": from_block},
                    "to_block": {"block_number": to_block},
                    "address": contract_address,
                    "chunk_size": 1000
                }
            });

            // Add continuation token if we have one
            if let Some(token) = &continuation_token {
                params["filter"]["continuation_token"] = json!(token);
            }

            let result = self.rpc_call("starknet_getEvents", params).await?;
            
            let events = result.get("events")
                .and_then(|v| v.as_array())
                .ok_or(AppError::BadRequest("Invalid events response".to_string()))?;

            println!("   ✅ Page {}: {} events (Total: {})", page_count, events.len(), all_events.len() + events.len());

            // Check for continuation token
            continuation_token = result.get("continuation_token")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Process events from this page
            for event in events {
                let block_number = event.get("block_number")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                
                let tx_hash = event.get("transaction_hash")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let keys: Vec<String> = event.get("keys")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|k| k.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let data: Vec<String> = event.get("data")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|d| d.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                // Decode event
                let (event_name, decoded_data) = self.decode_event(&keys, &data);

                // We'll estimate timestamp later in batch
                all_events.push((block_number, tx_hash, keys, data, event_name, decoded_data));
            }

            // Check if we should continue
            if continuation_token.is_none() {
                println!("   ✅ No more pages. Pagination complete!");
                break;
            }

            if page_count >= max_pages {
                println!("   ⚠️ Reached maximum page limit ({}). Stopping pagination.", max_pages);
                break;
            }

            println!("   🔄 More events available, fetching next page...");
        }

        println!("🎉 COMPLETE! Fetched {} total events across {} pages", all_events.len(), page_count);

        // Now fetch timestamps for interpolation
        let latest_block = self.get_block_with_txs(to_block).await?;
        let from_block_data = self.get_block_with_txs(from_block).await?;
        
        let latest_timestamp = latest_block.timestamp;
        let from_timestamp = from_block_data.timestamp;
        let total_diff = to_block.saturating_sub(from_block);
        let time_diff = latest_timestamp.saturating_sub(from_timestamp);

        // Convert to EventData with timestamps
        let decoded_events: Vec<EventData> = all_events.into_iter().map(|(block_number, tx_hash, keys, data, event_name, decoded_data)| {
            // Estimate timestamp
            let block_diff = to_block.saturating_sub(block_number);
            let estimated_timestamp = if total_diff > 0 {
                latest_timestamp.saturating_sub((block_diff * time_diff) / total_diff)
            } else {
                latest_timestamp
            };

            EventData {
                block_number,
                transaction_hash: tx_hash,
                keys,
                data,
                event_name,
                decoded_data,
                timestamp: chrono::DateTime::from_timestamp(estimated_timestamp as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                timestamp_raw: estimated_timestamp,
            }
        }).collect();

        Ok(decoded_events)
    }

    fn decode_event(&self, keys: &[String], data: &[String]) -> (String, Value) {
        if keys.is_empty() {
            return ("Unknown Event".to_string(), json!({}));
        }

        let key = &keys[0];

        // Transfer event
        if key == "0x99cd8bde557814842a3121e8ddfd433a539b8c9f14bf31ebf108d12e6196e9" {
            if data.len() >= 3 {
                return ("Transfer".to_string(), json!({
                    "from": data[0],
                    "to": data[1],
                    "amount": u128::from_str_radix(data[2].trim_start_matches("0x"), 16)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| data[2].clone())
                }));
            }
        }

        // Approval event
        if key == "0x1dcde06aabdbca2f80aa51392b345d7549d7757aa855f7e37f5d335ac8243b1" {
            if data.len() >= 3 {
                return ("Approval".to_string(), json!({
                    "owner": data[0],
                    "spender": data[1],
                    "amount": u128::from_str_radix(data[2].trim_start_matches("0x"), 16)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| data[2].clone())
                }));
            }
        }

        ("Unknown Event".to_string(), json!({}))
    }

    pub async fn analyze_contract(
        &self,
        contract_address: &str,
        from_timestamp: Option<u64>,
        to_timestamp: Option<u64>,
    ) -> Result<ContractAnalysis, AppError> {
        let current_block = self.get_block_number().await?;
        
        let from_block = if let Some(ts) = from_timestamp {
            self.find_block_by_timestamp(ts).await?
        } else {
            current_block.saturating_sub(1000)
        };

        let to_block = if let Some(ts) = to_timestamp {
            self.find_block_by_timestamp(ts).await?
        } else {
            current_block
        };

        // UNLIMITED MODE: No artificial limits - search the entire range requested
        let search_blocks = to_block - from_block + 1;
        let mut contract_transactions = Vec::new();
        
        println!("🔍 Analyzing {} blocks for contract transactions", search_blocks);

        for i in 0..search_blocks {
            let block_num = to_block - i;
            if block_num < from_block {
                break;
            }

            if let Ok(block) = self.get_block_with_txs(block_num).await {
                for tx in block.transactions {
                    let sender = tx.get("sender_address")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    
                    let contract = tx.get("contract_address")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if sender == contract_address || contract == contract_address {
                        let tx_hash = tx.get("transaction_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        
                        let max_fee = tx.get("max_fee")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0x0")
                            .to_string();

                        let tx_type = tx.get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("INVOKE")
                            .to_string();

                        contract_transactions.push(TransactionInfo {
                            block_number: block_num,
                            transaction_hash: tx_hash,
                            sender_address: sender.to_string(),
                            contract_address: contract_address.to_string(),
                            max_fee,
                            tx_type,
                            timestamp: block.timestamp,
                        });
                    }
                }
            }
        }

        if contract_transactions.is_empty() {
            return Ok(ContractAnalysis {
                contract_address: contract_address.to_string(),
                status: "No Recent Activity".to_string(),
                transaction_count: 0,
                avg_fee: "0".to_string(),
                total_fees: "0".to_string(),
                unique_senders: 0,
                blocks_analyzed: search_blocks as usize,
                current_block,
                from_block,
                to_block,
                transactions: vec![],
            });
        }

        let total_fees: u128 = contract_transactions.iter()
            .filter_map(|tx| u128::from_str_radix(tx.max_fee.trim_start_matches("0x"), 16).ok())
            .sum();

        let avg_fee = total_fees / contract_transactions.len() as u128;
        let unique_senders = contract_transactions.iter()
            .map(|tx| tx.sender_address.clone())
            .collect::<std::collections::HashSet<_>>()
            .len();

        Ok(ContractAnalysis {
            contract_address: contract_address.to_string(),
            status: "Active".to_string(),
            transaction_count: contract_transactions.len(),
            avg_fee: format!("{:.6}", avg_fee as f64 / 1e18),
            total_fees: format!("{:.4}", total_fees as f64 / 1e18),
            unique_senders,
            blocks_analyzed: search_blocks as usize,
            current_block,
            from_block,
            to_block,
            transactions: contract_transactions.into_iter().take(10).collect(),
        })
    }
}

impl Default for RpcService {
    fn default() -> Self {
        Self::new()
    }
}
