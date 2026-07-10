use std::collections::{HashMap, HashSet};
use std::ops::Shr;
use std::sync::Arc;
use serde::Deserialize;
use serenity::all::{ChannelId, Colour, Context, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage, Guild, GuildId, InviteCreateEvent, InviteDeleteEvent, Member, Ready, RichInvite, User};
use serenity::Client;
use serenity::prelude::{EventHandler, GatewayIntents};
use sqlx::{PgPool, Row};
use time::{OffsetDateTime, UtcOffset};


mod db;

/// Convert a Discord snowflake id into the UTC timestamp at which the entity was created.
fn snowflake_to_timestamp(discord_id: u64) -> OffsetDateTime {
    let discord_epoch = OffsetDateTime::new_in_offset(
        time::Date::from_calendar_date(2015, time::Month::January, 1).unwrap(),
        time::Time::from_hms(0, 0, 0).unwrap(),
        UtcOffset::UTC,
    );
    // The upper 42 bits are a millisecond timestamp relative to the Discord epoch,
    // so shifting right by 22 bits yields the milliseconds since 2015-01-01.
    let millis_since_epoch = discord_id.shr(22) as i64;
    OffsetDateTime::from_unix_timestamp(discord_epoch.unix_timestamp() + millis_since_epoch / 1000).unwrap()
}


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

/// Everything we need to know about the invite a member used to join.
struct UsedInvite {
    code: String,
    inviter_id: u64,
    #[allow(dead_code)]
    inviter_name: String,
    created_at: i64,
}

fn build_join_message(new_member: &Member, join_amount: i32, last_known_join: i64, used_invite: Option<&UsedInvite>) -> CreateMessage {
    let user_id = new_member.user.id.get();
    let account_created = snowflake_to_timestamp(user_id).unix_timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();

    // Suspicious-join indicators. Each pushes a human-readable reason; if any are present the
    // embed is recoloured amber and a "Suspicious" field is added so it stands out in the log.
    const NEW_ACCOUNT_THRESHOLD_SECS: i64 = 24 * 60 * 60;
    let mut reasons: Vec<String> = Vec::new();
    if now - account_created < NEW_ACCOUNT_THRESHOLD_SECS {
        reasons.push(format!("Account younger than 24h (<t:{account_created}:R>)"));
    }
    if let Some(until) = new_member.unusual_dm_activity_until {
        if until.unix_timestamp() > now {
            let until_ts = until.unix_timestamp();
            reasons.push(format!("Unusual DM activity flagged (until <t:{until_ts}:F>)"));
        }
    }
    if new_member.user.avatar.is_none() {
        reasons.push("No avatar set".to_string());
    }
    if new_member.user.global_name.is_none() {
        reasons.push("No display name set".to_string());
    }
    let is_suspicious = !reasons.is_empty();

    // Only meaningful on a rejoin: on a first-time join prev_last_join == now, which would
    // misleadingly render as "just now".
    let last_known_join_line = if join_amount > 0 {
        format!("\n*Last known join <t:{last_known_join}:R>*")
    } else {
        String::new()
    };

    let invite_info = match used_invite {
        Some(inv) => format!(
            "**Code:** `{code}`\n\
             **Invited by:** <@{inviter_id}> ({inviter_id})\n\
             *Created <t:{invite_created}:R>*{last_known_join_line}",
            code = inv.code,
            inviter_id = inv.inviter_id,
            invite_created = inv.created_at,
            last_known_join_line = last_known_join_line,
        ),
        None => "*Could not determine which invite was used.*".to_string(),
    };

    // `<@id>` renders as a real, clickable user ping (right-click -> ban), not just plain text.
    let embed_description = format!(
        "<@{user_id}>\n\n\
         **Account created:**\n\
         <t:{account_created}:F>\n\
         *(<t:{account_created}:R> at time of joining)*\n\n\
         **Invite Info:**\n\
         {invite_info}",
    );

    let avatar_url = new_member.avatar_url().unwrap_or_else(|| new_member.user.face());
    let embed_author = CreateEmbedAuthor::new(&new_member.user.name).icon_url(&avatar_url);
    let embed_footer = CreateEmbedFooter::new(format!("JOINED {user_id}"));

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("MEMBER JOINED")
        .color(if is_suspicious {
            Colour::new(0xFFA500)
        } else {
            Colour::new(0x00FF00)
        })
        .description(embed_description)
        .thumbnail(&avatar_url)
        .field("Display Name", new_member.display_name().to_string(), true)
        .field("Username", new_member.user.name.clone(), true)
        .field("Rejoins", join_amount.to_string(), true);

    if is_suspicious {
        embed = embed.field("Suspicious", reasons.join("\n"), false);
    }

    let embed = embed.footer(embed_footer);

    CreateMessage::new().embed(embed)
}

fn build_leave_message(user: &User, last_join: Option<i64>) -> CreateMessage {
    let user_id = user.id.get();

    // Relative time from the last join renders as "x months ago", i.e. how long they were a member.
    let membership = match last_join {
        Some(ts) => format!("Joined <t:{ts}:F>\n*(<t:{ts}:R>)*"),
        None => "*Unknown - no join record found.*".to_string(),
    };

    let embed_description = format!(
        "<@{user_id}> ({user_id})\n\n\
         **Member since:**\n{membership}",
    );

    let avatar_url = user.face();
    let embed_author = CreateEmbedAuthor::new(&user.name).icon_url(&avatar_url);
    let embed_footer = CreateEmbedFooter::new(format!("LEFT {user_id}"));

    let embed = CreateEmbed::new()
        .author(embed_author)
        .title("MEMBER LEFT")
        .color(Colour::new(0xFF0000))
        .description(embed_description)
        .thumbnail(&avatar_url)
        .footer(embed_footer);

    CreateMessage::new().embed(embed)
}

fn build_invite_message(data: &InviteCreateEvent) -> CreateMessage {
    let (inviter_id, inviter_name, avatar_url) = match &data.inviter {
        Some(user) => (user.id.get(), user.name.clone(), Some(user.face())),
        None => (0, "unknown".to_string(), None),
    };

    let created = data.created_at.unix_timestamp();
    let expiry = if data.max_age == 0 {
        "**Expires:** Never".to_string()
    } else {
        let expires_at = created + data.max_age as i64;
        format!("**Expires:** <t:{expires_at}:F> (<t:{expires_at}:R>)")
    };

    let embed_description = format!(
        "<@{inviter_id}>\n\n\
         **Code:** `{code}`\n\
         **Created:** <t:{created}:F>\n\
         {expiry}",
        code = data.code,
    );

    let mut embed_author = CreateEmbedAuthor::new(&inviter_name);
    if let Some(url) = &avatar_url {
        embed_author = embed_author.icon_url(url);
    }
    let embed_footer = CreateEmbedFooter::new(format!("INV_CREATED {inviter_id}"));

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("INVITE CREATED")
        .color(Colour::new(0x00AAFF))
        .description(embed_description)
        .footer(embed_footer);
    if let Some(url) = &avatar_url {
        embed = embed.thumbnail(url);
    }

    CreateMessage::new().embed(embed)
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
        let msg = build_join_message(&new_member, join_amount, prev_last_join, used_invite.as_ref());
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

    async fn guild_member_removal(&self, ctx: Context, _guild_id: GuildId, user: User, _member: Option<Member>) {
        log::debug!("Member {} left", user.name);

        let last_join: Option<i64> = sqlx::query_scalar("SELECT last_join FROM joined_member WHERE user_id = $1")
            .bind(user.id.get() as i64)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);

        let channel = ChannelId::new(self.config.target_channel);
        let msg = build_leave_message(&user, last_join);
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
        let msg = build_invite_message(&data);
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

    let pool = db::initialize_database_pool(&database_url).await?;

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
