use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::{
    RollingFileAppender,
    policy::compound::{
        CompoundPolicy, roll::fixed_window::FixedWindowRoller, trigger::size::SizeTrigger,
    },
};
use log4rs::config::{Appender, Config as LogConfig, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use serde::Deserialize;
use serenity::Client;
use serenity::all::{
    Attachment, ChannelId, Context, Guild, GuildId, InviteCreateEvent, InviteDeleteEvent, Member, Message, MessageId, MessageUpdateEvent, Ready, RichInvite, User, UserId,
};
use serenity::futures::StreamExt;
use serenity::prelude::{EventHandler, GatewayIntents};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use stringmetrics::levenshtein_limit;
use time::OffsetDateTime;

use discord_logging::datastructures::UsedInvite;
use discord_logging::{db::initialize_database_pool, messages};

#[derive(Deserialize, Clone, Debug)]
struct Config {
    #[serde(default)]
    token: String,
    join_leave_channel: ChannelId,
    deleted_msg_channel: ChannelId,
    edited_msg_distance: u32,
    bulk_delete_min_length: usize,
    bulk_delete_max_length: usize,
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
             ON CONFLICT (code) DO UPDATE SET uses = EXCLUDED.uses",
        )
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

    fn format_attachments (&self, attachments: Vec<Attachment>) -> Option<String>{
        let urls: Vec<&str> = attachments
            .iter()
            .filter(|attachment| {
                attachment
                    .content_type
                    .as_ref()
                    .map(|ct| ct.starts_with("image/"))
                    .unwrap_or(false)
            })
            .map(|attachment| attachment.proxy_url.as_str())
            .collect();
        
        if urls.is_empty() {
            None
        } else {
            Some(urls.join("\n"))
        }
    }

    async fn insert_members_batch(&self, batch: &[(i64, i64)]) {
        let mut query_builder =
            sqlx::QueryBuilder::new("INSERT INTO joined_member (last_join, user_id, join_amount) ");

        query_builder.push_values(batch, |mut b, (joined_at, user_id)| {
            b.push(joined_at).push(user_id).push(0);
        });

        query_builder.push(" ON CONFLICT (user_id) DO NOTHING");

        if let Err(e) = query_builder.build().execute(&self.pool).await {
            log::error!("Failed to insert user into database: {}", e);
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
        let stored_rows = sqlx::query(
            "SELECT code, uses, max_usages, inviter, created_at FROM invites WHERE guild = $1",
        )
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
        let mut members_iter = guild.id.members_iter(&ctx).boxed();

        const BATCH_SIZE: usize = 256;
        let mut batch = Vec::with_capacity(BATCH_SIZE);

        while let Some(member_result) = members_iter.next().await {
            match member_result {
                Ok(member) => {
                    // Only process members with a joined_at timestamp
                    if let Some(joined_at) = member.joined_at {
                        batch.push((joined_at.unix_timestamp(), member.user.id.get() as i64));

                        // If batch is full, execute the insert
                        if batch.len() >= BATCH_SIZE {
                            self.insert_members_batch(&batch).await;
                            batch.clear();
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error fetching member: {}", e);
                }
            }
        }

        if batch.len() > 0 {
            self.insert_members_batch(&batch).await;
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
             RETURNING id, last_join, join_amount",
        )
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

        if let Err(e) = sqlx::query("UPDATE joined_member SET last_join = $1 WHERE id = $2")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
        {
            log::error!("Failed to update last_join: {}", e);
        }

        let used_invite = self.sync_invites_find_used(&ctx, new_member.guild_id).await;

        let channel = self.config.join_leave_channel;
        let msg = messages::build_join_message(
            &new_member,
            join_amount,
            prev_last_join,
            used_invite.as_ref(),
        );
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!("Unable to send join message to channel {}: {}", channel, e);
        }
    }

    async fn guild_member_removal(
        &self,
        ctx: Context,
        _guild_id: GuildId,
        user: User,
        _member: Option<Member>,
    ) {
        log::debug!("Member {} left", user.name);

        let last_join: Option<i64> =
            sqlx::query_scalar("SELECT last_join FROM joined_member WHERE user_id = $1")
                .bind(user.id.get() as i64)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);

        let channel = self.config.join_leave_channel;
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
             ON CONFLICT (code) DO UPDATE SET uses = EXCLUDED.uses",
        )
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

        let channel = self.config.join_leave_channel;
        let msg = messages::build_invite_message(&data);
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!(
                "Unable to send invite message to channel {}: {}",
                channel,
                e
            );
        }
    }

    async fn invite_delete(&self, _ctx: Context, data: InviteDeleteEvent) {
        // Intentionally do NOT delete the row here: this event races with `guild_member_addition`,
        // and a single-use invite that was just consumed needs to survive in the DB long enough for
        // the join handler to attribute it. Stale rows are pruned during the next join's sync.
        log::debug!("Invite {} deleted", data.code);
    }

    async fn message(&self, _ctx: Context, message: Message) {
        let attachments_string = self.format_attachments(message.attachments);
            
        if let Err(e) = sqlx::query(
            "INSERT INTO messages (id, user_id, message, embeds) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(message.id.get() as i64)
        .bind(message.author.id.get() as i64)
        .bind(message.content)
        .bind(attachments_string)
        .execute(&self.pool)
        .await
        {
            log::error!("Failed to store message: {}", e);
        }
    }

    async fn message_update(
        &self,
        ctx: Context,
        _old: Option<Message>,
        _new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        let Some(guild) = event.guild_id else {
            return;
        };

        if let Some(author) = &event.author && author.bot {
            return;
        }

        let result = sqlx::query(
            "UPDATE messages SET \
                message = COALESCE($2, message), \
                edits = edits + 1 \
            WHERE id = $1 \
            RETURNING \
                user_id, OLD.message, edits",
        )
        .bind(event.id.get() as i64)
        .bind(&event.content)
        .fetch_optional(&self.pool)
        .await;

        let result = match result {
            Ok(Some(row)) => row,
            Ok(None) => {
                    let attachments_string = event.attachments.map(|attachments| {
                        self.format_attachments(attachments)
                }).flatten();
                // If we have the author, store it as a new message
                if let Some(author) = event.author {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO messages (id, user_id, message, embeds, edits) \
                    VALUES ($1, $2, $3, $4, 1)",
                    )
                    .bind(event.id.get() as i64)
                    .bind(author.id.get() as i64)
                    .bind(&event.content)
                    .bind(&attachments_string)
                    .execute(&self.pool)
                    .await
                        {
                            log::error!("Failed to store message: {}", e);
                        }
                }
                return;
            }
            Err(e) => {
                log::error!("Failed to update message edit: {}", e);
                return;
            }
        };

        let user_id: i64 = result.get(0);
        let old_message: Option<String> = result.get(1);
        let edits: i32 = result.get(2);

        if let Some(new_message) = event.content && let Some(old_message) = old_message {
            if old_message.is_empty() {
                // If the message has no content there's no point in logging it
                return;
            }
            let similarity = levenshtein_limit(new_message.as_str(), old_message.as_str(), self.config.edited_msg_distance);

            if similarity < self.config.edited_msg_distance{
                return;
            }

            let channel = self.config.deleted_msg_channel;
            let msg = messages::build_edited_message(
                    event.author, 
                    Some(UserId::new(user_id as u64)),
                    event.channel_id.to_channel(&ctx).await.ok(),
                    event.channel_id,
                    guild,
                    event.id,
                    old_message,
                    edits
                );

            if let Err(e) = channel.send_message(&ctx, msg).await {
                log::error!(
                    "Unable to send edited message to channel {}: {}",
                    channel,
                    e
                );
            }
        }
    }


async fn message_delete (
    &self,
    ctx: Context,
    channel_id: ChannelId,
    deleted_message_id: MessageId,
    guild_id: Option<GuildId>,
    ) {
        let Some(guild_id) = guild_id else {
            // Ignore DMs
            return;
        };

        let row = sqlx::query(
            "SELECT user_id, message, embeds, edits \
             FROM messages \
             WHERE id = $1",
        )
        .bind(deleted_message_id.get() as i64)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("Failed to fetch message: {}", e);
            None
        });

        let (user_id, content, attachments, edits) = row
            .as_ref()
            .map(|r| (
                r.get::<i64, _>(0),
                r.get::<Option<String>, _>(1),
                r.get::<Option<String>, _>(2),
                r.get::<i32, _>(3),
            ))
            .unwrap_or((0, None, None, 0));

            
        let user_id = if user_id != 0 {
            Some(UserId::new(user_id as u64))
        } else {
            None
        };


        let user = match user_id {
            Some(user_id) => user_id.to_user(&ctx).await.ok(),
            None => None,
        };

        if let Some(user) = &user && user.bot {
            return;
        }

        let channel = self.config.deleted_msg_channel;
        let msg = messages::build_deleted_message(
                user, 
                user_id,
                channel_id.to_channel(&ctx).await.ok(),
                channel_id,
                guild_id,
                deleted_message_id,
                content,
                attachments,
                edits
            );
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!(
                "Unable to send deleted message to channel {}: {}",
                channel,
                e
            );
        }
    }

    async fn message_delete_bulk(
        &self,
        ctx: Context,
        channel_id: ChannelId,
        deleted_messages_ids: Vec<MessageId>,
        guild_id: Option<GuildId>,
    ) {
        if guild_id.is_none() {
            // Ignore DMs
            return;
        };
        if deleted_messages_ids.is_empty() {
            return;
        }

        let ids: Vec<i64> = deleted_messages_ids
            .iter()
            .map(|id| id.get() as i64)
            .collect();

        // Use ANY() with a single array parameter
        let rows = sqlx::query_as::<_, (i64, String)>(
            "SELECT DISTINCT user_id, message \
            FROM messages \
            WHERE id = ANY($1)"
        )
        .bind(&ids) 
        .fetch_all(&self.pool)
        .await
        .unwrap_or_else(|e| {
            log::error!("Failed to fetch messages: {}", e);
            vec![]
        });

        // group by user first
        let mut messages_by_user: HashMap<UserId, Vec<String>> = HashMap::new();
        for (id, msg) in rows {
            let user_messages = messages_by_user
                .entry(UserId::new(id as u64))
                .or_insert_with(Vec::new);

            if msg.len() < self.config.bulk_delete_min_length {
                continue;
            }

            let msg = msg.replace('\n', " ");
            let msg = if msg.len() > self.config.bulk_delete_max_length {
                format!("{}...", msg[..self.config.bulk_delete_max_length].to_string())
            } else {
                msg
            };

            user_messages.push(msg);
        }


        let mut messages_with_user = Vec::new();
        
        for (user_id, messages) in messages_by_user {
            let user = user_id.to_user(&ctx).await.ok();
            messages_with_user.push((user_id, user, messages));
        }

        let channel = self.config.deleted_msg_channel;
        let msg = messages::build_bulk_delete_message(
                messages_with_user,
                channel_id.to_channel(&ctx).await.ok(),
                channel_id,
                deleted_messages_ids.len()
            );
        if let Err(e) = channel.send_message(&ctx, msg).await {
            log::error!(
                "Unable to send deleted message to channel {}: {}",
                channel,
                e
            );
        }

    }


    async fn ready(&self, _ctx: Context, _ready: Ready) {
        log::info!("Bot is online and watching for invites coming in");
    }
}

fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    let pattern = "{d(%Y-%m-%d %H:%M:%S)} {l:5} - {m}{n}";

    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(pattern)))
        .build();

    let roller = FixedWindowRoller::builder().build("logs/bot.{}.log", 5)?;
    let trigger = SizeTrigger::new(10 * 1024 * 1024);
    let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));

    let file = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(pattern)))
        .build("logs/bot.log", Box::new(policy));

    let mut config_builder = LogConfig::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .logger(Logger::builder().build("tracing", LevelFilter::Off));

    let mut root_builder = Root::builder().appender("stdout");

    // In debug mode, only use stdout and show debug messages. In release mode
    // also attach the file appender, but fall back to stdout-only if the logs/
    // directory is not writable rather than crashing.
    let level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        match file {
            Ok(file) => {
                config_builder =
                    config_builder.appender(Appender::builder().build("file", Box::new(file)));
                root_builder = root_builder.appender("file");
                LevelFilter::Warn
            }
            Err(e) => {
                eprintln!(
                    "WARNING: file logger could not be initialised (logs/ not writable?), \
                     continuing with stdout only: {e}"
                );
                LevelFilter::Warn
            }
        }
    };

    let root = root_builder.build(level);
    let config = config_builder.build(root)?;

    log4rs::init_config(config)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging()?;

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
        | GatewayIntents::GUILD_INVITES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGES;

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;
    Ok(())
}
