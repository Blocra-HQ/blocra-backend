use crate::{db::DbPool, models::activity_log::*, errors::AppError};
use actix_web::HttpRequest;
use serde::{Deserialize, Serialize};
use crate::utils::jwt;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    pub user_id: Option<i64>,
    pub activity_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardResponse {
    pub stats: AdminStats,
    pub recent_activities: Vec<ActivityLog>,
    pub top_users: Vec<UserActivity>,
}

// Get admin dashboard overview
pub async fn get_admin_dashboard(
    pool: &DbPool,
    req: &HttpRequest,
) -> Result<AdminDashboardResponse, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Check if user is admin
    let user: crate::models::user::User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if user.role != "admin" {
        return Err(AppError::Unauthorized("Admin access required".to_string()));
    }

    // Get stats
    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    let active_users_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_id) FROM activity_logs 
         WHERE DATE(created_at) = DATE('now')"
    )
    .fetch_one(pool)
    .await?;

    let total_queries_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM query_logs 
         WHERE DATE(created_at) = DATE('now')"
    )
    .fetch_one(pool)
    .await?;

    let total_reports_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM report_logs 
         WHERE DATE(created_at) = DATE('now')"
    )
    .fetch_one(pool)
    .await?;

    // Most queried contracts
    let most_queried: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT contract_address, COUNT(*) as query_count, COUNT(DISTINCT user_id) as unique_users
         FROM query_logs 
         WHERE contract_address IS NOT NULL
         GROUP BY contract_address
         ORDER BY query_count DESC
         LIMIT 10"
    )
    .fetch_all(pool)
    .await?;

    let most_queried_contracts = most_queried.into_iter()
        .map(|(addr, count, users)| ContractStats {
            contract_address: addr,
            query_count: count,
            unique_users: users,
        })
        .collect();

    let stats = AdminStats {
        total_users,
        active_users_today,
        total_queries_today,
        total_reports_today,
        most_queried_contracts,
    };

    // Recent activities
    let recent_activities: Vec<ActivityLog> = sqlx::query_as(
        "SELECT * FROM activity_logs 
         ORDER BY created_at DESC 
         LIMIT 50"
    )
    .fetch_all(pool)
    .await?;

    // Top users by activity
    let top_users_data: Vec<(i64, Option<String>, Option<String>, i64, i64, i64)> = sqlx::query_as(
        "SELECT 
            u.id,
            u.email,
            u.username,
            COUNT(DISTINCT CASE WHEN a.activity_type = 'login' THEN a.id END) as login_count,
            COUNT(DISTINCT q.id) as query_count,
            COUNT(DISTINCT r.id) as report_count
         FROM users u
         LEFT JOIN activity_logs a ON u.id = a.user_id
         LEFT JOIN query_logs q ON u.id = q.user_id
         LEFT JOIN report_logs r ON u.id = r.user_id
         GROUP BY u.id
         ORDER BY (login_count + query_count + report_count) DESC
         LIMIT 10"
    )
    .fetch_all(pool)
    .await?;

    let mut top_users = Vec::new();
    for (uid, email, username, logins, queries, reports) in top_users_data {
        let last_activity: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT created_at FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 1"
        )
        .bind(uid)
        .fetch_optional(pool)
        .await?;

        let recent: Vec<ActivityLog> = sqlx::query_as(
            "SELECT * FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 5"
        )
        .bind(uid)
        .fetch_all(pool)
        .await?;

        top_users.push(UserActivity {
            user_id: uid,
            email,
            username,
            total_logins: logins,
            total_queries: queries,
            total_reports: reports,
            last_activity,
            recent_activities: recent,
        });
    }

    Ok(AdminDashboardResponse {
        stats,
        recent_activities,
        top_users,
    })
}

// Get all users with their activity
pub async fn get_all_users(
    pool: &DbPool,
    req: &HttpRequest,
) -> Result<Vec<UserActivity>, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Check if user is admin
    let user: crate::models::user::User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if user.role != "admin" {
        return Err(AppError::Unauthorized("Admin access required".to_string()));
    }

    let users_data: Vec<(i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, email, username FROM users ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::new();
    for (uid, email, username) in users_data {
        let logins: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM activity_logs WHERE user_id = ? AND activity_type = 'login'"
        )
        .bind(uid)
        .fetch_one(pool)
        .await?;

        let queries: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM query_logs WHERE user_id = ?"
        )
        .bind(uid)
        .fetch_one(pool)
        .await?;

        let reports: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM report_logs WHERE user_id = ?"
        )
        .bind(uid)
        .fetch_one(pool)
        .await?;

        let last_activity: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT created_at FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 1"
        )
        .bind(uid)
        .fetch_optional(pool)
        .await?;

        let recent: Vec<ActivityLog> = sqlx::query_as(
            "SELECT * FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 10"
        )
        .bind(uid)
        .fetch_all(pool)
        .await?;

        result.push(UserActivity {
            user_id: uid,
            email,
            username,
            total_logins: logins,
            total_queries: queries,
            total_reports: reports,
            last_activity,
            recent_activities: recent,
        });
    }

    Ok(result)
}

// Get user activity details
pub async fn get_user_activity(
    pool: &DbPool,
    req: &HttpRequest,
    target_user_id: i64,
) -> Result<UserActivity, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Check if user is admin
    let user: crate::models::user::User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if user.role != "admin" {
        return Err(AppError::Unauthorized("Admin access required".to_string()));
    }

    let target_user: crate::models::user::User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(target_user_id)
    .fetch_one(pool)
    .await?;

    let logins: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_logs WHERE user_id = ? AND activity_type = 'login'"
    )
    .bind(target_user_id)
    .fetch_one(pool)
    .await?;

    let queries: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM query_logs WHERE user_id = ?"
    )
    .bind(target_user_id)
    .fetch_one(pool)
    .await?;

    let reports: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM report_logs WHERE user_id = ?"
    )
    .bind(target_user_id)
    .fetch_one(pool)
    .await?;

    let last_activity: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT created_at FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 1"
    )
    .bind(target_user_id)
    .fetch_optional(pool)
    .await?;

    let recent: Vec<ActivityLog> = sqlx::query_as(
        "SELECT * FROM activity_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 50"
    )
    .bind(target_user_id)
    .fetch_all(pool)
    .await?;

    Ok(UserActivity {
        user_id: target_user_id,
        email: target_user.email,
        username: target_user.username,
        total_logins: logins,
        total_queries: queries,
        total_reports: reports,
        last_activity,
        recent_activities: recent,
    })
}

// Get query logs
pub async fn get_query_logs(
    pool: &DbPool,
    req: &HttpRequest,
    user_id_filter: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<QueryLog>, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Check if user is admin
    let user: crate::models::user::User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if user.role != "admin" {
        return Err(AppError::Unauthorized("Admin access required".to_string()));
    }

    let limit = limit.unwrap_or(100).min(1000);

    let logs: Vec<QueryLog> = if let Some(uid) = user_id_filter {
        sqlx::query_as(
            "SELECT * FROM query_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT ?"
        )
        .bind(uid)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT * FROM query_logs ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(logs)
}

// Log query execution (called by query handler)
#[allow(dead_code)]
pub async fn log_query_execution(
    pool: &DbPool,
    user_id: i64,
    query_text: String,
    query_type: Option<String>,
    contract_address: Option<String>,
    execution_time_ms: Option<i64>,
    result_count: Option<i64>,
    success: bool,
    error_message: Option<String>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO query_logs 
         (user_id, query_text, query_type, contract_address, execution_time_ms, result_count, success, error_message)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(user_id)
    .bind(query_text)
    .bind(query_type)
    .bind(contract_address)
    .bind(execution_time_ms)
    .bind(result_count)
    .bind(success)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

// Log report generation
#[allow(dead_code)]
pub async fn log_report_generation(
    pool: &DbPool,
    user_id: i64,
    report_type: String,
    report_name: Option<String>,
    contract_addresses: Option<Vec<String>>,
    parameters: Option<serde_json::Value>,
) -> Result<(), AppError> {
    let addresses_json = contract_addresses.map(|addrs| serde_json::to_string(&addrs).ok()).flatten();
    let params_json = parameters.map(|p| serde_json::to_string(&p).ok()).flatten();

    sqlx::query(
        "INSERT INTO report_logs 
         (user_id, report_type, report_name, contract_addresses, parameters)
         VALUES (?, ?, ?, ?, ?)"
    )
    .bind(user_id)
    .bind(report_type)
    .bind(report_name)
    .bind(addresses_json)
    .bind(params_json)
    .execute(pool)
    .await?;

    Ok(())
}
