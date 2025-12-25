use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct Reward {
    pub id: String,
    pub bounty_id: String,
    pub user_id: String,
    pub amount: f64,
    pub token: String,
    pub transaction_hash: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateReward {
    pub bounty_id: String,
    pub user_id: String,
    pub amount: f64,
    pub token: String,
    pub transaction_hash: Option<String>,
}
