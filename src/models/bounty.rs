use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Bounty {
    pub id: String,
    pub title: String,
    pub description: String,
    pub reward_amount: f64,
    pub reward_token: String,
    pub status: String,
    pub difficulty: String,
    pub category: String,
    pub created_by: String,
    pub deadline: Option<DateTime<Utc>>,
    pub max_participants: Option<i32>,
    pub requirements: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBounty {
    pub title: String,
    pub description: String,
    pub reward_amount: f64,
    pub reward_token: String,
    pub difficulty: String,
    pub category: String,
    pub deadline: Option<DateTime<Utc>>,
    pub max_participants: Option<i32>,
    pub requirements: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UpdateBounty {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reward_amount: Option<f64>,
    pub status: Option<String>,
    pub difficulty: Option<String>,
    pub category: Option<String>,
    pub deadline: Option<DateTime<Utc>>,
}
