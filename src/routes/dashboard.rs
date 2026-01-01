use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::dashboard as dashboard_handler, db::DbPool, services::rpc::RpcService};
use serde::Deserialize;

#[derive(Deserialize)]
struct StatsQuery {
    chain: Option<String>,
}

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

async fn get_blockchain_stats(
    rpc: web::Data<RpcService>,
    query: web::Query<StatsQuery>,
) -> impl Responder {
    // Only fetch if Starknet is selected (or no chain specified for backwards compatibility)
    let chain_id = query.chain.as_deref().unwrap_or("starknet");
    
    if chain_id != "starknet" {
        // Return mock data for other chains
        return HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "block_number": 0,
            "timestamp": chrono::Utc::now().timestamp() as u64,
            "stats": {
                "total_transactions": 0,
                "active_users": 0,
                "gas_used": 0,
                "volume": 0,
                "tvl": 0,
                "unique_senders": 0,
                "total_fees": "0",
                "avg_fee": "0",
                "successful_txs": 0,
                "failed_txs": 0,
                "pending_txs": 0
            },
            "block_info": {
                "block_number": 0,
                "timestamp": chrono::Utc::now().timestamp() as u64,
                "transaction_count": 0
            },
            "message": format!("Chain '{}' not yet supported. Only Starknet is active.", chain_id)
        }));
    }
    
    // Fetch real data for Starknet
    match dashboard_handler::get_blockchain_stats(&rpc).await {
        Ok(stats) => HttpResponse::Ok().json(stats),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
