use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::contract as contract_handler, db::DbPool};

pub fn configure(cfg: &mut web::ServiceConfig) {
    
    cfg.service(
        web::scope("/contracts")
            .route("/query", web::post().to(query_contract))
            .route("/queries", web::get().to(list_queries))
            .route("/queries/{id}", web::get().to(get_query))
            .route("/save-query", web::post().to(save_query))
            .route("/saved-queries", web::get().to(get_saved_queries))
    );
}

async fn query_contract(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match contract_handler::query_contract(&pool, &req, payload.into_inner()).await {
        Ok(query) => HttpResponse::Ok().json(query),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn list_queries(pool: web::Data<DbPool>, req: actix_web::HttpRequest) -> impl Responder {
    match contract_handler::list_queries(&pool, &req).await {
        Ok(queries) => HttpResponse::Ok().json(queries),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_query(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    id: web::Path<String>,
) -> impl Responder {
    match contract_handler::get_query(&pool, &req, &id).await {
        Ok(query) => HttpResponse::Ok().json(query),
        Err(e) => actix_web::ResponseError::error_response(&e),
    }
}

async fn save_query(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match contract_handler::save_contract_query(&pool, &req, payload.into_inner()).await {
        Ok(query) => HttpResponse::Ok().json(query),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_saved_queries(pool: web::Data<DbPool>, req: actix_web::HttpRequest) -> impl Responder {
    match contract_handler::get_saved_queries(&pool, &req).await {
        Ok(queries) => HttpResponse::Ok().json(queries),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
