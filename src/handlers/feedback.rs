use actix_web::web;
use serde::{Deserialize, Serialize};
use crate::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct FeedbackPayload {
    pub name: String,
    pub feedback: String,
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
    // Log feedback for now (in production, this would send an email)
    log::info!(
        "Feedback received from {}: {}",
        payload.name,
        payload.feedback
    );

    // TODO: Implement email sending using SMTP
    // For now, we'll just log it and return success
    // In production, you would:
    // 1. Use lettre crate for SMTP
    // 2. Send email to ndifrekemkpanam@gmail.com
    // 3. Include name, feedback, and user_email in the email body

    Ok(web::Json(FeedbackResponse {
        success: true,
        message: "Thank you for your feedback!".to_string(),
    }))
}
