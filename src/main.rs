mod config;
mod db;
mod models;
mod routes;
mod handlers;
mod middleware;
mod utils;
mod errors;
mod services;
mod auth;

use actix_web::{web, App, HttpServer, middleware::Logger};
use actix_cors::Cors;
use dotenv::dotenv;
use std::env;
use services::alchemy::AlchemyService;
use services::ai::AiService;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let config = config::Config::from_env();

    // Refuse to start without a real signing key rather than run with one
    // anyone can read in the source.
    if let Err(e) = utils::jwt::secret() {
        log::error!("❌ {e}");
        eprintln!("{e}");
        std::process::exit(1);
    }
    
    // Log configuration for debugging
    log::info!("🚀 Starting BlocRA Backend");
    log::info!("📊 Configuration:");
    log::info!("  - Database URL: {}", config.database_url);
    log::info!("  - Host: {}", config.host);
    log::info!("  - Port: {}", config.port);
    log::info!("  - CORS Origin: {}", config.cors_origin);
    
    // Check environment
    if env::var("RENDER").is_ok() {
        log::info!("🌐 Running on Render platform");
    }
    
    // Check /tmp directory
    if std::path::Path::new("/tmp").exists() {
        log::info!("✅ /tmp directory exists and is accessible");
    } else {
        log::error!("❌ /tmp directory does not exist!");
    }
    
    log::info!("🔌 Initializing database connection...");
    let db_pool = match db::init_pool(&config.database_url).await {
        Ok(pool) => {
            log::info!("✅ Database pool created successfully");
            pool
        }
        Err(e) => {
            log::error!("❌ Failed to create database pool: {:?}", e);
            log::error!("Database URL was: {}", config.database_url);
            panic!("Failed to create database pool: {:?}", e);
        }
    };

    // Run migrations
    log::info!("🔄 Running database migrations...");
    match db::run_migrations(&db_pool).await {
        Ok(_) => log::info!("✅ Migrations completed successfully"),
        Err(e) => {
            log::error!("❌ Failed to run migrations: {:?}", e);
            panic!("Failed to run migrations: {:?}", e);
        }
    }

    log::info!("✅ Database ready");
    
    // Initialize Alchemy service
    // Optional: powers POST /api/contract/transactions for EVM chains.
    let alchemy_api_key = env::var("ALCHEMY_API_KEY").unwrap_or_default();
    let alchemy_service = AlchemyService::new(alchemy_api_key);
    if alchemy_service.is_configured() {
        log::info!("✅ Alchemy service initialized");
    } else {
        log::warn!("ALCHEMY_API_KEY not set; /api/contract/transactions is disabled");
    }
    
    // Warn about ephemeral storage only on cloud platforms (Render, Heroku, etc.)
    if config.database_url.contains("/tmp/") && (env::var("RENDER").is_ok() || env::var("DYNO").is_ok()) {
        log::warn!("⚠️  WARNING: Using ephemeral storage (/tmp) on cloud platform");
        log::warn!("⚠️  ALL USER DATA WILL BE LOST on restart/redeploy!");
        log::warn!("⚠️  For production, use PostgreSQL or persistent disk");
        log::warn!("⚠️  See RENDER_DEPLOYMENT.md for details");
    }
    
    // Optional: the AI analyst (chat, reports, text-to-SQL). See services/ai.rs.
    let ai_service = AiService::from_env();
    if ai_service.is_configured() {
        log::info!("✅ AI analyst enabled (model {})", ai_service.model());
    } else {
        log::warn!("AI_BASE_URL not set; /api/ai/* will answer 503");
    }

    // Profile images are written under ./uploads; serve it even before the first upload.
    if let Err(e) = std::fs::create_dir_all("./uploads/profiles") {
        log::warn!("Could not create ./uploads: {e}");
    }

    let host = config.host.clone();
    let port = config.port;
    
    log::info!("🚀 Starting HTTP server on {}:{}", host, port);

    HttpServer::new(move || {
        let cors_origin = env::var("CORS_ORIGIN").unwrap_or_else(|_| "*".to_string());
        
        let cors = if cors_origin == "*" {
            Cors::default()
                .allow_any_origin()
                .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allowed_headers(vec![
                    actix_web::http::header::CONTENT_TYPE,
                    actix_web::http::header::AUTHORIZATION,
                ])
                .max_age(3600)
        } else {
            // Comma-separated list, e.g. https://blocra.app,https://www.blocra.app
            let mut cors = Cors::default();
            for origin in cors_origin.split(',').map(str::trim).filter(|o| !o.is_empty()) {
                cors = cors.allowed_origin(origin);
            }
            cors
                .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allowed_headers(vec![
                    actix_web::http::header::CONTENT_TYPE,
                    actix_web::http::header::AUTHORIZATION,
                ])
                .supports_credentials()
                .max_age(3600)
        };

        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(config.clone()))
            .app_data(web::Data::new(alchemy_service.clone()))
            .app_data(web::Data::new(ai_service.clone()))
            .wrap(cors)
            .wrap(Logger::default())
            .wrap(middleware::rate_limit::RateLimitMiddleware)
            // Serve uploaded files by exact path only; no directory listing.
            .service(actix_files::Files::new("/uploads", "./uploads"))
            .configure(routes::configure)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
