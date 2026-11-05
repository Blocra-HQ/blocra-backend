use crate::{db::DbPool, utils::jwt, errors::AppError, services::rpc::RpcService};
use crate::services::stellar::{HorizonOperation, HorizonTransaction, LedgerInfo, StellarService};
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
}
