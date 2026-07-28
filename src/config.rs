use serde::Deserialize;
use serenity::all::ChannelId;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub token: String,
    pub join_leave_channel: ChannelId,
    pub deleted_msg_channel: ChannelId,
    pub edited_msg_distance: u32,
    pub bulk_delete_min_length: usize,
    pub bulk_delete_max_length: usize,
    pub purge_interval_hours: u32,
    pub purge_retention_days: u32,
    #[serde(default)]
    pub database_url: String,
}

impl Config {
    pub fn resolve_token(&self) -> String {
        let trimmed = self.token.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("DISCORD_TOKEN")
            .expect("No token in config.toml and the DISCORD_TOKEN env var is not set")
    }

    pub fn resolve_database_url(&self) -> String {
        let trimmed = self.database_url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://discord:discord@127.0.0.1:5432/discord".to_string())
    }
}