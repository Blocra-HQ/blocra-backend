use actix_web::web;
use serde::{Deserialize, Serialize};
use crate::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct FeedbackPayload {
    pub name: String,
    pub feedback: String,
    #[allow(dead_code)]
    pub user_email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub success: bool,
    pub message: String,
}

pub async fn submit_feedback(
    payload: web::Json<FeedbackPayload>,
) -> Result<web::Json<FeedbackResponse>, AppError> {
    // Log feedback
    log::info!(
        "Feedback received from {}: {}",
        payload.name,
        payload.feedback
    );

    // Email functionality disabled for now
    // Can be enabled by adding lettre dependency and SMTP configuration

    Ok(web::Json(FeedbackResponse {
        success: true,
        message: "Thank you for your feedback!".to_string(),
    }))
}
