use actix_web::{web, HttpResponse, Responder};
use serde_json::json;

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(json!({
        "status": "OK",
        "uptime": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

async fn root() -> impl Responder {
    HttpResponse::Ok().json(json!({
        "success": true,
        "message": "Welcome to BlocRA Rust API"
    }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/health")
            .route(web::get().to(health_check))
    )
    .service(
        web::resource("/")
            .route(web::get().to(root))
    );
}
