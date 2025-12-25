use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Submission {
    pub id: String,
    pub bounty_id: String,
    pub user_id: String,
    pub content: String,
    pub status: String,
    pub feedback: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateSubmission {
    pub bounty_id: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ReviewSubmission {
    pub status: String,
    pub feedback: Option<String>,
}
