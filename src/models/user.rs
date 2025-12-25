use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub wallet_address: Option<String>,
    pub email: Option<String>,
    pub username: Option<String>,
    pub role: String,
    pub google_id: Option<String>,
    pub profile_picture: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreateUser {
    pub wallet_address: Option<String>,
    pub email: Option<String>,
    pub username: Option<String>,
    pub google_id: Option<String>,
    pub profile_picture: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: i64,
    pub wallet_address: Option<String>,
    pub email: Option<String>,
    pub username: Option<String>,
    pub role: String,
    pub profile_picture: Option<String>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            wallet_address: user.wallet_address,
            email: user.email,
            username: user.username,
            role: user.role,
            profile_picture: user.profile_picture,
        }
    }
}
