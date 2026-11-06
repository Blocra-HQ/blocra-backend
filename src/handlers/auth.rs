use crate::{db::DbPool, models::user::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use actix_multipart::Multipart;
use futures_util::StreamExt;
use std::io::Write;
use serde::{Deserialize, Serialize};
use bcrypt::{hash, verify, DEFAULT_COST};
use reqwest;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct RegisterPayload {
    pub email: String,
    pub password: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LoginPayload {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleAuthPayload {
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WalletAuthPayload {
    pub wallet_address: String,
    pub signature: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefreshTokenPayload {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfilePayload {
    pub username: Option<String>,
    pub email: Option<String>,
    pub profile_picture: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub success: bool,
    pub token: String,
    pub user: UserResponse,
}

/// Trimmed, lower-cased email, or an error if it isn't plausibly an address.
pub fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid {
        Ok(email)
    } else {
        Err(AppError::BadRequest("Please enter a valid email address".to_string()))
    }
}

pub async fn register(
    pool: &DbPool,
    mut payload: RegisterPayload,
) -> Result<AuthResponse, AppError> {
    payload.email = normalize_email(&payload.email)?;
    if payload.password.is_none() {
        return Err(AppError::BadRequest("Password is required".to_string()));
    }
    // Check if user exists
    let existing: Option<User> = sqlx::query_as(
        "SELECT * FROM users WHERE email = ?"
    )
    .bind(&payload.email)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest("User already exists".to_string()));
    }

    // Hash password if provided
    let password_hash = if let Some(password) = payload.password {
        if password.len() < 6 {
            return Err(AppError::BadRequest("Password must be at least 6 characters".to_string()));
        }
        Some(hash(password, DEFAULT_COST)
            .map_err(|e| AppError::BadRequest(format!("Failed to hash password: {}", e)))?)
    } else {
        None
    };

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, username, password_hash, role) VALUES (?, ?, ?, 'user')
         RETURNING *"
    )
    .bind(&payload.email)
    .bind(payload.username)
    .bind(password_hash)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

pub async fn login(
    pool: &DbPool,
    payload: LoginPayload,
) -> Result<AuthResponse, AppError> {
    let user: User = sqlx::query_as(
        "SELECT * FROM users WHERE email = ?"
    )
    .bind(payload.email.trim().to_lowercase())
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::Unauthorized("Invalid credentials".to_string()))?;

    // Verify password
    if let Some(password_hash) = &user.password_hash {
        let valid = verify(&payload.password, password_hash)
            .map_err(|_| AppError::Unauthorized("Invalid credentials".to_string()))?;
        
        if !valid {
            return Err(AppError::Unauthorized("Invalid credentials".to_string()));
        }
    } else {
        // Allow login without password for OAuth-only accounts
        log::warn!("User {} has no password set, allowing login", user.id);
    }

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

pub async fn google_authenticate(
    pool: &DbPool,
    payload: GoogleAuthPayload,
) -> Result<AuthResponse, AppError> {
    // Verify Google token with Google's API
    let google_user_info = verify_google_token(&payload.token).await?;
    
    // Extract email and google_id from verified token
    let email = google_user_info.get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Unauthorized("Invalid Google token: missing email".to_string()))?;
    
    let google_id = google_user_info.get("sub")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Unauthorized("Invalid Google token: missing sub".to_string()))?;
    
    // Create or update user
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, google_id, role) VALUES (?, ?, 'user')
         ON CONFLICT(email) DO UPDATE SET google_id = ?, updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(email)
    .bind(google_id)
    .bind(google_id)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

/// Verify Google OAuth token with Google's API
async fn verify_google_token(token: &str) -> Result<Value, AppError> {
    // Without our client id we can't tell our tokens from any other app's.
    let client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
    if client_id.is_empty() {
        return Err(AppError::BadRequest("Google sign-in is not configured on this server".to_string()));
    }
    let client = reqwest::Client::new();
    let response = client
        .get("https://oauth2.googleapis.com/tokeninfo")
        .query(&[("id_token", token)])
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Failed to verify Google token: {}", e)))?;
    
    if !response.status().is_success() {
        return Err(AppError::Unauthorized("Invalid Google token".to_string()));
    }
    
    let user_info: Value = response.json().await
        .map_err(|e| AppError::Unauthorized(format!("Failed to parse Google response: {}", e)))?;
    
    // Verify token is not expired
    if let Some(exp) = user_info.get("exp").and_then(|v| v.as_str()) {
        let exp_timestamp: i64 = exp.parse()
            .map_err(|_| AppError::Unauthorized("Invalid expiration time".to_string()))?;
        let now = chrono::Utc::now().timestamp();
        
        if now > exp_timestamp {
            return Err(AppError::Unauthorized("Google token expired".to_string()));
        }
    }
    
    // The token must have been issued to BlocRA, not to some other app.
    if user_info.get("aud").and_then(|v| v.as_str()) != Some(client_id.as_str()) {
        return Err(AppError::Unauthorized("Google token was issued to a different app".to_string()));
    }
    if user_info.get("email_verified").and_then(|v| v.as_str()) != Some("true") {
        return Err(AppError::Unauthorized("Google account email is not verified".to_string()));
    }

    Ok(user_info)
}

/// Signs in (or registers) the Stellar account that `auth::stellar_wallet`
/// has just verified a signed challenge for.
pub async fn stellar_sign_in(pool: &DbPool, account: &str) -> Result<AuthResponse, AppError> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (wallet_address, role) VALUES (?, 'user')
         ON CONFLICT(wallet_address) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(account)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

pub async fn refresh_token(
    pool: &DbPool,
    payload: RefreshTokenPayload,
) -> Result<AuthResponse, AppError> {
    // Verify the refresh token
    let claims = jwt::verify_token(&payload.refresh_token)?;
    
    // Check if token is expired
    let now = chrono::Utc::now().timestamp();
    if claims.exp < now {
        return Err(AppError::Unauthorized("Refresh token expired".to_string()));
    }
    
    // Get user from database
    let user: User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(claims.sub)
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::Unauthorized("User not found".to_string()))?;

    // Generate new access token
    let new_token = jwt::create_token(user.id)?;

    log::info!("Refresh token successful for user {}", user.id);

    Ok(AuthResponse {
        success: true,
        token: new_token,
        user: user.into(),
    })
}

pub async fn get_current_user(
    pool: &DbPool,
    req: &HttpRequest,
) -> Result<UserResponse, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(user.into())
}

#[derive(Debug, Serialize)]
pub struct ImageUploadResponse {
    pub success: bool,
    pub profile_picture: String,
}

/// Largest accepted profile picture.
pub const MAX_PROFILE_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// File extension for a JPEG, PNG or WebP image, by its magic bytes.
pub fn image_extension(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

#[cfg(test)]
mod upload_tests {
    use super::image_extension;

    #[test]
    fn detects_images_by_content() {
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(image_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"<html><script>"), None);
        assert_eq!(image_extension(b""), None);
    }
}

pub async fn upload_profile_image(
    pool: &DbPool,
    req: &HttpRequest,
    mut payload: Multipart,
) -> Result<ImageUploadResponse, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Create uploads directory if it doesn't exist
    std::fs::create_dir_all("uploads/profiles")
        .map_err(|e| AppError::BadRequest(format!("Failed to create upload directory: {}", e)))?;

    // Read the first file field into memory, capped at MAX_PROFILE_IMAGE_BYTES.
    let mut data: Vec<u8> = Vec::new();
    if let Some(item) = payload.next().await {
        let mut field = item.map_err(|e| AppError::BadRequest(format!("Failed to read field: {}", e)))?;
        while let Some(chunk) = field.next().await {
            let chunk = chunk.map_err(|e| AppError::BadRequest(format!("Failed to read chunk: {}", e)))?;
            if data.len() + chunk.len() > MAX_PROFILE_IMAGE_BYTES {
                return Err(AppError::BadRequest(format!(
                    "Image is larger than {} MB", MAX_PROFILE_IMAGE_BYTES / 1024 / 1024
                )));
            }
            data.extend_from_slice(&chunk);
        }
    }
    if data.is_empty() {
        return Err(AppError::BadRequest("No file uploaded".to_string()));
    }

    // Trust the bytes, not the client's filename or content type.
    let ext = image_extension(&data)
        .ok_or_else(|| AppError::BadRequest("Only JPEG, PNG or WebP images are allowed".to_string()))?;
    let filename = format!("profile_{}_{}.{}", user_id, chrono::Utc::now().timestamp(), ext);
    let file_path = format!("uploads/profiles/{}", filename);
    let mut file = std::fs::File::create(&file_path)
        .map_err(|e| AppError::BadRequest(format!("Failed to create file: {}", e)))?;
    file.write_all(&data)
        .map_err(|e| AppError::BadRequest(format!("Failed to write file: {}", e)))?;



    // Update user profile picture in database
    let profile_picture_url = format!("/{}", file_path);
    sqlx::query(
        "UPDATE users SET profile_picture = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
    )
    .bind(&profile_picture_url)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(ImageUploadResponse {
        success: true,
        profile_picture: profile_picture_url,
    })
}

pub async fn update_profile(
    pool: &DbPool,
    req: &HttpRequest,
    payload: UpdateProfilePayload,
) -> Result<UserResponse, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    let mut query = String::from("UPDATE users SET updated_at = CURRENT_TIMESTAMP");
    let mut has_updates = false;

    if payload.username.is_some() {
        query.push_str(", username = ?");
        has_updates = true;
    }
    if payload.email.is_some() {
        query.push_str(", email = ?");
        has_updates = true;
    }
    if payload.profile_picture.is_some() {
        query.push_str(", profile_picture = ?");
        has_updates = true;
    }

    if !has_updates {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    query.push_str(" WHERE id = ? RETURNING *");

    let mut q = sqlx::query_as::<_, User>(&query);
    
    if let Some(username) = payload.username {
        q = q.bind(username);
    }
    if let Some(email) = payload.email {
        q = q.bind(email);
    }
    if let Some(profile_picture) = payload.profile_picture {
        q = q.bind(profile_picture);
    }
    
    q = q.bind(user_id);

    let user = q.fetch_one(pool).await?;

    Ok(user.into())
}
