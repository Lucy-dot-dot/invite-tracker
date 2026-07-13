use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use serde::Deserialize;
use serenity::all::{ChannelId, Context, Guild, GuildId, InviteCreateEvent, InviteDeleteEvent, Member, Ready, RichInvite, User};
use serenity::Client;
use serenity::prelude::{EventHandler, GatewayIntents};
use sqlx::{PgPool, Row};
use time::{OffsetDateTime};

use discord_logging::datastructures::UsedInvite;
use discord_logging::{messages, db::initialize_database_pool};

#[derive(Deserialize, Clone, Debug)]
struct Config {
    #[serde(default)]
    token: String,
    target_channel: u64,
    #[serde(default)]
    database_url: String,
}

impl Config {
    fn resolve_token(&self) -> String {
        let trimmed = self.token.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("DISCORD_TOKEN")
            .expect("No token in config.toml and the DISCORD_TOKEN env var is not set")
    }

    fn resolve_database_url(&self) -> String {
        let trimmed = self.database_url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://discord:discord@127.0.0.1:5432/discord".to_string())
    }
}

struct Handler {
    config: Arc<Config>,
    pool: PgPool,
}

impl Handler {
    /// Store or refresh a single invite (its current use count in particular) in the database.
    async fn upsert_invite(&self, guild_id: i64, invite: &RichInvite) {
        let inviter_id = invite.inviter.as_ref().map(|u| u.id.get()).unwrap_or(0) as i64;
        if let Err(e) = sqlx::query(
            "INSERT INTO invites (created_at, guild, inviter, max_age, max_usages, code, uses) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (code) DO UPDATE SET uses = EXCLUDED.uses")
            .bind(invite.created_at.unix_timestamp())
            .bind(guild_id)
            .bind(inviter_id)
            .bind(invite.max_age as i32)
            .bind(invite.max_uses as i32)
            .bind(&invite.code)
            .bind(invite.uses as i64)
            .execute(&self.pool)
            .await
        {
            log::error!("Failed to upsert invite {}: {}", invite.code, e);
        }
    }

    /// Re-fetch the guild's invites and compare their use counts against what we last stored to
    /// figure out which invite the joining member used. Also refreshes the stored counts and
    /// prunes invites that no longer exist.
    async fn sync_invites_find_used(&self, ctx: &Context, guild_id: GuildId) -> Option<UsedInvite> {
        let guild_key = guild_id.get() as i64;

        let current = match guild_id.invites(ctx).await {
            Ok(invites) => invites,
            Err(e) => {
                log::error!("Failed to fetch invites for guild {}: {}", guild_id, e);
                return None;
            }
        };

        // Load what we previously stored for this guild: code -> (uses, max_usages, inviter, created_at).
        let stored_rows = sqlx::query("SELECT code, uses, max_usages, inviter, created_at FROM invites WHERE guild = $1")
            .bind(guild_key)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        let mut stored: HashMap<String, (i64, i32, i64, i64)> = HashMap::new();
        for row in &stored_rows {
            let code: String = row.get(0);
            stored.insert(code, (row.get(1), row.get(2), row.get(3), row.get(4)));
        }

        let current_codes: HashSet<&str> = current.iter().map(|i| i.code.as_str()).collect();

        // Primary signal: an invite that still exists had its use count go up.
        let mut used: Option<UsedInvite> = None;
        for invite in &current {
            let prev_uses = stored.get(&invite.code).map(|s| s.0).unwrap_or(0);
            if invite.uses as i64 > prev_uses {
                used = Some(UsedInvite {
                    code: invite.code.clone(),
                    uses: invite.uses,
                    inviter_id: invite.inviter.as_ref().map(|u| u.id.get()).unwrap_or(0),
                    inviter_name: invite
                        .inviter
                        .as_ref()
                        .map(|u| u.name.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    created_at: invite.created_at.unix_timestamp(),
                });
                break;
            }
        }

        // Fallback: a limited-use invite that vanished since we last saw it was almost certainly
        // consumed on this join - Discord deletes single/last-use invites once they are exhausted,
        // so they never show up in the live fetch above.
        if used.is_none() {
            for (code, (uses, max_usages, inviter, created_at)) in &stored {
                let vanished = !current_codes.contains(code.as_str());
                let exhausted = *max_usages > 0 && *uses >= *max_usages as i64 - 1;
                if vanished && exhausted {
                    used = Some(UsedInvite {
                        code: code.clone(),
                        uses: *uses as u64,
                        inviter_id: *inviter as u64,
                        inviter_name: "unknown".to_string(),
                        created_at: *created_at,
                    });
                    break;
                }
            }
        }

        // Refresh stored state: upsert everything still live, then drop rows for invites that no
        // longer exist so a stale entry can never produce a false match on a later join.
        for invite in &current {
            self.upsert_invite(guild_key, invite).await;
        }
        for code in stored.keys() {
            if !current_codes.contains(code.as_str()) {
                if let Err(e) = sqlx::query("DELETE FROM invites WHERE guild = $1 AND code = $2")
                    .bind(guild_key)
                    .bind(code)
                    .execute(&self.pool)
                    .await
                {
                    log::error!("Failed to prune vanished invite {}: {}", code, e);
                }
            }
        }

        used
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        for (&user_id, member) in &guild.members {
            // joined_at is optional in Discord's API and isn't guaranteed to be populated, so skip
            // members we can't date rather than inserting a meaningless zero/null.
            let Some(joined_at) = member.joined_at else { continue };
            if let Err(e) = sqlx::query(
                "INSERT INTO joined_member (last_join, user_id, join_amount) VALUES ($1, $2, 0) \
                 ON CONFLICT (user_id) DO NOTHING")
                .bind(joined_at.unix_timestamp())
                .bind(user_id.get() as i64)
                .execute(&self.pool)
                .await
            {
                log::error!("Failed to backfill member {}: {}", user_id, e);
            }
        }

        if is_new.unwrap_or(false) {
            log::debug!("Guild {} is connected", guild.name);
            match guild.invites(&ctx).await {
                Ok(existing) => {
                    for invite in existing.iter() {
                        self.upsert_invite(guild.id.get() as i64, invite).await;
                    }
                    log::debug!("Guild is now ready to go");
                }
                Err(e) => log::error!("Failed to fetch invites for guild {}: {}", guild.id, e),
            }
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        log::debug!("Member {} joined", new_member.user.name);
        let now = OffsetDateTime::now_utc().unix_timestamp();

        // Upsert the member. On a rejoin we bump join_amount but keep the previous last_join value
        // in the RETURNING row so we can update it to "now" afterwards.
        let result = sqlx::query(
            "INSERT INTO joined_member (last_join, user_id, join_amount) VALUES ($1, $2, 0) \
             ON CONFLICT (user_id) DO UPDATE SET join_amount = joined_member.join_amount + 1 \
             RETURNING id, last_join, join_amount")
            .bind(now)
            .bind(new_member.user.id.get() as i64)
            .fetch_one(&self.pool)
            .await;
        let result = match result {
            Ok(row) => row,
            Err(e) => {
                log::error!("Failed to record member join: {}", e);
                return;
            }
        };
        let id: sqlx::types::Uuid = result.get(0);
        let prev_last_join: i64 = result.get(1);
        let join_amount: i32 = result.get(2);

        let used_invite = self.sync_invites_find_used(&ctx, new_member.guild_id).await;

        let channel = ChannelId::new(self.config.target_channel);
        let msg = messages::build_join_message(&new_member, join_amount, prev_last_join, used_invite.as_ref());
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!("Unable to send join message to channel {}: {}", channel, e);
        }

        if let Err(e) = sqlx::query("UPDATE joined_member SET last_join = $1 WHERE id = $2")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
        {
            log::error!("Failed to update last_join: {}", e);
        }
    }

    async fn guild_member_removal(&self, ctx: Context, _guild_id: GuildId, user: User, member: Option<Member>) {
        log::debug!("Member {} left", user.name);

        let mut last_join: Option<i64> = sqlx::query_scalar("SELECT last_join FROM joined_member WHERE user_id = $1")
            .bind(user.id.get() as i64)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);

        if last_join.is_none() &&
          let Some(member) = member &&
          let Some(joined_at) = member.joined_at {
            if let Err(e) = sqlx::query(
                "INSERT INTO joined_member (last_join, user_id, join_amount) VALUES ($1, $2, 0)")
                .bind(joined_at.unix_timestamp())
                .bind(user.id.get() as i64)
                .fetch_one(&self.pool)
                .await
            {
                log::error!("Failed to insert user into database: {}", e);
            }
            last_join = Some(joined_at.unix_timestamp());
        }

        let channel = ChannelId::new(self.config.target_channel);
        let msg = messages::build_leave_message(&user, last_join);
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!("Unable to send leave message to channel {}: {}", channel, e);
        }
    }

    async fn invite_create(&self, ctx: Context, data: InviteCreateEvent) {
        let (id, name) = data
            .inviter
            .as_ref()
            .map(|user| (user.id.get(), user.name.clone()))
            .unwrap_or((0, "unknown".to_string()));
        log::debug!("Invite {} created by {} ({})", data.code, name, id);

        let guild_id = match data.guild_id {
            Some(g) => g,
            None => {
                log::error!("Invite create event without a guild id");
                return;
            }
        };

        if let Err(e) = sqlx::query(
            "INSERT INTO invites (created_at, guild, inviter, max_age, max_usages, code, uses) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (code) DO UPDATE SET uses = EXCLUDED.uses")
            .bind(data.created_at.unix_timestamp())
            .bind(guild_id.get() as i64)
            .bind(id as i64)
            .bind(data.max_age as i32)
            .bind(data.max_uses as i32)
            .bind(&data.code)
            .bind(data.uses as i64)
            .execute(&self.pool)
            .await
        {
            log::error!("Failed to insert invite {}: {}", data.code, e);
        }

        let channel = ChannelId::new(self.config.target_channel);
        let msg = messages::build_invite_message(&data);
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!("Unable to send invite message to channel {}: {}", channel, e);
        }
    }

    async fn invite_delete(&self, _ctx: Context, data: InviteDeleteEvent) {
        // Intentionally do NOT delete the row here: this event races with `guild_member_addition`,
        // and a single-use invite that was just consumed needs to survive in the DB long enough for
        // the join handler to attribute it. Stale rows are pruned during the next join's sync.
        log::debug!("Invite {} deleted", data.code);
    }

    async fn ready(&self, _ctx: Context, _ready: Ready) {
        log::info!("Bot is online and watching for invites coming in");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_module("tracing", log::LevelFilter::Off).init();

    let raw = std::fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&raw)?;
    let token = config.resolve_token();
    let database_url = config.resolve_database_url();

    let pool = initialize_database_pool(&database_url).await?;

    let handler = Handler {
        config: Arc::new(config),
        pool,
    };

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_INVITES;

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;
    Ok(())
}
