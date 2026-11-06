//! SQL over the analytics store (see services::analytics).
use actix_web::{web, HttpResponse, ResponseError};
use serde::Deserialize;
use serde_json::json;

use crate::services::analytics::{Analytics, TABLES};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/query")
            .app_data(web::JsonConfig::default().limit(64 * 1024))
            .route("/schema", web::get().to(schema))
            .route("/run", web::post().to(run)),
    );
}

/// GET /api/query/schema: tables, columns and how much data is stored.
async fn schema(analytics: web::Data<Analytics>) -> HttpResponse {
    let tables: Vec<_> = TABLES
        .iter()
        .map(|(name, description, cols)| {
            json!({
                "name": name,
                "description": description,
                "columns": cols.iter().map(|(c, t, d)| json!({ "name": c, "type": t, "description": d })).collect::<Vec<_>>(),
            })
        })
        .collect();
    match analytics.coverage().await {
        Ok(coverage) => HttpResponse::Ok().json(json!({ "success": true, "tables": tables, "coverage": coverage })),
        Err(e) => crate::errors::AppError::DatabaseError(e).error_response(),
    }
}

#[derive(Deserialize)]
struct RunRequest {
    sql: String,
}

/// POST /api/query/run {sql}: read-only SQL, at most 1000 rows.
async fn run(analytics: web::Data<Analytics>, body: web::Json<RunRequest>) -> HttpResponse {
    match analytics.run_query(&body.sql).await {
        Ok(result) => {
            let mut v = serde_json::to_value(result).unwrap_or_default();
            v["success"] = json!(true);
            HttpResponse::Ok().json(v)
        }
        Err(e) => HttpResponse::BadRequest().json(json!({ "success": false, "message": e.to_string().trim_start_matches("Bad request: ") })),
    }
}
