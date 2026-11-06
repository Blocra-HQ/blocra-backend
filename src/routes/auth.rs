use actix_web::{web, HttpResponse, Responder, HttpRequest};
use actix_multipart::Multipart;
use actix_web::ResponseError;
use crate::{auth::stellar_wallet, handlers::auth as auth_handler, db::DbPool};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/google", web::post().to(google_auth))
            .route("/wallet", web::post().to(wallet_auth_removed))
            .route("/stellar/challenge", web::post().to(stellar_challenge))
            .route("/stellar/verify", web::post().to(stellar_verify))
            .route("/refresh", web::post().to(refresh_token))
            .route("/me", web::get().to(get_current_user))
            .route("/profile", web::get().to(get_profile))
            .route("/profile", web::put().to(update_profile))
            .route("/upload-profile-image", web::post().to(upload_profile_image))
            .route("/logout", web::post().to(logout))
            .route("/password", web::put().to(change_password))
            .route("/account", web::delete().to(delete_account))
            .route("/oauth/config", web::get().to(get_oauth_config))
    );
}

async fn register(
    pool: web::Data<DbPool>,
    payload: web::Json<auth_handler::RegisterPayload>,
) -> impl Responder {
    match auth_handler::register(&pool, payload.into_inner()).await {
        Ok(response) => HttpResponse::Created().json(response),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn login(
    pool: web::Data<DbPool>,
    payload: web::Json<auth_handler::LoginPayload>,
) -> impl Responder {
    match auth_handler::login(&pool, payload.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn google_auth(
    pool: web::Data<DbPool>,
    payload: web::Json<auth_handler::GoogleAuthPayload>,
) -> impl Responder {
    match auth_handler::google_authenticate(&pool, payload.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

/// The old wallet login accepted any signature, so anyone could sign in as
/// any address. It is gone; clients use the Stellar challenge flow below.
async fn wallet_auth_removed() -> impl Responder {
    HttpResponse::Gone().json(serde_json::json!({
        "success": false,
        "message": "Wallet sign-in moved to POST /api/auth/stellar/challenge and /api/auth/stellar/verify"
    }))
}

/// Step 1: get a one-time challenge for a Stellar account.
async fn stellar_challenge(
    pool: web::Data<DbPool>,
    payload: web::Json<stellar_wallet::ChallengeRequest>,
) -> impl Responder {
    match stellar_wallet::create_challenge(&pool, &payload.account).await {
        Ok(challenge) => HttpResponse::Ok().json(challenge),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        })),
    }
}

/// Step 2: send the challenge back with the wallet's signature to get a JWT.
async fn stellar_verify(
    pool: web::Data<DbPool>,
    payload: web::Json<stellar_wallet::VerificationRequest>,
) -> impl Responder {
    let account = match stellar_wallet::verify_challenge(&pool, &payload).await {
        Ok(account) => account,
        Err(e) => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "success": false,
                "message": e.to_string()
            }))
        }
    };
    match auth_handler::stellar_sign_in(&pool, &account).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => e.error_response(),
    }
}

async fn refresh_token(
    pool: web::Data<DbPool>,
    payload: web::Json<auth_handler::RefreshTokenPayload>,
) -> impl Responder {
    match auth_handler::refresh_token(&pool, payload.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_current_user(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    match auth_handler::get_current_user(&pool, &req).await {
        Ok(user) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": { "user": user }
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn get_profile(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    match auth_handler::get_current_user(&pool, &req).await {
        Ok(user) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "data": { "user": user }
        })),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn update_profile(
    pool: web::Data<DbPool>,
    req: actix_web::HttpRequest,
    payload: web::Json<auth_handler::UpdateProfilePayload>,
) -> impl Responder {
    match auth_handler::update_profile(&pool, &req, payload.into_inner()).await {
        Ok(user) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Profile updated successfully",
            "data": { "user": user }
        })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}

async fn change_password(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    payload: web::Json<auth_handler::ChangePasswordPayload>,
) -> HttpResponse {
    match auth_handler::change_password(&pool, &req, payload.into_inner()).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "success": true, "message": "Password updated" })),
        Err(e) => e.error_response(),
    }
}

async fn delete_account(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    payload: Option<web::Json<auth_handler::DeleteAccountPayload>>,
) -> HttpResponse {
    let payload = payload.map(|p| p.into_inner()).unwrap_or(auth_handler::DeleteAccountPayload { password: None });
    match auth_handler::delete_account(&pool, &req, payload).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "success": true, "message": "Account deleted" })),
        Err(e) => e.error_response(),
    }
}

async fn logout() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "Logged out successfully"
    }))
}

async fn get_oauth_config() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "data": {
            "google": {
                "clientId": std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default()
            }
        }
    }))
}

async fn upload_profile_image(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    payload: Multipart,
) -> impl Responder {
    match auth_handler::upload_profile_image(&pool, &req, payload).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
    }
}
