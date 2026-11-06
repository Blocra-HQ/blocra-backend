//! Traction reports (see services::reports).
use actix_web::{web, HttpResponse, ResponseError};
use serde::Deserialize;
use serde_json::json;

use crate::errors::AppError;
use crate::services::analytics::Analytics;
use crate::services::reports;
use crate::services::stellar::StellarService;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/reports")
            .app_data(web::Data::new(StellarService::from_env()))
            .route("/subjects", web::get().to(subjects))
            .route("/traction", web::get().to(traction)),
    );
}

#[derive(Deserialize)]
struct SubjectsQuery {
    kind: Option<String>,
    days: Option<i64>,
    limit: Option<i64>,
}

/// GET /api/reports/subjects?kind=contract|asset&days=7&limit=20
async fn subjects(store: web::Data<Analytics>, q: web::Query<SubjectsQuery>) -> HttpResponse {
    let kind = match q.kind.as_deref().unwrap_or("contract") {
        "asset" => "asset",
        _ => "contract",
    };
    match reports::top_subjects(&store, kind, q.days.unwrap_or(7), q.limit.unwrap_or(20)).await {
        Ok(list) => HttpResponse::Ok().json(json!({ "success": true, "kind": kind, "subjects": list })),
        Err(e) => e.error_response(),
    }
}

#[derive(Deserialize)]
struct TractionQuery {
    subject: String,
    days: Option<i64>,
}

/// GET /api/reports/traction?subject=C...|XLM|CODE:ISSUER&days=7
async fn traction(
    store: web::Data<Analytics>,
    stellar: web::Data<StellarService>,
    q: web::Query<TractionQuery>,
) -> HttpResponse {
    match reports::traction_report(&store, &stellar, &q.subject, q.days.unwrap_or(7)).await {
        Ok(report) => HttpResponse::Ok().json(json!({ "success": true, "report": report })),
        Err(AppError::NotFound(m)) => HttpResponse::NotFound().json(json!({ "success": false, "message": m })),
        Err(e) => e.error_response(),
    }
}
