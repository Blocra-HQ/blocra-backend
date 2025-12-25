use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BountyParticipant {
    pub id: String,
    pub bounty_id: String,
    pub user_id: String,
    pub status: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JoinBounty {
    pub bounty_id: String,
}
