use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::contract as contract_handler, db::DbPool, services::rpc::RpcService};

pub fn configure(cfg: &mut web::ServiceConfig) {
    let rpc_service = web::Data::new(RpcService::new());
    
    cfg.service(
        web::scope("/contracts")
            .app_data(rpc_service)
            .route("/query", web::post().to(query_contract))
            .route("/queries", web::get().to(list_queries))
            .route("/queries/{id}", web::get().to(get_query))
            .route("/events", web::post().to(get_events))
            .route("/analyze", web::post().to(analyze_contract))
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

async fn get_query(pool: web::Data<DbPool>, id: web::Path<String>) -> impl Responder {
    match contract_handler::get_query(&pool, &id).await {
        Ok(query) => HttpResponse::Ok().json(query),
        Err(e) => HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_events(
    rpc: web::Data<RpcService>,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match contract_handler::get_contract_events(&rpc, payload.into_inner()).await {
        Ok(events) => HttpResponse::Ok().json(events),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn analyze_contract(
    rpc: web::Data<RpcService>,
    payload: web::Json<serde_json::Value>,
) -> impl Responder {
    match contract_handler::analyze_contract(&rpc, payload.into_inner()).await {
        Ok(analysis) => HttpResponse::Ok().json(analysis),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
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
