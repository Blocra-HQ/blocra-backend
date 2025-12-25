use actix_web::{web, HttpRequest, HttpResponse};
use sqlx::SqlitePool;
use crate::errors::AppError;
use crate::models::subscription::*;
use crate::utils::jwt;

// Get all available subscription plans
pub async fn get_plans(pool: web::Data<SqlitePool>) -> Result<HttpResponse, AppError> {
    let plans = sqlx::query_as::<_, SubscriptionPlan>(
        "SELECT * FROM subscription_plans WHERE is_active = 1 ORDER BY price_usdt ASC"
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "plans": plans
    })))
}

// Get supported payment chains
pub async fn get_payment_chains(pool: web::Data<SqlitePool>) -> Result<HttpResponse, AppError> {
    let chains = sqlx::query_as::<_, PaymentChain>(
        "SELECT * FROM payment_chains WHERE is_active = 1 ORDER BY chain_name ASC"
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "chains": chains
    })))
}

// Check if user is admin
async fn is_admin(pool: &SqlitePool, user_id: i64) -> Result<bool, AppError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(role == "admin")
}

// Get user's subscription status
pub async fn get_subscription_status(
    pool: web::Data<SqlitePool>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&req)?;
    let admin = is_admin(pool.get_ref(), user_id).await.unwrap_or(false);

    // Get active subscription
    let subscription = sqlx::query_as::<_, UserSubscription>(
        "SELECT * FROM user_subscriptions 
         WHERE user_id = ? AND status = 'active' 
         AND end_date > datetime('now')
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    let (has_active, reports_remaining, can_generate) = if admin {
        (true, 999999, true) // Admins have unlimited reports
    } else if let Some(ref sub) = subscription {
        let remaining = sub.reports_limit - sub.reports_used;
        (true, remaining, remaining > 0)
    } else {
        (false, 0, false)
    };

    Ok(HttpResponse::Ok().json(SubscriptionStatus {
        has_active_subscription: has_active,
        subscription,
        reports_remaining,
        can_generate_report: can_generate,
        is_admin: admin,
    }))
}

// Create one-time payment for single report (3 USDT)
pub async fn create_one_time_payment(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
    req: web::Json<CreatePaymentRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;

    // Get plan details
    let plan = sqlx::query_as::<_, SubscriptionPlan>(
        "SELECT * FROM subscription_plans WHERE id = ? AND name = 'Pay Per Report'"
    )
    .bind(req.plan_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| AppError::NotFound("Plan not found".to_string()))?;

    // Verify chain is supported
    let chain = sqlx::query_as::<_, PaymentChain>(
        "SELECT * FROM payment_chains WHERE chain_id = ? AND is_active = 1"
    )
    .bind(&req.chain_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| AppError::BadRequest("Unsupported payment chain".to_string()))?;

    // In production, verify the transaction on-chain
    // For now, we'll accept the tx_hash and mark as completed

    // Create payment transaction
    let transaction_id = sqlx::query(
        "INSERT INTO payment_transactions 
         (user_id, wallet_address, amount_usdt, payment_type, status, payment_chain_id, tx_hash)
         VALUES (?, ?, ?, 'one_time', 'completed', ?, ?)"
    )
    .bind(user_id)
    .bind(&req.wallet_address)
    .bind(plan.price_usdt)
    .bind(&chain.chain_id)
    .bind(&req.tx_hash)
    .execute(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?
    .last_insert_rowid();

    Ok(HttpResponse::Ok().json(PaymentResponse {
        success: true,
        transaction_id: Some(transaction_id),
        message: format!("Payment of {} USDT received. You can now generate 1 report.", plan.price_usdt),
    }))
}

// Create monthly subscription (50 USDT)
pub async fn create_subscription(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
    req: web::Json<CreateSubscriptionRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;

    // Check if user already has active subscription
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_subscriptions 
         WHERE user_id = ? AND status = 'active' AND end_date > datetime('now')"
    )
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    if existing > 0 {
        return Err(AppError::BadRequest("You already have an active subscription".to_string()));
    }

    // Get plan details
    let plan = sqlx::query_as::<_, SubscriptionPlan>(
        "SELECT * FROM subscription_plans WHERE id = ? AND name = 'Monthly Pro'"
    )
    .bind(req.plan_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| AppError::NotFound("Plan not found".to_string()))?;

    // Verify chain is supported
    let chain = sqlx::query_as::<_, PaymentChain>(
        "SELECT * FROM payment_chains WHERE chain_id = ? AND is_active = 1"
    )
    .bind(&req.chain_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| AppError::BadRequest("Unsupported payment chain".to_string()))?;

    // Create subscription
    let start_date = chrono::Utc::now();
    let end_date = start_date + chrono::Duration::days(plan.duration_days);

    let subscription_id = sqlx::query(
        "INSERT INTO user_subscriptions 
         (user_id, wallet_address, plan_id, status, reports_used, reports_limit, start_date, end_date, payment_chain_id, tx_hash)
         VALUES (?, ?, ?, 'active', 0, ?, ?, ?, ?, ?)"
    )
    .bind(user_id)
    .bind(&req.wallet_address)
    .bind(plan.id)
    .bind(plan.report_limit)
    .bind(start_date.to_rfc3339())
    .bind(end_date.to_rfc3339())
    .bind(&chain.chain_id)
    .bind(&req.tx_hash)
    .execute(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?
    .last_insert_rowid();

    // Create payment transaction
    sqlx::query(
        "INSERT INTO payment_transactions 
         (user_id, wallet_address, amount_usdt, payment_type, status, payment_chain_id, tx_hash, subscription_id)
         VALUES (?, ?, ?, 'subscription', 'completed', ?, ?, ?)"
    )
    .bind(user_id)
    .bind(&req.wallet_address)
    .bind(plan.price_usdt)
    .bind(&chain.chain_id)
    .bind(&req.tx_hash)
    .bind(subscription_id)
    .execute(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(PaymentResponse {
        success: true,
        transaction_id: Some(subscription_id),
        message: format!("Subscription activated! You have {} reports available for {} days.", plan.report_limit, plan.duration_days),
    }))
}

// Cancel subscription
pub async fn cancel_subscription(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
    req: web::Json<CancelSubscriptionRequest>,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;

    // Verify subscription belongs to user
    let subscription = sqlx::query_as::<_, UserSubscription>(
        "SELECT * FROM user_subscriptions WHERE id = ? AND user_id = ?"
    )
    .bind(req.subscription_id)
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|_| AppError::NotFound("Subscription not found".to_string()))?;

    if subscription.status != "active" {
        return Err(AppError::BadRequest("Subscription is not active".to_string()));
    }

    // Cancel subscription
    sqlx::query(
        "UPDATE user_subscriptions SET status = 'cancelled', updated_at = datetime('now') WHERE id = ?"
    )
    .bind(req.subscription_id)
    .execute(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "Subscription cancelled successfully"
    })))
}

// Check if user can generate report and track usage
pub async fn check_and_track_report_usage(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
    contract_address: web::Json<serde_json::Value>,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;
    let address = contract_address
        .get("contract_address")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("contract_address required".to_string()))?;

    // Check if user is admin - admins get free reports
    let admin = is_admin(pool.get_ref(), user_id).await.unwrap_or(false);
    
    if admin {
        // Track admin usage
        sqlx::query(
            "INSERT INTO report_usage (user_id, contract_address, report_type, payment_type, is_admin_generated)
             VALUES (?, ?, 'eda', 'admin_free', 1)"
        )
        .bind(user_id)
        .bind(address)
        .execute(pool.get_ref())
        .await
        .map_err(|e| AppError::DatabaseError(e))?;

        return Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "can_generate": true,
            "payment_type": "admin_free",
            "is_admin": true,
            "message": "Admin report generation authorized (free)."
        })));
    }

    // Check for active subscription
    let subscription = sqlx::query_as::<_, UserSubscription>(
        "SELECT * FROM user_subscriptions 
         WHERE user_id = ? AND status = 'active' 
         AND end_date > datetime('now')
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    if let Some(sub) = subscription {
        // Check if reports remaining
        if sub.reports_used >= sub.reports_limit {
            return Err(AppError::BadRequest("Report limit reached. Please upgrade or wait for renewal.".to_string()));
        }

        // Track usage
        sqlx::query(
            "INSERT INTO report_usage (user_id, contract_address, report_type, payment_type, subscription_id, is_admin_generated)
             VALUES (?, ?, 'eda', 'subscription', ?, 0)"
        )
        .bind(user_id)
        .bind(address)
        .bind(sub.id)
        .execute(pool.get_ref())
        .await
        .map_err(|e| AppError::DatabaseError(e))?;

        // Increment usage counter
        sqlx::query(
            "UPDATE user_subscriptions SET reports_used = reports_used + 1, updated_at = datetime('now') WHERE id = ?"
        )
        .bind(sub.id)
        .execute(pool.get_ref())
        .await
        .map_err(|e| AppError::DatabaseError(e))?;

        let remaining = sub.reports_limit - sub.reports_used - 1;
        return Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "can_generate": true,
            "payment_type": "subscription",
            "reports_remaining": remaining,
            "is_admin": false,
            "message": format!("Report generation authorized. {} reports remaining.", remaining)
        })));
    }

    // Check for one-time payment
    let one_time_payment = sqlx::query_as::<_, PaymentTransaction>(
        "SELECT * FROM payment_transactions 
         WHERE user_id = ? AND payment_type = 'one_time' AND status = 'completed'
         AND id NOT IN (SELECT transaction_id FROM report_usage WHERE transaction_id IS NOT NULL)
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    if let Some(payment) = one_time_payment {
        // Track usage
        sqlx::query(
            "INSERT INTO report_usage (user_id, contract_address, report_type, payment_type, transaction_id, is_admin_generated)
             VALUES (?, ?, 'eda', 'one_time', ?, 0)"
        )
        .bind(user_id)
        .bind(address)
        .bind(payment.id)
        .execute(pool.get_ref())
        .await
        .map_err(|e| AppError::DatabaseError(e))?;

        return Ok(HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "can_generate": true,
            "payment_type": "one_time",
            "is_admin": false,
            "message": "Report generation authorized (one-time payment)."
        })));
    }

    // No valid payment found
    Err(AppError::Unauthorized("No valid payment or subscription found. Please purchase a report (3 USDT) or subscribe (50 USDT/month).".to_string()))
}

// Get user's payment history
pub async fn get_payment_history(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;

    let transactions = sqlx::query_as::<_, PaymentTransaction>(
        "SELECT * FROM payment_transactions WHERE user_id = ? ORDER BY created_at DESC LIMIT 50"
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "transactions": transactions
    })))
}

// Get user's report usage history
pub async fn get_report_history(
    pool: web::Data<SqlitePool>,
    http_req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let user_id = jwt::extract_user_id(&http_req)?;

    let reports = sqlx::query_as::<_, ReportUsage>(
        "SELECT * FROM report_usage WHERE user_id = ? ORDER BY created_at DESC LIMIT 100"
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::DatabaseError(e))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "reports": reports
    })))
}
