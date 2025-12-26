use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path;

pub type DbPool = SqlitePool;

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    // Extract the file path from the database URL
    let db_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    
    log::info!("Initializing database at: {}", db_path);
    log::info!("Full database URL: {}", database_url);
    
    // Ensure parent directory exists and create empty database file if needed
    if let Some(parent) = Path::new(db_path).parent() {
        log::info!("Database parent directory: {:?}", parent);
        if !parent.exists() {
            log::info!("Creating database directory: {:?}", parent);
            match std::fs::create_dir_all(parent) {
                Ok(_) => log::info!("✅ Directory created successfully"),
                Err(e) => {
                    log::error!("❌ Failed to create directory: {}", e);
                    return Err(sqlx::Error::Configuration(
                        format!("Failed to create database directory: {}", e).into()
                    ));
                }
            }
        } else {
            log::info!("✅ Database directory already exists");
        }
    }
    
    // Create empty database file if it doesn't exist (required for SQLite)
    if !Path::new(db_path).exists() {
        log::info!("Creating empty database file: {}", db_path);
        match std::fs::File::create(db_path) {
            Ok(_) => log::info!("✅ Database file created successfully"),
            Err(e) => {
                log::error!("❌ Failed to create database file: {}", e);
                return Err(sqlx::Error::Configuration(
                    format!("Failed to create database file: {}", e).into()
                ));
            }
        }
    } else {
        log::info!("✅ Database file already exists");
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
