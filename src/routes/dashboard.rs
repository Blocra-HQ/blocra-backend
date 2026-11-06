use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::dashboard as dashboard_handler, db::DbPool, services::starknet::StarknetRpc, services::stellar::StellarService, services::chains::{self, ChainRpc}, services::analytics::{self, Analytics}, services::collector};
use serde::Deserialize;

#[derive(Deserialize)]
struct StatsQuery {
    chain: Option<String>,
}

#[derive(Deserialize)]
struct HistoryQuery {
    chain: Option<String>,
    limit: Option<i64>,
}

/// A stored summary younger than this is served instead of asking the RPC
/// again. Stellar ledgers close every ~5 s and the collector stores each one.
const FRESH_SECS: i64 = 8;
/// Serve stored Stellar ledgers up to this old before reading Horizon live.
const STELLAR_STALE_SECS: i64 = 60;

pub fn configure(cfg: &mut web::ServiceConfig) {
    let rpc_service = web::Data::new(StarknetRpc::new());
    let stellar_service = web::Data::new(StellarService::from_env());
    let chain_rpc = web::Data::new(ChainRpc::new());

    cfg.service(
        web::scope("/dashboards")
            .app_data(rpc_service)
            .app_data(stellar_service)
            .app_data(chain_rpc)
            .route("", web::get().to(get_dashboard_data))
            .route("/analytics", web::get().to(get_analytics))
            .route("/stats", web::get().to(get_blockchain_stats))
            .route("/history", web::get().to(get_history))
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
    rpc: web::Data<StarknetRpc>,
    stellar: web::Data<StellarService>,
    chain_rpc: web::Data<ChainRpc>,
    store: web::Data<Analytics>,
    query: web::Query<StatsQuery>,
) -> impl Responder {
    let chain_id = query.chain.as_deref().unwrap_or("stellar");

    // Serve a block stored moments ago (by the Stellar collector or an
    // earlier request) rather than re-reading it from the RPC.
    if let Ok(Some((summary, collected_at))) = store.latest_summary(chain_id).await {
        // The collector stores every Stellar ledger, so the store is the
        // source of truth unless it has stalled (a live Horizon read takes
        // 5-15 s). Other chains are only stored when fetched.
        let ledger_age = analytics::now() - summary["timestamp"].as_i64().unwrap_or(0);
        let fresh = if chain_id == "stellar" && collector::enabled() {
            ledger_age <= STELLAR_STALE_SECS
        } else {
            analytics::now() - collected_at <= FRESH_SECS
        };
        if fresh {
            return HttpResponse::Ok().json(summary);
        }
    }

    let result = match chain_id {
        "stellar" => dashboard_handler::get_stellar_stats(&stellar).await,
        "starknet" => dashboard_handler::get_blockchain_stats(&rpc).await,
        other => match chains::chain(other) {
            Some(chain) => dashboard_handler::get_chain_stats(&chain_rpc, other, chain).await,
            None => {
                return HttpResponse::NotFound().json(serde_json::json!({
                    "success": false,
                    "message": format!(
                        "Unknown chain '{other}'. Supported chains: {}.",
                        chains::registry().keys().map(String::as_str).collect::<Vec<_>>().join(", ")
                    )
                }))
            }
        },
    };
    match result {
        Ok(stats) => {
            // Every block a dashboard fetches becomes part of the history.
            if let Err(e) = store.record_summary(chain_id, &stats).await {
                log::warn!("Could not store {chain_id} block: {e}");
            }
            HttpResponse::Ok().json(stats)
        }
        Err(e) => HttpResponse::BadGateway().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        })),
    }
}

/// GET /api/dashboards/history?chain=&limit= : stored blocks, oldest first,
/// each in the /stats response shape.
async fn get_history(store: web::Data<Analytics>, query: web::Query<HistoryQuery>) -> impl Responder {
    let chain_id = query.chain.as_deref().unwrap_or("stellar");
    if chain_id != "stellar" && chain_id != "starknet" && chains::chain(chain_id).is_none() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "message": format!("Unknown chain '{chain_id}'")
        }));
    }
    let limit = query.limit.unwrap_or(60).clamp(1, 500);
    match store.history(chain_id, limit).await {
        Ok(points) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "chain": chain_id,
            "points": points
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        })),
    }
}
