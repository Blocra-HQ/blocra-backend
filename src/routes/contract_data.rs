//! Contract data from the analytics store and the chain:
//!
//! - GET /api/contracts/{id}/interface[?refresh=true]: functions, events and
//!   types read from the contract's code (cached for a day).
//! - GET /api/contracts/{id}/calls?limit=50: recent direct calls, arguments
//!   named from the interface.
//! - GET /api/contracts/{id}/events?name=&limit=50: recent events, fields
//!   named from the interface, token movements decoded.
//! - GET /api/contracts/{id}/activity?days=7: daily callers, users reached
//!   through other contracts, events and token flows.

use actix_web::{web, HttpResponse, ResponseError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::errors::AppError;
use crate::services::analytics::{self, Analytics};
use crate::services::contract_spec;
use crate::services::soroban_rpc::SorobanRpc;

/// Interfaces are re-read after a day, since contracts can be upgraded.
const INTERFACE_TTL_SECS: i64 = 86_400;

pub fn routes(scope: &mut web::ServiceConfig) {
    scope
        .app_data(web::Data::new(SorobanRpc::new()))
        .route("/{id}/interface", web::get().to(interface))
        .route("/{id}/calls", web::get().to(calls))
        .route("/{id}/events", web::get().to(events))
        .route("/{id}/activity", web::get().to(activity));
}

fn contract_id(raw: &str) -> Result<String, AppError> {
    let id = raw.trim().to_ascii_uppercase();
    stellar_strkey::Contract::from_string(&id)
        .map(|_| id.clone())
        .map_err(|_| AppError::BadRequest(format!("Not a Stellar contract id (C...): {raw}")))
}

/// The cached interface, refreshed when older than a day or on request.
async fn load_interface(store: &Analytics, rpc: &SorobanRpc, id: &str, refresh: bool) -> Result<Value, AppError> {
    if !refresh {
        if let Some((cached, at)) = store.cached_interface(id).await? {
            if analytics::now() - at < INTERFACE_TTL_SECS {
                return Ok(cached);
            }
        }
    }
    match contract_spec::fetch(rpc, id).await {
        Ok(fresh) => {
            store.store_interface(id, &fresh).await?;
            Ok(fresh)
        }
        // Serve a stale copy rather than nothing when the RPC is down.
        Err(e) => match store.cached_interface(id).await? {
            Some((cached, _)) if !matches!(e, AppError::NotFound(_)) => Ok(cached),
            _ => Err(e),
        },
    }
}

/// The interface for naming fields, or `None` (fields stay positional).
async fn interface_for_naming(store: &Analytics, rpc: &SorobanRpc, id: &str) -> Option<Value> {
    load_interface(store, rpc, id, false).await.ok()
}

#[derive(Deserialize)]
struct InterfaceQuery {
    refresh: Option<bool>,
}

async fn interface(
    path: web::Path<String>,
    q: web::Query<InterfaceQuery>,
    store: web::Data<Analytics>,
    rpc: web::Data<SorobanRpc>,
) -> HttpResponse {
    let result = async {
        let id = contract_id(&path)?;
        load_interface(&store, &rpc, &id, q.refresh.unwrap_or(false)).await
    }
    .await;
    match result {
        Ok(i) => HttpResponse::Ok().json(json!({ "success": true, "interface": i })),
        Err(e) => e.error_response(),
    }
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<i64>,
    name: Option<String>,
}

async fn calls(
    path: web::Path<String>,
    q: web::Query<ListQuery>,
    store: web::Data<Analytics>,
    rpc: web::Data<SorobanRpc>,
) -> HttpResponse {
    let result = async {
        let id = contract_id(&path)?;
        let mut calls = store.contract_calls(&id, q.limit.unwrap_or(50)).await?;
        if let Some(i) = interface_for_naming(&store, &rpc, &id).await {
            for c in &mut calls {
                let args = c["args"].as_array().cloned().unwrap_or_default();
                c["named_args"] = contract_spec::name_call(&i, c["function"].as_str().unwrap_or(""), &args);
            }
        }
        Ok::<_, AppError>(json!({ "success": true, "contract": id, "calls": calls }))
    }
    .await;
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => e.error_response(),
    }
}

async fn events(
    path: web::Path<String>,
    q: web::Query<ListQuery>,
    store: web::Data<Analytics>,
    rpc: web::Data<SorobanRpc>,
) -> HttpResponse {
    let result = async {
        let id = contract_id(&path)?;
        let mut events = store.contract_events(&id, q.name.as_deref(), q.limit.unwrap_or(50)).await?;
        let interface = interface_for_naming(&store, &rpc, &id).await;
        for e in &mut events {
            let topics = e["topics"].as_array().cloned().unwrap_or_default();
            let named = interface.as_ref().and_then(|i| contract_spec::name_event(i, &topics, &e["data"]));
            let token_move = e["token_move"].clone();
            match (named, &token_move) {
                (Some((name, fields)), _) => {
                    e["event"] = json!(name);
                    e["fields"] = fields;
                }
                (None, Value::Object(m)) => {
                    e["event"] = e["name"].clone();
                    e["fields"] = Value::Object(m.clone());
                }
                _ => {
                    e["event"] = e["name"].clone();
                    e["fields"] = json!({ "topics": topics.get(1..).unwrap_or_default(), "data": e["data"] });
                }
            }
        }
        Ok::<_, AppError>(json!({ "success": true, "contract": id, "events": events }))
    }
    .await;
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => e.error_response(),
    }
}

#[derive(Deserialize)]
struct ActivityQuery {
    days: Option<i64>,
}

async fn activity(path: web::Path<String>, q: web::Query<ActivityQuery>, store: web::Data<Analytics>) -> HttpResponse {
    let result = async {
        let id = contract_id(&path)?;
        Ok::<_, AppError>(store.contract_activity(&id, q.days.unwrap_or(7)).await?)
    }
    .await;
    match result {
        Ok(mut body) => {
            body["success"] = json!(true);
            HttpResponse::Ok().json(body)
        }
        Err(e) => e.error_response(),
    }
}
