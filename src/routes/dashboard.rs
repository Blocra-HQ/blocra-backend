use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::dashboard as dashboard_handler, db::DbPool, services::rpc::RpcService};

pub fn configure(cfg: &mut web::ServiceConfig) {
    let rpc_service = web::Data::new(RpcService::new());
    
    cfg.service(
        web::scope("/dashboards")
            .app_data(rpc_service)
            .route("", web::get().to(get_dashboard_data))
            .route("/analytics", web::get().to(get_analytics))
            .route("/stats", web::get().to(get_blockchain_stats))
    );
}

async fn get_dashboard_data(pool: web::Data<DbPool>, req: actix_web::HttpRequest) -> impl Responder {
    match dashboard_handler::get_dashboard_data(&pool, &req).await {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_analytics(pool: web::Data<DbPool>) -> impl Responder {
    match dashboard_handler::get_analytics(&pool).await {
        Ok(analytics) => HttpResponse::Ok().json(analytics),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_blockchain_stats(rpc: web::Data<RpcService>) -> impl Responder {
    match dashboard_handler::get_blockchain_stats(&rpc).await {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
