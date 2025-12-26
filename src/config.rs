use std::env;

#[derive(Clone)]
#[allow(dead_code)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiration: i64,
    pub cors_origin: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub rate_limit_max: usize,
    pub rate_limit_window: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "5000".to_string())
                .parse()
                .expect("PORT must be a number"),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| {
                    // Always use /tmp for cloud deployments (Render, Railway, etc.)
                    // /tmp is the only writable directory on most cloud platforms
                    "sqlite:/tmp/blocra.db".to_string()
                }),
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| {
                    log::warn!("JWT_SECRET not set, using default (NOT SECURE FOR PRODUCTION)");
                    "default_jwt_secret_change_in_production_12345678901234567890".to_string()
                }),
            jwt_expiration: env::var("JWT_EXPIRATION")
                .unwrap_or_else(|_| "86400".to_string())
                .parse()
                .unwrap_or(86400),
            cors_origin: env::var("CORS_ORIGIN")
                .unwrap_or_else(|_| "*".to_string()),
            google_client_id: env::var("GOOGLE_CLIENT_ID")
                .unwrap_or_default(),
            google_client_secret: env::var("GOOGLE_CLIENT_SECRET")
                .unwrap_or_default(),
            rate_limit_max: env::var("RATE_LIMIT_MAX_REQUESTS")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),
            rate_limit_window: env::var("RATE_LIMIT_WINDOW_SECS")
                .unwrap_or_else(|_| "900".to_string())
                .parse()
                .unwrap_or(900),
        }
    }
}
