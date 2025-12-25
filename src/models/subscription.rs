use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct SubscriptionPlan {
    pub id: i64,
    pub name: String,
    pub price_usdt: f64,
    pub report_limit: i64,
    pub duration_days: i64,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PaymentChain {
    pub id: i64,
    pub chain_id: String,
    pub chain_name: String,
    pub usdt_contract_address: String,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct UserSubscription {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub plan_id: i64,
    pub status: String,
    pub reports_used: i64,
    pub reports_limit: i64,
    pub start_date: String,
    pub end_date: String,
    pub payment_chain_id: Option<String>,
    pub tx_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PaymentTransaction {
    pub id: i64,
    pub user_id: i64,
    pub wallet_address: String,
    pub amount_usdt: f64,
    pub payment_type: String,
    pub status: String,
    pub payment_chain_id: String,
    pub tx_hash: Option<String>,
    pub subscription_id: Option<i64>,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ReportUsage {
    pub id: i64,
    pub user_id: i64,
    pub contract_address: String,
    pub report_type: String,
    pub payment_type: String,
    pub subscription_id: Option<i64>,
    pub transaction_id: Option<i64>,
    pub is_admin_generated: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    pub plan_id: i64,
    pub wallet_address: String,
    pub chain_id: String,
    pub tx_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub plan_id: i64,
    pub wallet_address: String,
    pub chain_id: String,
    pub tx_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct CancelSubscriptionRequest {
    pub subscription_id: i64,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionStatus {
    pub has_active_subscription: bool,
    pub subscription: Option<UserSubscription>,
    pub reports_remaining: i64,
    pub can_generate_report: bool,
    pub is_admin: bool,
}

#[derive(Debug, Serialize)]
pub struct PaymentResponse {
    pub success: bool,
    pub transaction_id: Option<i64>,
    pub message: String,
}
