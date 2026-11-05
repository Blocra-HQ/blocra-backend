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

/// Shortest JWT_SECRET accepted: HS256 needs a long random key to be safe.
pub const MIN_SECRET_LEN: usize = 32;

/// JWT_SECRET, or an error when it is unset or too short. There is no
/// fallback: a default secret in public source would let anyone mint tokens.
pub fn secret() -> Result<String, String> {
    match env::var("JWT_SECRET") {
        Ok(s) if s.len() >= MIN_SECRET_LEN => Ok(s),
        Ok(_) => Err(format!("JWT_SECRET must be at least {MIN_SECRET_LEN} characters")),
        Err(_) => Err("JWT_SECRET is not set (generate one with: openssl rand -base64 32)".into()),
    }
}

pub fn create_token(user_id: i64) -> Result<String, AppError> {
    let secret = secret().map_err(AppError::Unauthorized)?;
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
    let secret = secret().map_err(AppError::Unauthorized)?;
    
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
