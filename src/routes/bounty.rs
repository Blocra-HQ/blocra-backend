use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::bounty as bounty_handler, db::DbPool, models::bounty::*};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/bounties")
            .route("", web::get().to(list_bounties))
            .route("", web::post().to(create_bounty))
            .route("/{id}", web::get().to(get_bounty))
            .route("/{id}", web::put().to(update_bounty))
            .route("/{id}", web::delete().to(delete_bounty))
            .route("/{id}/join", web::post().to(join_bounty))
            .route("/{id}/participants", web::get().to(get_participants))
            .route("/{id}/submissions", web::get().to(get_submissions))
            .route("/{id}/submit", web::post().to(submit_bounty))
    );
}

async fn list_bounties(pool: web::Data<DbPool>) -> impl Responder {
    match bounty_handler::list_bounties(&pool).await {
        Ok(bounties) => HttpResponse::Ok().json(bounties),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn create_bounty(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    payload: web::Json<CreateBounty>,
) -> impl Responder {
    match bounty_handler::create_bounty(&pool, &req, payload.into_inner()).await {
        Ok(bounty) => HttpResponse::Created().json(bounty),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_bounty(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match bounty_handler::get_bounty(&pool, &id).await {
        Ok(bounty) => HttpResponse::Ok().json(bounty),
        Err(e) => HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn update_bounty(
    pool: web::Data<DbPool>,
    id: web::Path<String>,
    payload: web::Json<UpdateBounty>,
) -> impl Responder {
    match bounty_handler::update_bounty(&pool, &id, payload.into_inner()).await {
        Ok(bounty) => HttpResponse::Ok().json(bounty),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn delete_bounty(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match bounty_handler::delete_bounty(&pool, &id).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Bounty deleted"
        })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn join_bounty(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    id: web::Path<String>,
) -> impl Responder {
    match bounty_handler::join_bounty(&pool, &req, &id).await {
        Ok(participant) => HttpResponse::Ok().json(participant),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_participants(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match bounty_handler::get_participants(&pool, &id).await {
        Ok(participants) => HttpResponse::Ok().json(participants),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_submissions(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match bounty_handler::get_submissions(&pool, &id).await {
        Ok(submissions) => HttpResponse::Ok().json(submissions),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn submit_bounty(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    id: web::Path<String>,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match bounty_handler::submit_bounty(&pool, &req, &id, payload.into_inner()).await {
        Ok(submission) => HttpResponse::Created().json(submission),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
