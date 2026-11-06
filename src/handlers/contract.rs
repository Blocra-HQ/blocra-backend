use crate::{db::DbPool, models::*, utils::jwt, errors::AppError};
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
    .bind(&query_id).bind(user_id.to_string()).bind(contract_address)
    .bind(query_type).bind(parameters)
    .fetch_one(pool).await?)
}

pub async fn list_queries(pool: &DbPool, req: &HttpRequest) -> Result<Vec<ContractQuery>, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    Ok(sqlx::query_as("SELECT * FROM contract_queries WHERE user_id = ? ORDER BY created_at DESC")
        .bind(user_id.to_string()).fetch_all(pool).await?)
}

/// One of the caller's contract queries; others' answer "not found".
pub async fn get_query(pool: &DbPool, req: &HttpRequest, id: &str) -> Result<ContractQuery, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    sqlx::query_as("SELECT * FROM contract_queries WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Query not found".to_string()))
}

// RPC-based contract event fetching
// RPC-based contract analysis
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
    .bind(user_id.to_string())
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
    .bind(user_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(json!({
        "success": true,
        "data": {
            "queries": queries
        }
    }))
}
