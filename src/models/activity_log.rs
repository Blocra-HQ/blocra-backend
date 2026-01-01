use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ActivityLog {
    pub id: i64,
    pub user_id: i64,
    pub activity_type: String,
    pub description: String,
    pub metadata: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct QueryLog {
    pub id: i64,
    pub user_id: i64,
    pub query_text: String,
    pub query_type: Option<String>,
    pub contract_address: Option<String>,
    pub execution_time_ms: Option<i64>,
    pub result_count: Option<i64>,
    pub success: bool,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReportLog {
    pub id: i64,
    pub user_id: i64,
    pub report_type: String,
    pub report_name: Option<String>,
    pub contract_addresses: Option<String>,
    pub parameters: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserActivity {
    pub user_id: i64,
    pub email: Option<String>,
    pub username: Option<String>,
    pub total_logins: i64,
    pub total_queries: i64,
    pub total_reports: i64,
    pub last_activity: Option<DateTime<Utc>>,
    pub recent_activities: Vec<ActivityLog>,
}

#[derive(Debug, Serialize)]
pub struct AdminStats {
    pub total_users: i64,
    pub active_users_today: i64,
    pub total_queries_today: i64,
    pub total_reports_today: i64,
    pub most_queried_contracts: Vec<ContractStats>,
}

#[derive(Debug, Serialize)]
pub struct ContractStats {
    pub contract_address: String,
    pub query_count: i64,
    pub unique_users: i64,
}
