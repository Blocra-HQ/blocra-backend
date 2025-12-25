use crate::{db::DbPool, models::*, errors::AppError};
use serde_json::{json, Value};

pub async fn list_users(pool: &DbPool) -> Result<Vec<User>, AppError> {
    Ok(sqlx::query_as("SELECT * FROM users").fetch_all(pool).await?)
}

pub async fn delete_user(pool: &DbPool, id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

pub async fn approve_bounty(pool: &DbPool, id: &str) -> Result<Bounty, AppError> {
    Ok(sqlx::query_as("UPDATE bounties SET status = 'approved' WHERE id = ? RETURNING *")
        .bind(id).fetch_one(pool).await?)
}

pub async fn review_submission(pool: &DbPool, id: &str, payload: Value) -> Result<Submission, AppError> {
    let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("reviewed");
    let feedback = payload.get("feedback").and_then(|v| v.as_str());
    
    Ok(sqlx::query_as(
        "UPDATE submissions SET status = ?, feedback = ?, reviewed_at = CURRENT_TIMESTAMP 
         WHERE id = ? RETURNING *"
    )
    .bind(status).bind(feedback).bind(id).fetch_one(pool).await?)
}

pub async fn get_stats(pool: &DbPool) -> Result<Value, AppError> {
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    let bounty_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bounties").fetch_one(pool).await?;
    let submission_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions").fetch_one(pool).await?;
    
    Ok(json!({
        "users": user_count,
        "bounties": bounty_count,
        "submissions": submission_count
    }))
}
