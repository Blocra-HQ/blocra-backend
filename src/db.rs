use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub type DbPool = SqlitePool;

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    // Extract the file path from the database URL
    let db_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
    
    // Create parent directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("Could not create database directory: {}", e);
        }
    }
    
    // Log the database path for debugging
    log::info!("Connecting to database at: {}", db_path);
    
    // Check if we can write to the directory
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        match std::fs::metadata(parent) {
            Ok(metadata) => {
                log::info!("Database directory exists, readonly: {}", metadata.permissions().readonly());
            }
            Err(e) => {
                log::warn!("Cannot access database directory: {}", e);
            }
        }
    }

    SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| sqlx::Error::Migrate(Box::new(e)))
}
