use sqlx::{Error, PgPool};

pub async fn connect_to_db_with_retry(
    database_url: &str,
    retry_interval_secs: u64,
) -> Result<PgPool, Error> {
    const MAX_RETRIES: u8 = 100;
    let mut retry_count = 0;

    loop {
        match PgPool::connect(database_url).await {
            Ok(pool) => {
                return Ok(pool);
            }
            Err(err) => {
                if retry_count >= MAX_RETRIES {
                    log::error!(
                        "Failed to connect to database after {} retries",
                        MAX_RETRIES
                    );
                    return Err(err);
                }
                log::error!(
                    "Failed to connect to database (attempt {}/{}): {}",
                    retry_count,
                    MAX_RETRIES,
                    err
                );
                log::info!("Retrying in {} seconds", retry_interval_secs);
                tokio::time::sleep(tokio::time::Duration::from_secs(retry_interval_secs)).await;
                retry_count += 1;
            }
        }
    }
}

// Initialize database connection pool and run migrations
pub async fn initialize_database_pool(
    database_url: &str,
) -> Result<PgPool, Error> {
    log::trace!("Connecting to PostgreSQL database");

    let pool = connect_to_db_with_retry(database_url, 5).await?;

    log::trace!("Running database migrations");
    sqlx::migrate!().run(&pool).await?;

    log::info!("Database initialized successfully");
    Ok(pool)
}
