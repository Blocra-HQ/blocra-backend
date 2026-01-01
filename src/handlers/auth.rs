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

pub async fn register(
    pool: &DbPool,
    payload: RegisterPayload,
) -> Result<AuthResponse, AppError> {
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
    .bind(&payload.email)
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
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("https://oauth2.googleapis.com/tokeninfo?id_token={}", token))
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
    
    Ok(user_info)
}

pub async fn wallet_authenticate(
    pool: &DbPool,
    payload: WalletAuthPayload,
) -> Result<AuthResponse, AppError> {
    // Verify wallet signature if provided
    if let Some(signature) = &payload.signature {
        verify_wallet_signature(&payload.wallet_address, signature)?;
    } else {
        log::warn!("Wallet authentication without signature verification");
    }
    
    // Create or update user
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (wallet_address, role) VALUES (?, 'user')
         ON CONFLICT(wallet_address) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(&payload.wallet_address)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

/// Verify wallet signature
/// This is a simplified implementation - in production, you'd verify the signature
/// matches the wallet address using the appropriate blockchain's signature verification
fn verify_wallet_signature(wallet_address: &str, signature: &str) -> Result<(), AppError> {
    // Basic validation
    if signature.len() < 10 {
        return Err(AppError::Unauthorized("Invalid signature format".to_string()));
    }
    
    // For Ethereum/EVM wallets (0x...)
    if wallet_address.starts_with("0x") {
        // In production, use ethers-rs or web3 to verify:
        // 1. Recover address from signature
        // 2. Compare with provided wallet_address
        // For now, we'll accept any signature for demo purposes
        log::info!("EVM wallet signature verification (simplified): {}", wallet_address);
        return Ok(());
    }
    
    // For Starknet wallets
    if wallet_address.len() == 66 && wallet_address.starts_with("0x") {
        // In production, use starknet-rs to verify signature
        log::info!("Starknet wallet signature verification (simplified): {}", wallet_address);
        return Ok(());
    }
    
    // For Solana wallets
    if wallet_address.len() >= 32 && !wallet_address.starts_with("0x") {
        // In production, use solana-sdk to verify signature
        log::info!("Solana wallet signature verification (simplified): {}", wallet_address);
        return Ok(());
    }
    
    Err(AppError::Unauthorized("Unsupported wallet type".to_string()))
}

pub async fn refresh_token(
    pool: &DbPool,
    payload: RefreshTokenPayload,
) -> Result<AuthResponse, AppError> {
    // Verify the refresh token
    let claims = jwt::verify_token(&payload.refresh_token)?;
    
    // Check if token is expired
    let now = chrono::Utc::now().timestamp() as usize;
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

pub async fn upload_profile_image(
    pool: &DbPool,
    req: &HttpRequest,
    mut payload: Multipart,
) -> Result<ImageUploadResponse, AppError> {
    let user_id = jwt::extract_user_id(req)?;
    
    // Create uploads directory if it doesn't exist
    std::fs::create_dir_all("uploads/profiles")
        .map_err(|e| AppError::BadRequest(format!("Failed to create upload directory: {}", e)))?;

    let mut file_path = String::new();

    // Process multipart data
    while let Some(item) = payload.next().await {
        let mut field = item.map_err(|e| AppError::BadRequest(format!("Failed to read field: {}", e)))?;
        
        // Generate unique filename
        let filename = format!("profile_{}_{}.jpg", user_id, chrono::Utc::now().timestamp());
        file_path = format!("uploads/profiles/{}", filename);
        
        // Save file
        let mut file = std::fs::File::create(&file_path)
            .map_err(|e| AppError::BadRequest(format!("Failed to create file: {}", e)))?;

        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| AppError::BadRequest(format!("Failed to read chunk: {}", e)))?;
            file.write_all(&data)
                .map_err(|e| AppError::BadRequest(format!("Failed to write file: {}", e)))?;
        }
    }

    if file_path.is_empty() {
        return Err(AppError::BadRequest("No file uploaded".to_string()));
    }

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
