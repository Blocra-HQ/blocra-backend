use actix_web::HttpRequest;
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use serde::{Deserialize, Serialize};
use std::env;
use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub exp: i64,
}

pub fn create_token(user_id: i64) -> Result<String, AppError> {
    let secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| {
            log::warn!("JWT_SECRET not set, using default");
            "default_jwt_secret_change_in_production_12345678901234567890".to_string()
        });
    let expiration = env::var("JWT_EXPIRATION")
        .unwrap_or_else(|_| "86400".to_string())
        .parse::<i64>()
        .unwrap_or(86400);
    
    let claims = Claims {
        sub: user_id,
        exp: chrono::Utc::now().timestamp() + expiration,
    };

    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .map_err(|_| AppError::Unauthorized("Failed to create token".to_string()))
}

pub fn verify_token(token: &str) -> Result<Claims, AppError> {
    let secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| {
            log::warn!("JWT_SECRET not set, using default");
            "default_jwt_secret_change_in_production_12345678901234567890".to_string()
        });
    
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("Invalid token".to_string()))
}

pub fn extract_user_id(req: &HttpRequest) -> Result<i64, AppError> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Unauthorized("Missing authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(AppError::Unauthorized("Invalid authorization format".to_string()))?;

    let claims = verify_token(token)?;
    Ok(claims.sub)
}
