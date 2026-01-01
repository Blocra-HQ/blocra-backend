use actix_web::{web, HttpResponse};
use crate::handlers::feedback;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/feedback")
            .route("/submit", web::post().to(submit_feedback))
    );
}

async fn submit_feedback(
    payload: web::Json<feedback::FeedbackPayload>,
) -> Result<HttpResponse, actix_web::Error> {
    let response = feedback::submit_feedback(payload).await?;
    Ok(HttpResponse::Ok().json(response))
}
