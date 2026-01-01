use actix_web::web;
use serde::{Deserialize, Serialize};
use crate::errors::AppError;
use lettre::{Message, SmtpTransport, Transport};
use lettre::transport::smtp::authentication::Credentials;

#[derive(Debug, Deserialize)]
pub struct FeedbackPayload {
    pub name: String;
    pub feedback: String;
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

    // Send email if SMTP is configured
    if let Err(e) = send_feedback_email(&payload.name, &payload.feedback, payload.user_email.as_deref()).await {
        log::error!("Failed to send feedback email: {}", e);
        // Don't fail the request if email fails - feedback is still logged
    }

    Ok(web::Json(FeedbackResponse {
        success: true,
        message: "Thank you for your feedback!".to_string(),
    }))
}

/// Send feedback email via SMTP
async fn send_feedback_email(name: &str, feedback: &str, user_email: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    // Get SMTP configuration from environment
    let smtp_host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "smtp.gmail.com".to_string());
    let smtp_port: u16 = std::env::var("SMTP_PORT")
        .unwrap_or_else(|_| "587".to_string())
        .parse()
        .unwrap_or(587);
    let smtp_username = std::env::var("SMTP_USERNAME")?;
    let smtp_password = std::env::var("SMTP_PASSWORD")?;
    let admin_email = std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "ndifrekemkpanam@gmail.com".to_string());

    // Build email body
    let email_body = format!(
        "New Feedback Received\n\n\
         From: {}\n\
         User Email: {}\n\n\
         Feedback:\n{}\n\n\
         ---\n\
         Sent from BlocRA Feedback System",
        name,
        user_email.unwrap_or("Not provided"),
        feedback
    );

    // Create email
    let email = Message::builder()
        .from(smtp_username.parse()?)
        .to(admin_email.parse()?)
        .subject(format!("BlocRA Feedback from {}", name))
        .body(email_body)?;

    // Create SMTP transport
    let creds = Credentials::new(smtp_username, smtp_password);
    let mailer = SmtpTransport::relay(&smtp_host)?
        .port(smtp_port)
        .credentials(creds)
        .build();

    // Send email
    mailer.send(&email)?;
    
    log::info!("Feedback email sent successfully to {}", admin_email);
    Ok(())
}
