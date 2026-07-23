use sqlx::{Error, PgPool};
use time::OffsetDateTime;
use tokio::time::{sleep, Duration};

const DISCORD_EPOCH: i64 = 1_420_070_400_000;

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

fn timestamp_to_snowflake(unix_seconds: i64) -> i64 {
    (unix_seconds * 1000 - DISCORD_EPOCH) << 22
}

pub async fn purge_thread(pool: PgPool, max_age_seconds: u32, seconds_interval: u32) {
    loop {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let snowflake_threshold =  timestamp_to_snowflake(now - max_age_seconds as i64);

        // Just in case discord snowflakes overflow (in 140 years) this will just stop purging instead of deleting everything
        if let Err(e) = sqlx::query("DELETE FROM messages WHERE id < $1 AND id > 0") 
            .bind(snowflake_threshold as i64)
            .execute(&pool)
            .await
        {
            log::error!("Failed to purge data from db: {}", e);
        }
        sleep(Duration::from_secs(seconds_interval as u64)).await;
    }
}

// Initialize database connection pool and run migrations
pub async fn initialize_database_pool(database_url: &str) -> Result<PgPool, Error> {
    log::trace!("Connecting to PostgreSQL database");

    let pool = connect_to_db_with_retry(database_url, 5).await?;

    log::trace!("Running database migrations");
    sqlx::migrate!().run(&pool).await?;

    log::info!("Database initialized successfully");
    Ok(pool)
}
