use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::admin as admin_handler, db::DbPool};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin")
            .route("/users", web::get().to(list_users))
            .route("/users/{id}", web::delete().to(delete_user))
            .route("/bounties/{id}/approve", web::post().to(approve_bounty))
            .route("/submissions/{id}/review", web::post().to(review_submission))
            .route("/stats", web::get().to(get_stats))
    );
}

async fn list_users(pool: web::Data<DbPool>) -> impl Responder {
    match admin_handler::list_users(&pool).await {
        Ok(users) => HttpResponse::Ok().json(users),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn delete_user(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match admin_handler::delete_user(&pool, &id).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "User deleted"
        })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn approve_bounty(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match admin_handler::approve_bounty(&pool, &id).await {
        Ok(bounty) => HttpResponse::Ok().json(bounty),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn review_submission(
    pool: web::Data<DbPool>,
    id: web::Path<String>,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match admin_handler::review_submission(&pool, &id, payload.into_inner()).await {
        Ok(submission) => HttpResponse::Ok().json(submission),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_stats(pool: web::Data<DbPool>) -> impl Responder {
    match admin_handler::get_stats(&pool).await {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
