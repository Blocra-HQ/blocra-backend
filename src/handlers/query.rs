use crate::{db::DbPool, models::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use uuid::Uuid;
use serde_json::Value;

pub async fn list_saved_queries(pool: &DbPool, req: &HttpRequest) -> Result<Vec<ContractQuery>, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    Ok(sqlx::query_as("SELECT * FROM contract_queries WHERE user_id = ?")
        .bind(&user_id).fetch_all(pool).await?)
}

pub async fn save_query(pool: &DbPool, req: &HttpRequest, payload: Value) -> Result<ContractQuery, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    let query_id = Uuid::new_v4().to_string();
    
    let contract_address = payload.get("contract_address").and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("contract_address required".to_string()))?;
    let query_type = payload.get("query_type").and_then(|v| v.as_str())
        .ok_or(AppError::BadRequest("query_type required".to_string()))?;
    
    Ok(sqlx::query_as(
        "INSERT INTO contract_queries (id, user_id, contract_address, query_type)
         VALUES (?, ?, ?, ?) RETURNING *"
    )
    .bind(&query_id).bind(&user_id).bind(contract_address).bind(query_type)
    .fetch_one(pool).await?)
}

pub async fn get_saved_query(pool: &DbPool, id: &str) -> Result<ContractQuery, AppError> {
    Ok(sqlx::query_as("SELECT * FROM contract_queries WHERE id = ?")
        .bind(id).fetch_one(pool).await?)
}

pub async fn delete_query(pool: &DbPool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM contract_queries WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}
