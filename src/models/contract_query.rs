use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContractQuery {
    pub id: String,
    pub user_id: String,
    pub contract_address: String,
    pub query_type: String,
    pub parameters: Option<String>,
    pub result: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateContractQuery {
    pub contract_address: String,
    pub query_type: String,
    pub parameters: Option<serde_json::Value>,
}
