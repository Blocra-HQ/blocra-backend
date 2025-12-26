mod config;
mod db;
mod models;
mod routes;
mod handlers;
mod middleware;
mod utils;
mod errors;
mod services;

use actix_web::{web, App, HttpServer, middleware::Logger};
use actix_cors::Cors;
use dotenv::dotenv;
use std::env;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let config = config::Config::from_env();
    
    // Log configuration for debugging
    log::info!("Starting BlocRA Backend");
    log::info!("Database URL: {}", config.database_url);
    log::info!("Host: {}", config.host);
    log::info!("Port: {}", config.port);
    
    // Check if running on Render
    if env::var("RENDER").is_ok() {
        log::info!("Running on Render platform");
    }
    
    let db_pool = db::init_pool(&config.database_url)
        .await
        .expect("Failed to create database pool");

    // Run migrations
    db::run_migrations(&db_pool)
        .await
        .expect("Failed to run migrations");

    log::info!("✅ Connected to SQLite database");
    
    let host = config.host.clone();
    let port = config.port;
    
    log::info!("🚀 Starting server on {}:{}", host, port);

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
            Cors::default()
                .allowed_origin(&cors_origin)
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
            .wrap(cors)
            .wrap(Logger::default())
            .wrap(middleware::rate_limit::RateLimitMiddleware)
            .configure(routes::configure)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
