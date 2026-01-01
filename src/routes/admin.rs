use actix_web::{web, HttpResponse, Responder, HttpRequest};
use crate::{handlers::admin as admin_handler, db::DbPool};
use serde::Deserialize;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin")
            .route("/login", web::post().to(admin_login))
            .route("/dashboard", web::get().to(get_dashboard))
            .route("/users", web::get().to(get_all_users))
            .route("/users/{user_id}", web::get().to(get_user_activity))
            .route("/query-logs", web::get().to(get_query_logs))
    );
}

#[derive(Deserialize)]
struct AdminLoginPayload {
    password: String,
}

async fn admin_login(
    pool: web::Data<DbPool>,
    payload: web::Json<AdminLoginPayload>,
) -> impl Responder {
    // Simple password check - in production, use proper auth
    if payload.password != "Ndifreke000" {
        return HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": "Invalid password"
        }));
    }

    // Get or create admin user
    let admin_user = sqlx::query_as::<_, crate::models::user::User>(
        "INSERT INTO users (email, username, role) VALUES ('admin@blocra.com', 'admin', 'admin')
         ON CONFLICT(email) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .fetch_one(pool.as_ref())
    .await;

    match admin_user {
        Ok(user) => {
            let token = crate::utils::jwt::create_token(user.id);
            match token {
                Ok(token) => {
                    // Log admin login
                    let _ = sqlx::query(
                        "INSERT INTO activity_logs (user_id, activity_type, description) 
                         VALUES (?, 'login', 'Admin login')"
                    )
                    .bind(user.id)
                    .execute(pool.as_ref())
                    .await;

                    HttpResponse::Ok().json(serde_json::json!({
                        "success": true,
                        "token": token,
                        "user": {
                            "id": user.id,
                            "email": user.email,
                            "role": user.role
                        }
                    }))
                }
                Err(_) => HttpResponse::InternalServerError().json(serde_json::json!({
                    "success": false,
                    "message": "Failed to create token"
                }))
            }
        }
        Err(_) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": "Failed to create admin user"
        }))
    }
}

async fn get_dashboard(
    pool: web::Data<DbPool>,
    req: HttpRequest,
) -> impl Responder {
    match admin_handler::get_admin_dashboard(&pool, &req).await {
        Ok(dashboard) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": dashboard
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_all_users(
    pool: web::Data<DbPool>,
    req: HttpRequest,
) -> impl Responder {
    match admin_handler::get_all_users(&pool, &req).await {
        Ok(users) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": users
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_user_activity(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    let user_id = path.into_inner();
    match admin_handler::get_user_activity(&pool, &req, user_id).await {
        Ok(activity) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": activity
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

#[derive(Deserialize)]
struct QueryLogsQuery {
    user_id: Option<i64>,
    limit: Option<i64>,
}

async fn get_query_logs(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    query: web::Query<QueryLogsQuery>,
) -> impl Responder {
    match admin_handler::get_query_logs(&pool, &req, query.user_id, query.limit).await {
        Ok(logs) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": logs
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
