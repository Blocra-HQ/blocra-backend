use actix_web::{web, HttpResponse, Responder};
use crate::{handlers::auth as auth_handler, db::DbPool};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/google", web::post().to(google_auth))
            .route("/wallet", web::post().to(wallet_auth))
            .route("/refresh", web::post().to(refresh_token))
            .route("/me", web::get().to(get_current_user))
            .route("/profile", web::get().to(get_profile))
            .route("/profile", web::put().to(update_profile))
            .route("/logout", web::post().to(logout))
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

async fn wallet_auth(
    pool: web::Data<DbPool>,
    payload: web::Json<auth_handler::WalletAuthPayload>,
) -> impl Responder {
    match auth_handler::wallet_authenticate(&pool, payload.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::Unauthorized().json(serde_json::json!({
            "success": false,
            "message": e.to_string()
        }))
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
