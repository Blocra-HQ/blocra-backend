//! Traction reports (see services::reports).
use actix_web::{web, HttpResponse, ResponseError};
use serde::Deserialize;
use serde_json::json;

use crate::errors::AppError;
use crate::services::analytics::Analytics;
use crate::services::attestation::{Attestor, METHOD_VERSION};
use crate::services::reports;
use crate::services::stellar::StellarService;

/// Chains traction reports support today. The CLI and API take a chain so
/// more can be added without changing either.
const REPORT_CHAINS: &[&str] = &["stellar"];

pub fn configure(cfg: &mut web::ServiceConfig) {
    let attestor = web::Data::new(Attestor::from_env());
    if !attestor.is_configured() {
        log::warn!("BLOCRA_SIGNING_SECRET not set; signed reports (?signed=true) answer 503");
    }
    cfg.service(
        web::scope("/reports")
            .app_data(web::Data::new(StellarService::from_env()))
            .app_data(attestor.clone())
            .route("/subjects", web::get().to(subjects))
            .route("/traction", web::get().to(traction)),
    )
    .service(web::scope("/attestation").app_data(attestor).route("/key", web::get().to(signing_key)));
}

/// GET /api/attestation/key: the public key reports are signed with.
async fn signing_key(attestor: web::Data<Attestor>) -> HttpResponse {
    match attestor.signer() {
        Some(signer) => HttpResponse::Ok().json(json!({
            "success": true, "algorithm": "ed25519", "signer": signer,
            "method_version": METHOD_VERSION, "chains": REPORT_CHAINS,
        })),
        None => HttpResponse::ServiceUnavailable().json(json!({
            "success": false, "message": "This server does not sign reports (BLOCRA_SIGNING_SECRET is not set)"
        })),
    }
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
    chain: Option<String>,
    signed: Option<bool>,
    /// Comma-separated relayer accounts the project runs.
    relayers: Option<String>,
}

/// GET /api/reports/traction?subject=C...|XLM|CODE:ISSUER&days=7[&chain=stellar][&signed=true]
///
/// With signed=true the response is `{success, signed: {report_json, attestation}}`,
/// where attestation is BlocRA's Ed25519 signature over report_json.
async fn traction(
    store: web::Data<Analytics>,
    stellar: web::Data<StellarService>,
    attestor: web::Data<Attestor>,
    q: web::Query<TractionQuery>,
) -> HttpResponse {
    let chain = q.chain.as_deref().unwrap_or("stellar").to_ascii_lowercase();
    if !REPORT_CHAINS.contains(&chain.as_str()) {
        return HttpResponse::BadRequest().json(json!({
            "success": false,
            "message": format!("Traction reports are not available for '{chain}' yet. Supported: {}.", REPORT_CHAINS.join(", "))
        }));
    }
    let relayers: Vec<String> = q
        .relayers
        .as_deref()
        .map(|r| r.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let report = match reports::traction_report(&store, &stellar, &q.subject, q.days.unwrap_or(7), &relayers).await {
        Ok(r) => r,
        Err(AppError::NotFound(m)) => return HttpResponse::NotFound().json(json!({ "success": false, "message": m })),
        Err(AppError::BadRequest(m)) => return HttpResponse::BadRequest().json(json!({ "success": false, "message": m })),
        Err(e) => return e.error_response(),
    };
    if q.signed.unwrap_or(false) {
        return match attestor.sign(&report) {
            Some(envelope) => HttpResponse::Ok().json(json!({ "success": true, "signed": envelope })),
            None => HttpResponse::ServiceUnavailable().json(json!({
                "success": false,
                "message": "This server does not sign reports (BLOCRA_SIGNING_SECRET is not set)"
            })),
        };
    }
    HttpResponse::Ok().json(json!({ "success": true, "report": report }))
}
