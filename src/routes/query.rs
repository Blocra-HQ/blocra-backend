use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::query as query_handler, db::DbPool};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/queries")
            .route("", web::get().to(list_saved_queries))
            .route("", web::post().to(save_query))
            .route("/{id}", web::get().to(get_saved_query))
            .route("/{id}", web::delete().to(delete_query))
    );
}

async fn list_saved_queries(pool: web::Data<DbPool>, req: actix_web::HttpRequest) -> impl Responder {
    match query_handler::list_saved_queries(&pool, &req).await {
        Ok(queries) => HttpResponse::Ok().json(queries),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn save_query(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match query_handler::save_query(&pool, &req, payload.into_inner()).await {
        Ok(query) => HttpResponse::Created().json(query),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_saved_query(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match query_handler::get_saved_query(&pool, &id).await {
        Ok(query) => HttpResponse::Ok().json(query),
        Err(e) => HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn delete_query(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match query_handler::delete_query(&pool, &id).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Query deleted"
        })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
