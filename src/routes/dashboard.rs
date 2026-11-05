use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::dashboard as dashboard_handler, db::DbPool, services::rpc::RpcService, services::stellar::StellarService};
use serde::Deserialize;

#[derive(Deserialize)]
struct StatsQuery {
    chain: Option<String>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    let rpc_service = web::Data::new(RpcService::new());
    let stellar_service = web::Data::new(StellarService::from_env());

    cfg.service(
        web::scope("/dashboards")
            .app_data(rpc_service)
            .app_data(stellar_service)
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

async fn get_blockchain_stats(
    rpc: web::Data<RpcService>,
    stellar: web::Data<StellarService>,
    query: web::Query<StatsQuery>,
) -> impl Responder {
    let chain_id = query.chain.as_deref().unwrap_or("stellar");
    let result = match chain_id {
        "stellar" => dashboard_handler::get_stellar_stats(&stellar).await,
        "starknet" => dashboard_handler::get_blockchain_stats(&rpc).await,
        other => {
            return HttpResponse::NotImplemented().json(serde_json::json!({
                "success": false,
                "message": format!(
                    "Network dashboard stats are available for Stellar and Starknet; '{other}' is not supported yet. Use Contract Analysis for this chain."
                )
            }))
        }
    };
    match result {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => HttpResponse::BadGateway().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        })),
    }
}
