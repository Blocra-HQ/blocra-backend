use crate::{db::DbPool, models::user::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use actix_multipart::Multipart;
use futures_util::StreamExt;
use std::io::Write;
use serde::{Deserialize, Serialize};
use bcrypt::{hash, verify, DEFAULT_COST};

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
    // TODO: Verify Google token
    // For now, create/get user
    
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (google_id, role) VALUES (?, 'user')
         ON CONFLICT(google_id) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(&payload.token)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
        user: user.into(),
    })
}

pub async fn wallet_authenticate(
    pool: &DbPool,
    payload: WalletAuthPayload,
) -> Result<AuthResponse, AppError> {
    // TODO: Verify wallet signature
    
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

pub async fn refresh_token(
    pool: &DbPool,
    payload: RefreshTokenPayload,
) -> Result<AuthResponse, AppError> {
    // TODO: Implement refresh token logic
    // For now, just verify and return new token
    let claims = jwt::verify_token(&payload.refresh_token)?;
    
    let user: User = sqlx::query_as(
        "SELECT * FROM users WHERE id = ?"
    )
    .bind(claims.sub)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(user.id)?;

    Ok(AuthResponse {
        success: true,
        token,
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
