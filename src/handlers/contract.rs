use crate::{db::DbPool, models::*, utils::jwt, errors::AppError, services::rpc::RpcService};
use actix_web::HttpRequest;
use uuid::Uuid;
use serde_json::{json, Value};

pub async fn query_contract(pool: &DbPool, req: &HttpRequest, payload: Value) -> Result<ContractQuery, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let query_id = Uuid::new_v4().to_string();
    
    let contract_address = payload.get("contract_address").and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("contract_address required".to_string()))?;
    let query_type = payload.get("query_type").and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("query_type required".to_string()))?;
    let parameters = payload.get("parameters").map(|v| v.to_string());
    
    Ok(sqlx::query_as(
        "INSERT INTO contract_queries (id, user_id, contract_address, query_type, parameters)
         VALUES (?, ?, ?, ?, ?) RETURNING *"
    )
    .bind(&query_id).bind(&user_id).bind(contract_address)
    .bind(query_type).bind(parameters)
    .fetch_one(pool).await?)
}

pub async fn list_queries(pool: &DbPool, req: &HttpRequest) -> Result<Vec<ContractQuery>, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    Ok(sqlx::query_as("SELECT * FROM contract_queries WHERE user_id = ? ORDER BY created_at DESC")
        .bind(&user_id).fetch_all(pool).await?)
}

pub async fn get_query(pool: &DbPool, id: &str) -> Result<ContractQuery, AppError> {
    Ok(sqlx::query_as("SELECT * FROM contract_queries WHERE id = ?")
        .bind(id).fetch_one(pool).await?)
}

// RPC-based contract event fetching
pub async fn get_contract_events(rpc: &RpcService, payload: Value) -> Result<Value, AppError> {
    let contract_address = payload.get("contractAddress")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("contractAddress required".to_string()))?;

    let from_date = payload.get("fromDate")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64);

    let to_date = payload.get("toDate")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64);

    let latest = rpc.get_block_number().await?;
    
    let from_block = if let Some(ts) = from_date {
        rpc.find_block_by_timestamp(ts).await?
    } else {
        // UNLIMITED MODE: Start from block 0 (genesis) for complete historical analysis
        0
    };

    let to_block = if let Some(ts) = to_date {
        rpc.find_block_by_timestamp(ts).await?
    } else {
        latest
    };

    println!("🚀 UNLIMITED MODE: Fetching events from block {} to {} ({} blocks)", from_block, to_block, to_block - from_block);

    let events = rpc.get_events(contract_address, from_block, to_block).await?;
    
    println!("✅ Fetched {} events", events.len());

    Ok(json!({
        "success": true,
        "data": {
            "events": events,
            "fromBlock": from_block,
            "toBlock": to_block,
            "totalEvents": events.len()
        }
    }))
}

// RPC-based contract analysis
pub async fn analyze_contract(rpc: &RpcService, payload: Value) -> Result<Value, AppError> {
    let contract_address = payload.get("contractAddress")
        .and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("contractAddress required".to_string()))?;

    // Validate contract address format
    if !contract_address.starts_with("0x") || contract_address.len() != 66 {
        return Err(AppError::BadRequest("Invalid contract address format".to_string()));
    }

    let from_date = payload.get("fromDate")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64);

    let to_date = payload.get("toDate")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64);

    let analysis = rpc.analyze_contract(contract_address, from_date, to_date).await?;

    Ok(json!({
        "success": true,
        "data": analysis
    }))
}

// Save contract query
pub async fn save_contract_query(pool: &DbPool, req: &HttpRequest, payload: Value) -> Result<Value, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let query_id = Uuid::new_v4().to_string();

    let contracts = payload.get("contracts")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "[]".to_string());
    
    let chain = payload.get("chain")
        .and_then(|v| v.as_str())
        .unwrap_or("starknet");

    let from_date = payload.get("fromDate")
        .and_then(|v| v.as_str());
    
    let _to_date = payload.get("toDate")
        .and_then(|v| v.as_str());

    let events = payload.get("events")
        .map(|v| v.to_string());

    let _stats = payload.get("stats")
        .map(|v| v.to_string());

    sqlx::query(
        "INSERT INTO contract_queries (id, user_id, contract_address, query_type, parameters, result)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&query_id)
    .bind(&user_id)
    .bind(contracts)
    .bind(chain)
    .bind(from_date)
    .bind(events)
    .execute(pool)
    .await?;

    Ok(json!({
        "success": true,
        "data": {
            "queryId": query_id
        }
    }))
}

// Get saved queries
pub async fn get_saved_queries(pool: &DbPool, req: &HttpRequest) -> Result<Value, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    let queries: Vec<ContractQuery> = sqlx::query_as(
        "SELECT * FROM contract_queries WHERE user_id = ? ORDER BY created_at DESC LIMIT 50"
    )
    .bind(&user_id)
    .fetch_all(pool)
    .await?;

    Ok(json!({
        "success": true,
        "data": {
            "queries": queries
        }
    }))
}
