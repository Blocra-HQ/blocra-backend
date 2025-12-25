use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub type DbPool = SqlitePool;

pub async fn init_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    // Create data directory if it doesn't exist
    if let Some(path_str) = database_url.strip_prefix("sqlite:") {
        if let Some(parent) = std::path::Path::new(path_str).parent() {
            std::fs::create_dir_all(parent).ok();
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
