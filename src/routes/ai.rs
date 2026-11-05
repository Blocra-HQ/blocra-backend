use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::services::ai::{AiError, AiService, ChatMessage};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ai")
            // Report requests carry contract stats; allow up to 256 KB.
            .app_data(web::JsonConfig::default().limit(256 * 1024))
            .route("/status", web::get().to(status))
            .route("/chat", web::post().to(chat))
            .route("/report", web::post().to(report))
            .route("/sql", web::post().to(sql)),
    );
}

fn error(e: AiError) -> HttpResponse {
    let body = json!({ "success": false, "message": e.to_string() });
    match e {
        AiError::NotConfigured => HttpResponse::ServiceUnavailable().json(body),
        AiError::BadRequest(_) => HttpResponse::BadRequest().json(body),
        AiError::Upstream(_) => HttpResponse::BadGateway().json(body),
    }
}

/// GET /api/ai/status: lets the frontend show whether AI is available.
async fn status(ai: web::Data<AiService>) -> impl Responder {
    HttpResponse::Ok().json(json!({
        "enabled": ai.is_configured(),
        "model": if ai.is_configured() { Some(ai.model()) } else { None },
    }))
}

#[derive(Deserialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
    #[serde(default)]
    context: Option<Value>,
}

/// POST /api/ai/chat {messages: [{role, content}], context?}
async fn chat(ai: web::Data<AiService>, body: web::Json<ChatRequest>) -> impl Responder {
    let body = body.into_inner();
    match ai.chat(body.messages, body.context).await {
        Ok(reply) => HttpResponse::Ok().json(json!({ "reply": reply, "model": ai.model() })),
        Err(e) => error(e),
    }
}

#[derive(Deserialize)]
struct ReportRequest {
    contract_name: String,
    contract_address: String,
    chain: String,
    stats: Value,
}

/// POST /api/ai/report {contract_name, contract_address, chain, stats}
async fn report(ai: web::Data<AiService>, body: web::Json<ReportRequest>) -> impl Responder {
    match ai
        .contract_report(&body.contract_name, &body.contract_address, &body.chain, &body.stats)
        .await
    {
        Ok(report) => HttpResponse::Ok().json(json!({ "report": report, "model": ai.model() })),
        Err(e) => error(e),
    }
}

#[derive(Deserialize)]
struct SqlRequest {
    question: String,
}

/// POST /api/ai/sql {question}
async fn sql(ai: web::Data<AiService>, body: web::Json<SqlRequest>) -> impl Responder {
    match ai.generate_sql(&body.question).await {
        Ok(sql) => HttpResponse::Ok().json(json!({ "sql": sql, "model": ai.model() })),
        Err(e) => error(e),
    }
}
