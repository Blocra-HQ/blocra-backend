use crate::{db::DbPool, models::user::*, utils::jwt, errors::AppError};
use actix_web::HttpRequest;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    let user_id = Uuid::new_v4().to_string();
    
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

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, username, role) VALUES (?, ?, ?, 'user')
         RETURNING *"
    )
    .bind(&user_id)
    .bind(&payload.email)
    .bind(payload.username)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(&user.id)?;

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

    // TODO: Verify password with bcrypt
    // For now, just return token

    let token = jwt::create_token(&user.id)?;

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
    let user_id = Uuid::new_v4().to_string();
    
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, google_id, role) VALUES (?, ?, 'user')
         ON CONFLICT(google_id) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(&user_id)
    .bind(&payload.token)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(&user.id)?;

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
    
    let user_id = Uuid::new_v4().to_string();
    
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, wallet_address, role) VALUES (?, ?, 'user')
         ON CONFLICT(wallet_address) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
         RETURNING *"
    )
    .bind(&user_id)
    .bind(&payload.wallet_address)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(&user.id)?;

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
    .bind(&claims.sub)
    .fetch_one(pool)
    .await?;

    let token = jwt::create_token(&user.id)?;

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
    .bind(&user_id)
    .fetch_one(pool)
    .await?;

    Ok(user.into())
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
    
    q = q.bind(&user_id);

    let user = q.fetch_one(pool).await?;

    Ok(user.into())
}
