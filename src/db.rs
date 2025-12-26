use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path;

pub type DbPool = SqlitePool;

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    // Extract the file path from the database URL
    let db_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    
    log::info!("Initializing database at: {}", db_path);
    
    // Ensure /tmp directory exists (it should always exist on Unix systems)
    if db_path.starts_with("/tmp") {
        log::info!("Using /tmp directory for database (cloud deployment)");
        // /tmp always exists on Unix, but let's verify
        if !Path::new("/tmp").exists() {
            log::error!("/tmp directory does not exist!");
            return Err(sqlx::Error::Configuration(
                "/tmp directory not found".into()
            ));
        }
    } else {
        // For local development, create the data directory
        if let Some(parent) = Path::new(db_path).parent() {
            if !parent.exists() {
                log::info!("Creating database directory: {:?}", parent);
                std::fs::create_dir_all(parent)
                    .map_err(|e| sqlx::Error::Configuration(
                        format!("Failed to create database directory: {}", e).into()
                    ))?;
            }
        }
    }

    log::info!("Connecting to SQLite database...");
    
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    
    log::info!("✅ Database connection established");
    
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| sqlx::Error::Migrate(Box::new(e)))
}
