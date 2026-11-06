use crate::{db::DbPool, utils::jwt, errors::AppError, services::rpc::RpcService};
use crate::services::stellar::{HorizonOperation, HorizonTransaction, LedgerInfo, StellarService};
use crate::services::chains::{ChainInfo, ChainRpc, DEFAULT_TIMEOUT};
use actix_web::HttpRequest;
use serde_json::{json, Value};

pub async fn get_dashboard_data(pool: &DbPool, req: &HttpRequest) -> Result<Value, AppError> {
    let user_id = jwt::extract_user_id(req)?;

    // contract_queries.user_id is TEXT, dashboards.user_id is INTEGER.
    let queries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contract_queries WHERE user_id = ?")
        .bind(user_id.to_string()).fetch_one(pool).await?;
    let dashboards: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dashboards WHERE user_id = ?")
        .bind(user_id).fetch_one(pool).await?;

    Ok(json!({
        "queries": queries,
        "dashboards": dashboards
    }))
}

pub async fn get_analytics(pool: &DbPool) -> Result<Value, AppError> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    let queries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contract_queries").fetch_one(pool).await?;
    let dashboards: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dashboards").fetch_one(pool).await?;

    Ok(json!({
        "total_users": users,
        "total_queries": queries,
        "total_dashboards": dashboards
    }))
}

// New endpoint: Get all blockchain stats from a single block
/// Stats for the latest Starknet block, all taken from the block's receipts.
/// Nothing is estimated: fees are the receipts' actual fees, success and
/// failure come from execution_status, gas from execution_resources.
pub async fn get_blockchain_stats(rpc: &RpcService) -> Result<Value, AppError> {
    let block_number = rpc.get_block_number().await?;
    let block = rpc.get_block_with_receipts(block_number).await?;
    Ok(summarize_block(block_number, &block))
}

fn hex_u128(v: Option<&Value>) -> u128 {
    v.and_then(|v| v.as_str())
        .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

/// Pure so it can be tested without an RPC.
pub fn summarize_block(block_number: u64, block: &Value) -> Value {
    let timestamp = block.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
    let txs = block.get("transactions").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut senders = std::collections::HashSet::new();
    let (mut fees_fri, mut fees_wei) = (0u128, 0u128);
    let (mut succeeded, mut reverted) = (0u64, 0u64);
    let (mut l2_gas, mut l1_gas, mut l1_data_gas) = (0u64, 0u64, 0u64);
    let mut tx_types: std::collections::BTreeMap<String, u64> = Default::default();

    for entry in &txs {
        let tx = entry.get("transaction").unwrap_or(&Value::Null);
        let receipt = entry.get("receipt").unwrap_or(&Value::Null);
        if let Some(sender) = tx.get("sender_address").and_then(|v| v.as_str()) {
            senders.insert(sender.to_string());
        }
        let kind = tx.get("type").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        *tx_types.entry(kind.to_string()).or_default() += 1;
        let fee = &receipt["actual_fee"];
        match fee.get("unit").and_then(|u| u.as_str()) {
            Some("WEI") => fees_wei += hex_u128(fee.get("amount")),
            _ => fees_fri += hex_u128(fee.get("amount")),
        }
        match receipt.get("execution_status").and_then(|v| v.as_str()) {
            Some("REVERTED") => reverted += 1,
            _ => succeeded += 1,
        }
        let res = &receipt["execution_resources"];
        l2_gas += res["l2_gas"].as_u64().unwrap_or(0);
        l1_gas += res["l1_gas"].as_u64().unwrap_or(0);
        l1_data_gas += res["l1_data_gas"].as_u64().unwrap_or(0);
    }

    let n = txs.len();
    let total_fees_strk = fees_fri as f64 / 1e18;
    json!({
        "success": true,
        "block_number": block_number,
        "timestamp": timestamp,
        "stats": {
            "total_transactions": n,
            "unique_senders": senders.len(),
            "active_users": senders.len(),
            "successful_txs": succeeded,
            "failed_txs": reverted,
            // Fees paid in STRK (FRI); the few legacy ETH-fee txs are reported separately.
            "total_fees": total_fees_strk.to_string(),
            "avg_fee": if n > 0 { (total_fees_strk / n as f64).to_string() } else { "0".to_string() },
            "fee_unit": "STRK",
            "total_fees_eth": (fees_wei as f64 / 1e18).to_string(),
            "gas_used": l2_gas + l1_gas,
            "l2_gas": l2_gas,
            "l1_gas": l1_gas,
            "l1_data_gas": l1_data_gas,
            // INVOKE / DECLARE / DEPLOY_ACCOUNT / L1_HANDLER counts.
            "tx_types": tx_types
        },
        "block_info": {
            "block_number": block_number,
            "timestamp": timestamp,
            "transaction_count": n
        }
    })
}

/// Stats for the latest Stellar ledger from Horizon: transactions (failed
/// included), fees actually charged, source accounts and operation types.
pub async fn get_stellar_stats(stellar: &StellarService) -> Result<Value, AppError> {
    let ledger = stellar.latest_ledger().await?;
    let (txs, ops) = tokio::try_join!(
        stellar.ledger_transactions(ledger.sequence),
        stellar.ledger_operations(ledger.sequence),
    )?;
    Ok(summarize_ledger(&ledger, &txs, &ops, &stellar.network))
}

/// Pure so it can be tested without Horizon.
pub fn summarize_ledger(
    ledger: &LedgerInfo,
    txs: &[HorizonTransaction],
    ops: &[HorizonOperation],
    network: &str,
) -> Value {
    let timestamp = chrono::DateTime::parse_from_rfc3339(&ledger.closed_at)
        .map(|t| t.timestamp() as u64)
        .unwrap_or(0);
    let senders: std::collections::HashSet<&str> = txs.iter().map(|t| t.source_account.as_str()).collect();
    let fees_stroops: u64 = txs
        .iter()
        .filter_map(|t| t.fee_charged.as_deref()?.parse::<u64>().ok())
        .sum();
    let mut op_types: std::collections::BTreeMap<String, u64> = Default::default();
    for op in ops {
        *op_types.entry(op.operation_type.clone()).or_default() += 1;
    }

    // Horizon's ledger counts are authoritative; transactions_sampled shows
    // how many of them the per-transaction figures cover.
    let total = (ledger.successful_transaction_count + ledger.failed_transaction_count) as usize;
    let total_fees = fees_stroops as f64 / 1e7;
    json!({
        "success": true,
        "chain": "stellar",
        "network": network,
        "block_number": ledger.sequence,
        "timestamp": timestamp,
        "stats": {
            "total_transactions": total,
            "unique_senders": senders.len(),
            "active_users": senders.len(),
            "successful_txs": ledger.successful_transaction_count,
            "failed_txs": ledger.failed_transaction_count,
            "total_fees": total_fees.to_string(),
            "avg_fee": if txs.is_empty() { "0".to_string() } else { (total_fees / txs.len() as f64).to_string() },
            "fee_unit": "XLM",
            "operations": ledger.operation_count,
            // Operation types (payment, invoke_host_function, ...) per ledger.
            "tx_types": op_types,
            "base_fee_stroops": ledger.base_fee,
            "protocol_version": ledger.protocol_version,
            "transactions_sampled": txs.len()
        },
        "block_info": {
            "block_number": ledger.sequence,
            "timestamp": timestamp,
            "transaction_count": total
        }
    })
}

// ── Registry chains (EVM, Solana, CometBFT) ────────────────────────────────

/// Stats for the latest block of a registry chain, dispatched on its type.
pub async fn get_chain_stats(rpc: &ChainRpc, id: &str, chain: &ChainInfo) -> Result<Value, AppError> {
    match chain.kind.as_str() {
        "evm" => get_evm_stats(rpc, id, chain).await,
        "solana" => get_solana_stats(rpc, id, chain).await,
        "cometbft" => get_cometbft_stats(rpc, id, chain).await,
        other => Err(AppError::BadRequest(format!("No stats reader for chain type '{other}'"))),
    }
}

/// A head block older than this means the node has stopped syncing; its
/// endpoint is skipped rather than shown as the latest block.
const STALE_AFTER_SECS: u64 = 3600;

fn check_fresh(timestamp: Option<u64>) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp() as u64;
    match timestamp {
        Some(t) if now.saturating_sub(t) > STALE_AFTER_SECS => {
            Err(format!("head block is {}h old; node not synced", now.saturating_sub(t) / 3600))
        }
        Some(_) => Ok(()),
        None => Err("block has no timestamp".to_string()),
    }
}

fn hex_u64(v: Option<&Value>) -> Option<u64> {
    v.and_then(|v| v.as_str())
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

fn hex_opt_u128(v: Option<&Value>) -> Option<u128> {
    v.and_then(|v| v.as_str())
        .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

/// Latest EVM block with full transactions and their receipts, from
/// `eth_getBlockReceipts` or, where no endpoint serves it, one
/// `eth_getTransactionReceipt` per transaction.
pub async fn get_evm_stats(rpc: &ChainRpc, id: &str, chain: &ChainInfo) -> Result<Value, AppError> {
    let block = rpc
        .call_checked(id, chain, "eth_getBlockByNumber", json!(["latest", true]), DEFAULT_TIMEOUT, |b| {
            check_fresh(hex_u64(b.get("timestamp")))
        })
        .await?;
    let number = block.get("number").cloned().unwrap_or(Value::Null);
    let hash = block.get("hash").and_then(|h| h.as_str()).unwrap_or_default().to_lowercase();
    let txs = block["transactions"].as_array().cloned().unwrap_or_default();
    let receipts = if txs.is_empty() {
        Some(Vec::new())
    } else {
        match evm_block_receipts(rpc, id, chain, &number, &hash, txs.len()).await {
            Some(r) => Some(r),
            None => evm_receipts_one_by_one(rpc, id, chain, &txs, &hash).await,
        }
    };
    Ok(summarize_evm_block(id, &chain.currency, &block, receipts.as_deref()))
}

/// Receipts must belong to this block: a node on another fork or behind the
/// head is skipped so the figures never mix two blocks.
fn receipt_matches(receipt: &Value, hash: &str) -> Result<(), String> {
    match receipt["blockHash"].as_str() {
        Some(h) if h.to_lowercase() != hash => Err("receipt is for another block".to_string()),
        _ => Ok(()),
    }
}

/// All receipts in one `eth_getBlockReceipts` call, when an endpoint serves it.
async fn evm_block_receipts(
    rpc: &ChainRpc,
    id: &str,
    chain: &ChainInfo,
    number: &Value,
    hash: &str,
    tx_count: usize,
) -> Option<Vec<Value>> {
    let result = rpc
        .call_checked(id, chain, "eth_getBlockReceipts", json!([number]), DEFAULT_TIMEOUT, |r| {
            let list = r.as_array().ok_or("receipts are not a list")?;
            if list.len() != tx_count {
                return Err(format!("{} receipts for {tx_count} transactions", list.len()));
            }
            list.first().map_or(Ok(()), |r| receipt_matches(r, hash))
        })
        .await;
    match result {
        Ok(r) => r.as_array().cloned(),
        Err(e) => {
            log::info!("{id}: eth_getBlockReceipts unavailable ({e})");
            None
        }
    }
}

/// Most transactions a block may have for receipts to be fetched one by one.
const MAX_SINGLE_RECEIPTS: usize = 200;

/// Falls back to `eth_getTransactionReceipt` per transaction for chains whose
/// RPCs lack `eth_getBlockReceipts`. All or nothing: one missing receipt and
/// the block is reported without receipt figures.
async fn evm_receipts_one_by_one(
    rpc: &ChainRpc,
    id: &str,
    chain: &ChainInfo,
    txs: &[Value],
    hash: &str,
) -> Option<Vec<Value>> {
    use futures::StreamExt;
    if txs.len() > MAX_SINGLE_RECEIPTS {
        return None;
    }
    let results: Vec<Result<Value, AppError>> = futures::stream::iter(txs.iter().map(|tx| {
        let tx_hash = tx["hash"].clone();
        async move {
            rpc.call_checked(id, chain, "eth_getTransactionReceipt", json!([tx_hash]), DEFAULT_TIMEOUT, |r| {
                receipt_matches(r, hash)
            })
            .await
        }
    }))
    .buffered(8)
    .collect()
    .await;
    match results.into_iter().collect::<Result<Vec<_>, _>>() {
        Ok(receipts) => Some(receipts),
        Err(e) => {
            log::warn!("{id}: transaction receipts unavailable ({e})");
            None
        }
    }
}

fn evm_tx_type(t: Option<&Value>) -> String {
    match hex_u64(t) {
        Some(0) | None => "LEGACY".into(),
        Some(1) => "ACCESS_LIST".into(),
        Some(2) => "EIP1559".into(),
        Some(3) => "BLOB".into(),
        Some(4) => "SET_CODE".into(),
        Some(0x7e) => "DEPOSIT".into(),
        // zkSync
        Some(0x71) => "EIP712".into(),
        Some(0xff) => "PRIORITY".into(),
        // Celo: fee paid in another token
        Some(0x7b) => "CIP64".into(),
        // Arbitrum
        Some(0x64) => "ARB_DEPOSIT".into(),
        Some(0x68) => "ARB_RETRY".into(),
        Some(0x69) => "ARB_RETRYABLE".into(),
        Some(0x6a) => "ARB_INTERNAL".into(),
        Some(n) => format!("TYPE_0X{n:X}"),
    }
}

/// Pure so it can be tested without an RPC. `receipts` is `None` when no
/// endpoint served them; fees and success/failure are then null, never guessed.
pub fn summarize_evm_block(chain_id: &str, currency: &str, block: &Value, receipts: Option<&[Value]>) -> Value {
    let block_number = hex_u64(block.get("number")).unwrap_or(0);
    let timestamp = hex_u64(block.get("timestamp")).unwrap_or(0);
    let txs = block.get("transactions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let n = txs.len();

    let mut senders = std::collections::HashSet::new();
    let mut tx_types: std::collections::BTreeMap<String, u64> = Default::default();
    let mut tx_kinds: std::collections::BTreeMap<&str, u64> = Default::default();
    let mut paid_in_other_token = 0u64;
    for tx in &txs {
        if let Some(from) = tx.get("from").and_then(|v| v.as_str()) {
            senders.insert(from.to_lowercase());
        }
        *tx_types.entry(evm_tx_type(tx.get("type"))).or_default() += 1;
        let input = tx.get("input").and_then(|v| v.as_str()).unwrap_or("0x");
        let kind = if tx.get("to").is_none_or(|t| t.is_null()) {
            "contract_creation"
        } else if input == "0x" || input.is_empty() {
            "transfer"
        } else {
            "call"
        };
        *tx_kinds.entry(kind).or_default() += 1;
        // Celo lets a transaction pay its fee in another token (CIP-64).
        if tx.get("feeCurrency").is_some_and(|f| !f.is_null()) {
            paid_in_other_token += 1;
        }
    }

    let mut stats = json!({
        "total_transactions": n,
        "unique_senders": senders.len(),
        "active_users": senders.len(),
        "fee_unit": currency,
        "gas_used": hex_u64(block.get("gasUsed")).unwrap_or(0),
        "gas_limit": hex_u64(block.get("gasLimit")),
        "base_fee_per_gas": hex_opt_u128(block.get("baseFeePerGas")).map(|v| v.to_string()),
        // EIP-2718 envelope types (LEGACY / EIP1559 / BLOB / DEPOSIT ...).
        "tx_types": tx_types,
        // transfer = no calldata, call = calldata to an address, contract_creation = no recipient.
        "tx_kinds": tx_kinds,
    });

    match receipts {
        Some(receipts) => {
            let (mut ok, mut failed) = (0u64, 0u64);
            let mut fees_wei = 0u128;
            let mut unpriced = 0usize;
            for (i, r) in receipts.iter().enumerate() {
                match hex_u64(r.get("status")) {
                    Some(0) => failed += 1,
                    _ => ok += 1,
                }
                if txs.get(i).and_then(|t| t.get("feeCurrency")).is_some_and(|f| !f.is_null()) {
                    continue;
                }
                let gas = hex_opt_u128(r.get("gasUsed"));
                // Legacy and access-list transactions pay exactly gasPrice.
                let price = hex_opt_u128(r.get("effectiveGasPrice")).or_else(|| {
                    let tx = txs.get(i)?;
                    matches!(hex_u64(tx.get("type")), None | Some(0) | Some(1))
                        .then(|| hex_opt_u128(tx.get("gasPrice")))
                        .flatten()
                });
                let (Some(gas), Some(price)) = (gas, price) else {
                    unpriced += 1;
                    continue;
                };
                let mut fee = gas.saturating_mul(price);
                // Rollup L1 data fee, charged on top of execution gas.
                fee = fee.saturating_add(hex_opt_u128(r.get("l1Fee")).unwrap_or(0));
                // Blob gas, only when the receipt gives both the amount and its price.
                if let (Some(blob_gas), Some(blob_price)) =
                    (hex_opt_u128(r.get("blobGasUsed")), hex_opt_u128(r.get("blobGasPrice")))
                {
                    fee = fee.saturating_add(blob_gas.saturating_mul(blob_price));
                }
                fees_wei = fees_wei.saturating_add(fee);
            }
            stats["successful_txs"] = json!(ok);
            stats["failed_txs"] = json!(failed);
            if unpriced == 0 {
                let total = fees_wei as f64 / 1e18;
                let priced = n - paid_in_other_token as usize;
                stats["total_fees"] = json!(total.to_string());
                stats["avg_fee"] = json!(if priced > 0 { (total / priced as f64).to_string() } else { "0".to_string() });
            } else {
                stats["total_fees"] = Value::Null;
                stats["avg_fee"] = Value::Null;
                stats["partial"] = json!(format!("{unpriced} receipts lack gas price; fees unavailable"));
            }
            if paid_in_other_token > 0 {
                // Their fees are in other tokens and are left out of total_fees.
                stats["non_native_fee_txs"] = json!(paid_in_other_token);
            }
        }
        None => {
            stats["successful_txs"] = Value::Null;
            stats["failed_txs"] = Value::Null;
            stats["total_fees"] = Value::Null;
            stats["avg_fee"] = Value::Null;
            stats["partial"] = json!("receipts unavailable");
        }
    }

    json!({
        "success": true,
        "chain": chain_id,
        "network": "mainnet",
        "block_number": block_number,
        "timestamp": timestamp,
        "stats": stats,
        "block_info": {
            "block_number": block_number,
            "timestamp": timestamp,
            "transaction_count": n
        }
    })
}

const SOLANA_VOTE_PROGRAM: &str = "Vote111111111111111111111111111111111111111";

/// Latest confirmed Solana block. Slots can be skipped or not yet served, so
/// it steps back a few slots before giving up.
pub async fn get_solana_stats(rpc: &ChainRpc, id: &str, chain: &ChainInfo) -> Result<Value, AppError> {
    let slot = rpc
        .call(id, chain, "getSlot", json!([{"commitment": "confirmed"}]), DEFAULT_TIMEOUT)
        .await?
        .as_u64()
        .ok_or_else(|| AppError::BadRequest("Solana getSlot returned no slot".into()))?;
    let options = json!({
        "commitment": "confirmed",
        "transactionDetails": "full",
        "maxSupportedTransactionVersion": 1,
        "rewards": false,
        "encoding": "json"
    });
    let mut last_err = None;
    for candidate in (slot.saturating_sub(5)..=slot).rev() {
        // Full blocks run to a few MB, hence the longer timeout.
        match rpc
            .call(id, chain, "getBlock", json!([candidate, options]), std::time::Duration::from_secs(25))
            .await
        {
            Ok(block) => return Ok(summarize_solana_block(id, candidate, &block)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::BadRequest("Solana getBlock failed".into())))
}

/// Pure so it can be tested without an RPC.
pub fn summarize_solana_block(chain_id: &str, slot: u64, block: &Value) -> Value {
    let timestamp = block.get("blockTime").and_then(|v| v.as_u64()).unwrap_or(0);
    let txs = block.get("transactions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let n = txs.len();

    let mut payers = std::collections::HashSet::new();
    let mut tx_types: std::collections::BTreeMap<String, u64> = Default::default();
    let (mut ok, mut failed, mut votes) = (0u64, 0u64, 0u64);
    let mut fees_lamports = 0u64;
    let mut compute_units: Option<u64> = None;
    for entry in &txs {
        let meta = &entry["meta"];
        let message = &entry["transaction"]["message"];
        let keys = message["accountKeys"].as_array().cloned().unwrap_or_default();
        // The first account key is the fee payer.
        if let Some(payer) = keys.first().and_then(|k| k.as_str()) {
            payers.insert(payer.to_string());
        }
        if meta["err"].is_null() {
            ok += 1;
        } else {
            failed += 1;
        }
        fees_lamports += meta["fee"].as_u64().unwrap_or(0);
        if let Some(cu) = meta["computeUnitsConsumed"].as_u64() {
            *compute_units.get_or_insert(0) += cu;
        }
        let is_vote = message["instructions"].as_array().is_some_and(|ixs| {
            ixs.iter().any(|ix| {
                ix["programIdIndex"]
                    .as_u64()
                    .and_then(|i| keys.get(i as usize))
                    .and_then(|k| k.as_str())
                    == Some(SOLANA_VOTE_PROGRAM)
            })
        });
        if is_vote {
            votes += 1;
        }
        let version = match &entry["version"] {
            Value::Number(v) => format!("V{v}"),
            _ => "LEGACY".to_string(),
        };
        *tx_types.entry(version).or_default() += 1;
    }

    let total = fees_lamports as f64 / 1e9;
    json!({
        "success": true,
        "chain": chain_id,
        "network": "mainnet",
        "block_number": slot,
        "timestamp": timestamp,
        "stats": {
            "total_transactions": n,
            "unique_senders": payers.len(),
            "active_users": payers.len(),
            "successful_txs": ok,
            "failed_txs": failed,
            "total_fees": total.to_string(),
            "avg_fee": if n > 0 { (total / n as f64).to_string() } else { "0".to_string() },
            "fee_unit": "SOL",
            // Compute units consumed, Solana's counterpart to gas.
            "gas_used": compute_units,
            "vote_txs": votes,
            "block_height": block.get("blockHeight").and_then(|v| v.as_u64()),
            // Transaction message versions (LEGACY / V0 / V1).
            "tx_types": tx_types
        },
        "block_info": {
            "block_number": slot,
            "timestamp": timestamp,
            "transaction_count": n
        }
    })
}

/// Latest CometBFT block and its execution results.
pub async fn get_cometbft_stats(rpc: &ChainRpc, id: &str, chain: &ChainInfo) -> Result<Value, AppError> {
    let block = rpc
        .call_checked(id, chain, "block", json!({}), DEFAULT_TIMEOUT, |b| {
            check_fresh(
                b["block"]["header"]["time"]
                    .as_str()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.timestamp() as u64),
            )
        })
        .await?;
    let height = block["block"]["header"]["height"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest(format!("{} block has no height", chain.name)))?
        .to_string();
    let results = rpc
        .call(id, chain, "block_results", json!({"height": height}), DEFAULT_TIMEOUT)
        .await?;
    Ok(summarize_cometbft_block(id, &chain.currency, &block, &results))
}

/// Event attributes as plain strings. CometBFT 0.34 base64-encodes keys and
/// values; later versions don't. Only the keys in `wanted` are returned.
fn event_attrs(event: &Value, wanted: &[&str]) -> Vec<(String, String)> {
    use base64::Engine;
    let decode = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    };
    let mut out = Vec::new();
    for attr in event["attributes"].as_array().into_iter().flatten() {
        let key = attr["key"].as_str().unwrap_or_default();
        let value = attr["value"].as_str().unwrap_or_default();
        if wanted.contains(&key) {
            out.push((key.to_string(), value.to_string()));
        } else if let Some(k) = decode(key).filter(|k| wanted.contains(&k.as_str())) {
            out.push((k, decode(value).unwrap_or_default()));
        }
    }
    out
}

/// The amount of a fee string such as "1861uaxl" in whole native units, or
/// `None` when it is in a token other than the chain's currency.
fn native_fee(fee: &str, currency: &str) -> Option<f64> {
    let mut total = 0.0;
    for coin in fee.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let split = coin.find(|c: char| !c.is_ascii_digit())?;
        let (amount, denom) = coin.split_at(split);
        let amount: u128 = amount.parse().ok()?;
        let cur = currency.to_lowercase();
        let scale = if denom == format!("u{cur}") {
            1e6
        } else if denom == format!("a{cur}") {
            1e18
        } else {
            return None;
        };
        total += amount as f64 / scale;
    }
    Some(total)
}

/// Pure so it can be tested without an RPC. `block` and `results` are the
/// `result` objects of CometBFT's `block` and `block_results`.
pub fn summarize_cometbft_block(chain_id: &str, currency: &str, block: &Value, results: &Value) -> Value {
    let header = &block["block"]["header"];
    let block_number: u64 = header["height"].as_str().and_then(|h| h.parse().ok()).unwrap_or(0);
    let timestamp = header["time"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or(0, |t| t.timestamp() as u64);
    let n = block["block"]["data"]["txs"].as_array().map_or(0, |t| t.len());
    let tx_results = results["txs_results"].as_array().cloned().unwrap_or_default();

    let mut senders = std::collections::HashSet::new();
    let mut tx_types: std::collections::BTreeMap<String, u64> = Default::default();
    let (mut ok, mut failed) = (0u64, 0u64);
    let (mut gas_used, mut gas_wanted) = (0u64, 0u64);
    let mut fees = 0.0;
    let mut unreadable_fees = 0usize;
    for r in &tx_results {
        let succeeded = r["code"].as_u64().unwrap_or(0) == 0;
        if succeeded {
            ok += 1;
        } else {
            failed += 1;
        }
        gas_used += r["gas_used"].as_str().and_then(|g| g.parse::<u64>().ok()).unwrap_or(0);
        gas_wanted += r["gas_wanted"].as_str().and_then(|g| g.parse::<u64>().ok()).unwrap_or(0);

        let (mut fee, mut sender, mut action) = (None, None, None);
        for event in r["events"].as_array().into_iter().flatten() {
            match event["type"].as_str() {
                Some("tx") => {
                    for (k, v) in event_attrs(event, &["fee", "fee_payer", "acc_seq"]) {
                        match k.as_str() {
                            "fee" => fee = Some(v),
                            "fee_payer" => sender = Some(v),
                            // "address/sequence" names the signer when no fee_payer is emitted.
                            "acc_seq" if sender.is_none() => {
                                sender = v.split('/').next().map(str::to_string)
                            }
                            _ => {}
                        }
                    }
                }
                Some("message") => {
                    for (k, v) in event_attrs(event, &["action", "sender"]) {
                        if k == "action" && action.is_none() {
                            action = Some(v);
                        } else if k == "sender" && sender.is_none() {
                            sender = Some(v);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = sender.filter(|s| !s.is_empty()) {
            senders.insert(s);
        }
        // First message type, e.g. "/ibc.core.client.v1.MsgUpdateClient" -> "MsgUpdateClient".
        let kind = action
            .map(|a| a.rsplit('.').next().unwrap_or(&a).to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string());
        *tx_types.entry(kind).or_default() += 1;
        match fee.as_deref().map(|f| native_fee(f, currency)) {
            Some(Some(f)) => fees += f,
            // A transaction rejected before fee deduction emits no fee event and pays nothing.
            None if !succeeded => {}
            _ => unreadable_fees += 1,
        }
    }

    let fees_known = unreadable_fees == 0 && tx_results.len() == n;
    let mut stats = json!({
        "total_transactions": n,
        "unique_senders": senders.len(),
        "active_users": senders.len(),
        "successful_txs": ok,
        "failed_txs": failed,
        "total_fees": if fees_known { json!(fees.to_string()) } else { Value::Null },
        "avg_fee": if !fees_known { Value::Null } else if n > 0 { json!((fees / n as f64).to_string()) } else { json!("0") },
        "fee_unit": currency,
        "gas_used": gas_used,
        "gas_wanted": gas_wanted,
        "chain_id": header["chain_id"],
        // First message type of each transaction.
        "tx_types": tx_types
    });
    if !fees_known {
        stats["partial"] = json!(format!("{unreadable_fees} transaction fees not in {currency} or not reported"));
    }

    json!({
        "success": true,
        "chain": chain_id,
        "network": "mainnet",
        "block_number": block_number,
        "timestamp": timestamp,
        "stats": stats,
        "block_info": {
            "block_number": block_number,
            "timestamp": timestamp,
            "transaction_count": n
        }
    })
}

#[cfg(test)]
mod tests {
    use super::summarize_block;
    use serde_json::json;

    #[test]
    fn summarizes_from_receipts_only() {
        let block = json!({
            "timestamp": 100,
            "transactions": [
                {"transaction": {"sender_address": "0xa", "type": "INVOKE"},
                 "receipt": {"actual_fee": {"amount": "0xde0b6b3a7640000", "unit": "FRI"},
                             "execution_status": "SUCCEEDED",
                             "execution_resources": {"l1_gas": 0, "l2_gas": 1000, "l1_data_gas": 64}}},
                {"transaction": {"sender_address": "0xa"},
                 "receipt": {"actual_fee": {"amount": "0xde0b6b3a7640000", "unit": "FRI"},
                             "execution_status": "REVERTED",
                             "execution_resources": {"l1_gas": 10, "l2_gas": 500, "l1_data_gas": 0}}}
            ]
        });
        let out = summarize_block(7, &block);
        let s = &out["stats"];
        assert_eq!(s["total_transactions"], 2);
        assert_eq!(s["unique_senders"], 1);
        assert_eq!(s["successful_txs"], 1);
        assert_eq!(s["failed_txs"], 1);
        assert_eq!(s["total_fees"], "2");
        assert_eq!(s["avg_fee"], "1");
        assert_eq!(s["gas_used"], 1510);
        assert_eq!(s["tx_types"]["INVOKE"], 1);
        assert_eq!(s["tx_types"]["UNKNOWN"], 1);
        assert!(s.get("tvl").is_none() && s.get("volume").is_none());
    }

    #[test]
    fn summarizes_a_stellar_ledger() {
        use super::summarize_ledger;
        use crate::services::stellar::{HorizonOperation, HorizonTransaction, LedgerInfo};
        let ledger = LedgerInfo {
            sequence: 9, hash: "h".into(), successful_transaction_count: 1, failed_transaction_count: 1,
            operation_count: 3, closed_at: "2026-11-05T22:03:06Z".into(), total_coins: "1".into(),
            fee_pool: "1".into(), base_fee: 100, base_reserve: 5_000_000, protocol_version: 23,
        };
        let tx = |ok: bool, src: &str, fee: &str| HorizonTransaction {
            hash: "t".into(), paging_token: "1".into(), ledger: 9, created_at: String::new(), source_account: src.into(),
            fee_account: None, fee_charged: Some(fee.into()), max_fee: None, operation_count: 1, successful: ok,
        };
        let op = |kind: &str| HorizonOperation {
            id: "o".into(), paging_token: "1".into(), transaction_hash: "t".into(), source_account: "G".into(),
            operation_type: kind.into(), created_at: String::new(),
        };
        let out = summarize_ledger(
            &ledger,
            &[tx(true, "GA", "100"), tx(false, "GA", "300")],
            &[op("payment"), op("payment"), op("invoke_host_function")],
            "mainnet",
        );
        let s = &out["stats"];
        assert_eq!(s["total_transactions"], 2);
        assert_eq!(s["failed_txs"], 1);
        assert_eq!(s["unique_senders"], 1);
        assert_eq!(s["total_fees"], "0.00004");
        assert_eq!(s["tx_types"]["payment"], 2);
        assert_eq!(out["timestamp"], 1793916186);
    }

    #[test]
    fn summarizes_an_evm_block_from_receipts() {
        use super::summarize_evm_block;
        let block = json!({
            "number": "0x10", "timestamp": "0x64", "gasUsed": "0x7530", "gasLimit": "0x1c9c380",
            "baseFeePerGas": "0x3b9aca00", "hash": "0xb",
            "transactions": [
                {"from": "0xAA", "to": "0xbb", "input": "0x", "type": "0x2"},
                {"from": "0xaa", "to": "0xcc", "input": "0xa9059cbb", "type": "0x0", "gasPrice": "0x3b9aca00"},
                {"from": "0xdd", "to": null, "input": "0x6080", "type": "0x7e"}
            ]
        });
        let receipts = vec![
            json!({"status": "0x1", "gasUsed": "0x5208", "effectiveGasPrice": "0x3b9aca00", "blockHash": "0xb"}),
            // No effectiveGasPrice: a legacy transaction pays exactly its gasPrice.
            json!({"status": "0x0", "gasUsed": "0x5208", "blockHash": "0xb"}),
            json!({"status": "0x1", "gasUsed": "0x0", "effectiveGasPrice": "0x0", "l1Fee": "0xde0b6b3a7640000",
                   "blobGasUsed": "0x10", "blockHash": "0xb"}),
        ];
        let out = summarize_evm_block("base", "ETH", &block, Some(&receipts));
        let s = &out["stats"];
        assert_eq!(out["chain"], "base");
        assert_eq!(out["block_number"], 16);
        assert_eq!(out["block_info"]["transaction_count"], 3);
        assert_eq!(s["total_transactions"], 3);
        assert_eq!(s["unique_senders"], 2);
        assert_eq!(s["successful_txs"], 2);
        assert_eq!(s["failed_txs"], 1);
        // 2 x 21000 gas at 1 gwei + a 1 ETH L1 fee; blob gas without a price is ignored.
        assert_eq!(s["total_fees"], "1.000042");
        assert_eq!(s["gas_used"], 30000);
        assert_eq!(s["fee_unit"], "ETH");
        assert_eq!(s["tx_types"]["EIP1559"], 1);
        assert_eq!(s["tx_types"]["LEGACY"], 1);
        assert_eq!(s["tx_types"]["DEPOSIT"], 1);
        assert_eq!(s["tx_kinds"]["transfer"], 1);
        assert_eq!(s["tx_kinds"]["call"], 1);
        assert_eq!(s["tx_kinds"]["contract_creation"], 1);
        assert!(s.get("partial").is_none());
    }

    #[test]
    fn evm_block_without_receipts_reports_nulls_not_guesses() {
        use super::summarize_evm_block;
        let block = json!({"number": "0x1", "timestamp": "0x2", "gasUsed": "0x5208",
            "transactions": [{"from": "0xa", "to": "0xb", "input": "0x", "type": "0x2"}]});
        let out = summarize_evm_block("ethereum", "ETH", &block, None);
        let s = &out["stats"];
        assert_eq!(s["total_transactions"], 1);
        assert_eq!(s["gas_used"], 21000);
        assert!(s["total_fees"].is_null() && s["avg_fee"].is_null());
        assert!(s["successful_txs"].is_null() && s["failed_txs"].is_null());
        assert_eq!(s["partial"], "receipts unavailable");
    }

    #[test]
    fn evm_fees_in_other_tokens_are_excluded() {
        use super::summarize_evm_block;
        let block = json!({"number": "0x1", "timestamp": "0x2", "gasUsed": "0x0", "transactions": [
            {"from": "0xa", "to": "0xb", "input": "0x", "type": "0x2"},
            {"from": "0xc", "to": "0xb", "input": "0x", "type": "0x7b", "feeCurrency": "0xusd"}]});
        let receipts = vec![
            json!({"status": "0x1", "gasUsed": "0x1", "effectiveGasPrice": "0xde0b6b3a7640000"}),
            json!({"status": "0x1", "gasUsed": "0x1", "effectiveGasPrice": "0xde0b6b3a7640000"}),
        ];
        let s = &summarize_evm_block("celo", "CELO", &block, Some(&receipts))["stats"];
        assert_eq!(s["total_fees"], "1");
        assert_eq!(s["avg_fee"], "1");
        assert_eq!(s["non_native_fee_txs"], 1);
    }

    #[test]
    fn summarizes_a_solana_block() {
        use super::summarize_solana_block;
        let vote = "Vote111111111111111111111111111111111111111";
        let block = json!({
            "blockTime": 1793940224, "blockHeight": 443998708,
            "transactions": [
                {"version": "legacy",
                 "meta": {"err": null, "fee": 5000, "computeUnitsConsumed": 2100},
                 "transaction": {"message": {"accountKeys": ["Payer1", vote],
                                             "instructions": [{"programIdIndex": 1}]}}},
                {"version": 0,
                 "meta": {"err": {"InstructionError": [0, {"Custom": 1}]}, "fee": 15000, "computeUnitsConsumed": 30000},
                 "transaction": {"message": {"accountKeys": ["Payer2", "Prog"],
                                             "instructions": [{"programIdIndex": 1}]}}},
                {"version": 0,
                 "meta": {"err": null, "fee": 80000, "computeUnitsConsumed": 1000},
                 "transaction": {"message": {"accountKeys": ["Payer2", "Prog"],
                                             "instructions": [{"programIdIndex": 1}]}}}
            ]
        });
        let out = summarize_solana_block("solana", 465979003, &block);
        let s = &out["stats"];
        assert_eq!(out["block_number"], 465979003_u64);
        assert_eq!(out["timestamp"], 1793940224_u64);
        assert_eq!(s["total_transactions"], 3);
        assert_eq!(s["unique_senders"], 2);
        assert_eq!(s["successful_txs"], 2);
        assert_eq!(s["failed_txs"], 1);
        assert_eq!(s["total_fees"], "0.0001");
        assert_eq!(s["fee_unit"], "SOL");
        assert_eq!(s["gas_used"], 33100);
        assert_eq!(s["vote_txs"], 1);
        assert_eq!(s["tx_types"]["LEGACY"], 1);
        assert_eq!(s["tx_types"]["V0"], 2);
    }

    #[test]
    fn summarizes_a_cometbft_block() {
        use super::summarize_cometbft_block;
        let block = json!({"block": {
            "header": {"height": "37334692", "time": "2026-11-06T04:42:33.68580941Z", "chain_id": "axelar-dojo-1"},
            "data": {"txs": ["tx1", "tx2", "tx3"]}}});
        let results = json!({"height": "37334692", "txs_results": [
            {"code": 0, "gas_used": "172517", "gas_wanted": "186095", "events": [
                {"type": "message", "attributes": [{"key": "sender", "value": "axelar1a"}]},
                {"type": "tx", "attributes": [{"key": "fee", "value": "1861uaxl"}, {"key": "fee_payer", "value": "axelar1a"}]},
                {"type": "message", "attributes": [{"key": "action", "value": "/ibc.core.client.v1.MsgUpdateClient"}]}]},
            {"code": 5, "gas_used": "50000", "gas_wanted": "60000", "events": [
                {"type": "tx", "attributes": [{"key": "fee", "value": "139uaxl"}, {"key": "fee_payer", "value": "axelar1b"}]},
                {"type": "message", "attributes": [{"key": "action", "value": "/cosmos.bank.v1beta1.MsgSend"}]}]},
            // Rejected before fee deduction: no events, no fee.
            {"code": 13, "gas_used": "0", "gas_wanted": "1000", "events": []}
        ]});
        let out = summarize_cometbft_block("axelar", "AXL", &block, &results);
        let s = &out["stats"];
        assert_eq!(out["block_number"], 37334692);
        assert_eq!(out["timestamp"], 1793940153);
        assert_eq!(s["total_transactions"], 3);
        assert_eq!(s["successful_txs"], 1);
        assert_eq!(s["failed_txs"], 2);
        assert_eq!(s["unique_senders"], 2);
        assert_eq!(s["total_fees"], "0.002");
        assert_eq!(s["gas_used"], 222517);
        assert_eq!(s["tx_types"]["MsgUpdateClient"], 1);
        assert_eq!(s["tx_types"]["MsgSend"], 1);
        assert_eq!(s["tx_types"]["UNKNOWN"], 1);
        assert_eq!(s["chain_id"], "axelar-dojo-1");
    }

    #[test]
    fn reads_base64_cometbft_0_34_events_and_flags_foreign_fees() {
        use super::summarize_cometbft_block;
        let block = json!({"block": {"header": {"height": "5", "time": "2026-11-06T04:42:29Z"},
                                     "data": {"txs": ["a", "b"]}}});
        // "fee" -> no value (zero fee), "acc_seq" -> "sgn1x/7", "action" -> "vote_updates".
        let results = json!({"txs_results": [
            {"code": 0, "gas_used": "41295", "gas_wanted": "550953", "events": [
                {"type": "tx", "attributes": [{"key": "ZmVl", "value": null}]},
                {"type": "tx", "attributes": [{"key": "YWNjX3NlcQ==", "value": "c2duMXgvNw=="}]},
                {"type": "message", "attributes": [{"key": "YWN0aW9u", "value": "dm90ZV91cGRhdGVz"}]}]},
            {"code": 0, "gas_used": "1", "gas_wanted": "1", "events": [
                {"type": "tx", "attributes": [{"key": "fee", "value": "10ibc/ABC"}, {"key": "fee_payer", "value": "sgn1y"}]}]}
        ]});
        let s = &summarize_cometbft_block("celer", "CELR", &block, &results)["stats"];
        assert_eq!(s["unique_senders"], 2);
        assert_eq!(s["tx_types"]["vote_updates"], 1);
        assert!(s["total_fees"].is_null() && s["avg_fee"].is_null());
        assert!(s["partial"].as_str().unwrap().starts_with("1 transaction fees"));
    }

    #[test]
    fn rejects_stale_head_blocks() {
        use super::check_fresh;
        let now = chrono::Utc::now().timestamp() as u64;
        assert!(check_fresh(Some(now - 10)).is_ok());
        assert!(check_fresh(Some(now - 90 * 86400)).unwrap_err().contains("not synced"));
        assert!(check_fresh(None).is_err());
    }
}
