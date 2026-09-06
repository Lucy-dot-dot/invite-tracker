use discord_logging::audit_log::{
    get_ban_or_kick_event, get_message_deleted_entry, init_audit_log,
};
use discord_logging::config::Config;
use discord_logging::db::purge_thread;
use discord_logging::messages;
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
use serenity::Client;
use serenity::all::audit_log::Action;
use serenity::all::{
    ChannelId, Context, CreateMessage, FullEvent, GuildId, Invite, MemberAction, Message, UserId,
};
use serenity::futures::StreamExt;
use serenity::prelude::{EventHandler, GatewayIntents};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use stringmetrics::levenshtein_limit;
use time::OffsetDateTime;
use tokio::time::sleep;

use discord_logging::datastructures::UsedInvite;
use discord_logging::db::initialize_database_pool;

pub struct Handler {
    pub config: Arc<Config>,
    pub pool: PgPool,
}

const MSG_RETRY_INTERVAL: Duration = Duration::from_millis(200);

pub async fn send_message(message: CreateMessage<'static>, ctx: &Context, channel_id: ChannelId) {
    if let Err(_) = channel_id
        .widen()
        .send_message(&ctx.http, message.clone())
        .await
    {
        sleep(MSG_RETRY_INTERVAL).await;

        if let Err(e) = channel_id.widen().send_message(&ctx.http, message).await {
            log::error!(
                "Unable to send message to channel {} after retry: {}",
                channel_id,
                e
            );
        }
    }
}

impl Handler {
    /// Store or refresh a single invite (its current use count in particular) in the database.
    async fn upsert_invite(&self, guild_id: i64, invite: &Invite) {
        let inviter_id = invite.inviter.as_ref().map(|u| u.id.get()).unwrap_or(0) as i64;
        let (uses, max_uses, max_age, created_at) = match &invite.metadata {
            Some(metadata) => (
                metadata.uses,
                metadata.max_uses,
                metadata.max_age,
                metadata.created_at.unix_timestamp(),
            ),
            None => (0, 0, 0, 0),
        };
        if let Err(e) = sqlx::query(
            "INSERT INTO invites (created_at, guild, inviter, max_age, max_usages, code, uses) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (code) DO UPDATE SET uses = EXCLUDED.uses",
        )
        .bind(created_at)
        .bind(guild_id)
        .bind(inviter_id)
        .bind(max_age as i32)
        .bind(max_uses as i32)
        .bind(invite.code.as_str())
        .bind(uses as i64)
        .execute(&self.pool)
        .await
        {
            log::error!("Failed to upsert invite {}: {}", invite.code, e);
        }
    }

    fn format_stickers(&self, message: &Message) -> Option<String> {
        // Actual stickers
        let mut urls: Vec<String> = message
            .sticker_items
            .iter()
            .filter_map(|sticker| sticker.image_url())
            .collect();

        // GIFs, only get the thumbnail url as it's static and can't be in any weird formats
        urls.extend(
            message
                .embeds
                .iter()
                .filter_map(|embed| embed.thumbnail.as_ref())
                .map(|thumbnail| thumbnail.url.to_string()),
        );

        if urls.is_empty() {
            None
        } else {
            Some(urls.join("\n"))
        }
    }

    fn format_attachments(&self, message: &Message) -> Option<String> {
        let urls: Vec<String> = message
            .attachments
            .iter()
            .filter(|attachement| attachement.size < self.config.max_upload_size)
            .map(|attachment| format!("{}|{}", attachment.proxy_url, attachment.filename))
            .collect();

        if urls.is_empty() {
            return None;
        }
        Some(urls.join("\n"))
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

        let current = match guild_id.invites(&ctx.http).await {
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
            let current_uses = invite.metadata.as_ref().map(|m| m.uses).unwrap_or(0);
            let prev_uses = stored.get(invite.code.as_str()).map(|s| s.0).unwrap_or(0);
            if current_uses as i64 > prev_uses {
                used = Some(UsedInvite {
                    code: invite.code.to_string(),
                    uses: current_uses,
                    inviter_id: invite.inviter.as_ref().map(|u| u.id.get()).unwrap_or(0),
                    inviter_name: invite
                        .inviter
                        .as_ref()
                        .map(|u| u.name.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    created_at: invite
                        .metadata
                        .as_ref()
                        .map(|m| m.created_at.unix_timestamp())
                        .unwrap_or(0),
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
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) {
        match event {
            FullEvent::GuildCreate {
                guild, is_new: _, ..
            } => {
                // initialize members
                let mut members_iter = guild.id.members_iter(&ctx.http).boxed();

                const BATCH_SIZE: usize = 256;
                let mut batch = Vec::with_capacity(BATCH_SIZE);

                while let Some(member_result) = members_iter.next().await {
                    match member_result {
                        Ok(member) => {
                            // Only process members with a joined_at timestamp
                            if let Some(joined_at) = member.joined_at {
                                batch.push((
                                    joined_at.unix_timestamp(),
                                    member.user.id.get() as i64,
                                ));

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

                // initialize invites
                match guild.id.invites(&ctx.http).await {
                    Ok(existing) => {
                        for invite in existing.iter() {
                            self.upsert_invite(guild.id.get() as i64, invite).await;
                        }
                    }
                    Err(e) => log::error!("Failed to fetch invites for guild {}: {}", guild.id, e),
                }

                // initialize audit logs
                init_audit_log(guild.id, &ctx, &self.pool).await;
            }
            FullEvent::GuildMemberAddition { new_member, .. } => {
                let now = new_member
                    .joined_at
                    .map(|ts| ts.unix_timestamp())
                    .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp());

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

                let msg = messages::invites::build_join_message(
                    &new_member,
                    join_amount,
                    prev_last_join,
                    used_invite.as_ref(),
                );
                send_message(msg, &ctx, self.config.join_leave_channel).await;
            }
            FullEvent::GuildMemberRemoval {
                guild_id,
                user,
                member_data_if_available: _,
                ..
            } => {
                log::debug!("Member {} left", user.name);

                let result = sqlx::query(
                    "SELECT last_join, join_amount FROM joined_member WHERE user_id = $1",
                )
                .bind(user.id.get() as i64)
                .fetch_optional(&self.pool)
                .await;

                let (last_join, join_amount) = match result {
                    Ok(Some(row)) => (row.get::<Option<i64>, _>(0), row.get::<Option<i32>, _>(1)),
                    Ok(None) => (None, None),
                    Err(e) => {
                        log::error!("Failed to read member join record: {}", e);
                        (None, None)
                    }
                };

                let result: Result<i64, sqlx::Error> =
                    sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE user_id = $1")
                        .bind(user.id.get() as i64)
                        .fetch_one(&self.pool)
                        .await;

                let message_count = match result {
                    Ok(value) => Some(value),
                    Err(e) => {
                        log::error!("Failed to read member message count: {}", e);
                        None
                    }
                };

                let entry = get_ban_or_kick_event(guild_id, &user.id, &ctx, &self.pool).await;

                let admin = match &entry {
                    Some(entry) => match entry.user_id {
                        Some(admin_id) => admin_id.to_user(&ctx).await.ok(),
                        None => None,
                    },
                    None => None,
                };

                let msg = messages::invites::build_leave_message(
                    &user,
                    last_join,
                    join_amount,
                    message_count,
                    admin,
                    entry,
                );
                send_message(msg, &ctx, self.config.join_leave_channel).await;
            }
            FullEvent::InviteCreate { data, .. } => {
                let (id, name) = data
                    .inviter
                    .as_ref()
                    .map(|user| (user.id.get(), user.name.to_string()))
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
        .bind(data.code.as_str())
        .bind(data.uses as i64)
        .execute(&self.pool)
        .await
        {
            log::error!("Failed to insert invite {}: {}", data.code, e);
        }

                let msg = messages::invites::build_invite_message(&data);
                send_message(msg, &ctx, self.config.join_leave_channel).await;
            }
            FullEvent::Message { new_message, .. } => {
                let attachments_string = self.format_attachments(new_message);
                let stickers_string = self.format_stickers(new_message);

                let content = new_message.content.as_str();

                if let Err(e) = sqlx::query(
                    "INSERT INTO messages (id, user_id, message, attachments, stickers) \
              VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(new_message.id.get() as i64)
                .bind(new_message.author.id.get() as i64)
                .bind(content)
                .bind(attachments_string)
                .bind(stickers_string)
                .execute(&self.pool)
                .await
                {
                    log::error!("Failed to store message: {}", e);
                }
            }
            FullEvent::MessageUpdate {
                old_if_available: _,
                event,
                ..
            } => {
                let Some(guild) = event.message.guild_id else {
                    return;
                };

                if event.message.author.bot() {
                    return;
                }

                let result = sqlx::query(
                    "WITH old AS ( \
                SELECT id, user_id, message, edits \
                FROM messages \
                WHERE id = $1 \
            ) \
            UPDATE messages AS m \
            SET \
                message = COALESCE($2, m.message), \
                edits = m.edits + 1 \
            FROM old \
            WHERE m.id = old.id \
            RETURNING \
               m.user_id, OLD.message, OLD.edits",
                )
                .bind(event.message.id.get() as i64)
                .bind(event.message.content.as_str())
                .fetch_optional(&self.pool)
                .await;

                let result = match result {
                    Ok(Some(row)) => row,
                    Ok(None) => return,
                    Err(e) => {
                        log::error!("Failed to update message edit: {}", e);
                        return;
                    }
                };

                let user_id: i64 = result.get(0);
                let old_message: Option<String> = result.get(1);
                let edits: i32 = result.get(2);

                if let Some(old_message) = old_message {
                    let new_message = event.message.content.as_str();

                    if old_message.is_empty() || new_message.starts_with(old_message.as_str()) {
                        // If the message has no content or does not remove any content, there's no point in logging
                        return;
                    }
                    let similarity = levenshtein_limit(
                        new_message,
                        old_message.as_str(),
                        self.config.edited_msg_distance,
                    );

                    if similarity < self.config.edited_msg_distance {
                        return;
                    }

                    let msg = messages::messages::build_edited_message(
                        Some(event.message.author.clone()),
                        UserId::new(user_id as u64),
                        event
                            .message
                            .channel_id
                            .to_channel(&ctx, Some(guild))
                            .await
                            .ok(),
                        event.message.channel_id,
                        guild,
                        event.message.id,
                        old_message,
                        edits,
                    );

                    send_message(msg, &ctx, self.config.deleted_msg_channel).await;
                }
            }
            FullEvent::MessageDelete {
                channel_id,
                deleted_message_id,
                guild_id,
                ..
            } => {
                let Some(guild_id) = guild_id else {
                    // Ignore DMs
                    return;
                };

                let row = sqlx::query(
                    "SELECT user_id, message, attachments, stickers, edits \
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

                let (user_id, content, attachments, stickers, edits) = row
                    .as_ref()
                    .map(|r| {
                        (
                            r.get::<i64, _>(0),
                            Some(r.get::<String, _>(1)),
                            r.get::<Option<String>, _>(2),
                            r.get::<Option<String>, _>(3),
                            r.get::<i32, _>(4),
                        )
                    })
                    .unwrap_or((0, None, None, None, 0));

                let (mut user_id, mut user) = if user_id != 0 {
                    let user_id = UserId::new(user_id as u64);
                    let user = user_id.to_user(&ctx).await.ok();

                    (Some(user_id), user)
                } else {
                    (None, None)
                };

                let entry =
                    get_message_deleted_entry(guild_id, &channel_id, &user_id, &ctx, &self.pool)
                        .await;

                let (deleter_id, deleter_user) = if let Some(entry) = entry {
                    let deleter_id = entry.user_id;
                    let deleter_user = match deleter_id {
                        Some(id) => id.to_user(&ctx).await.ok(),
                        None => None,
                    };

                    // If the message was not cached and we don't have the user_id, get it from the audit log instead
                    if user_id.is_none()
                        && let Some(audit_user_id) = entry.target_id
                    {
                        let audit_user_id = UserId::new(audit_user_id.get());
                        user_id = Some(audit_user_id);
                        user = audit_user_id.to_user(&ctx).await.ok();
                    };
                    (deleter_id, deleter_user)
                } else {
                    // Ignore bots only if the message is deleted by the bot itself
                    if let Some(user) = &user
                        && user.bot()
                    {
                        return;
                    }

                    (None, None)
                };

                let msg = messages::messages::build_deleted_message(
                    user,
                    user_id,
                    deleter_user,
                    deleter_id,
                    channel_id.to_channel(&ctx.http, Some(*guild_id)).await.ok(),
                    *channel_id,
                    *guild_id,
                    *deleted_message_id,
                    content,
                    attachments,
                    stickers,
                    edits,
                )
                .await;

                send_message(msg, &ctx, self.config.deleted_msg_channel).await;
            }
            FullEvent::MessageDeleteBulk {
                channel_id,
                multiple_deleted_messages_ids,
                guild_id,
                ..
            } => {
                if guild_id.is_none() {
                    // Ignore DMs
                    return;
                };

                if multiple_deleted_messages_ids.is_empty() {
                    return;
                }

                let count = multiple_deleted_messages_ids.len();

                let ids: Vec<i64> = multiple_deleted_messages_ids
                    .iter()
                    .map(|id| id.get() as i64)
                    .collect();

                // Use ANY() with a single array parameter
                let rows = sqlx::query_as::<_, (i64, String)>(
                    "SELECT DISTINCT user_id, message \
            FROM messages \
            WHERE id = ANY($1)",
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

                    if count > 10 && msg.len() < self.config.bulk_delete_min_length {
                        continue;
                    }

                    let msg = msg.replace('\n', " ");
                    let msg = if msg.chars().count() > self.config.bulk_delete_max_length {
                        format!(
                            "{}...",
                            msg.chars()
                                .take(self.config.bulk_delete_max_length)
                                .collect::<String>()
                                .trim()
                        )
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

                let msg = messages::messages::build_bulk_delete_message(
                    messages_with_user,
                    channel_id.to_channel(&ctx.http, *guild_id).await.ok(),
                    *channel_id,
                    count,
                );

                send_message(msg, &ctx, self.config.deleted_msg_channel).await;
            }
            FullEvent::GuildAuditLogEntryCreate {
                entry, guild_id, ..
            } => {
                let user = match entry.user_id {
                    Some(user_id) => user_id.to_user(&ctx).await.ok(),
                    None => None,
                };

                let msg = match entry.action {
                    Action::GuildUpdate => return,
                    Action::Channel(_) | Action::Thread(_) => {
                        messages::channel::build_channel_message(entry, user, guild_id, &ctx).await
                    }
                    Action::ChannelOverwrite(_) => {
                        messages::channel::build_permission_override_message(
                            entry.clone(),
                            user,
                            guild_id,
                            &ctx,
                        )
                        .await
                    }
                    Action::Role(_) => {
                        messages::roles::build_role_message(entry.clone(), user, *guild_id, &ctx)
                            .await
                    }
                    Action::Member(MemberAction::Prune) => {
                        messages::member::build_purge_message(entry.clone(), user)
                    }
                    Action::Member(MemberAction::BanRemove) => {
                        messages::member::build_unban_message(entry.clone(), user, &ctx).await
                    }
                    Action::Member(MemberAction::RoleUpdate) => {
                        messages::member::build_role_change_message(entry.clone(), user, &ctx).await
                    }
                    Action::Member(MemberAction::Update) => {
                        messages::member::build_member_update_message(entry.clone(), user, &ctx)
                            .await
                    }
                    Action::Member(MemberAction::BotAdd) => {
                        messages::member::build_bot_message(entry.clone(), user, &ctx).await
                    }
                    Action::AutoMod(_) => {
                        messages::automod::build_automod_message(
                            entry.clone(),
                            user,
                            *guild_id,
                            &ctx,
                        )
                        .await
                    }
                    Action::Sticker(_) => {
                        messages::sticker::build_sticker_message(entry.clone(), user, &ctx).await
                    }
                    Action::Emoji(_) => messages::sticker::build_emoji_message(entry.clone(), user),

                    // TODO
                    Action::Webhook(_) => return,
                    Action::Integration(_) => return,
                    Action::ScheduledEvent(_) => return,
                    Action::VoiceChannelStatus(_) => return,

                    Action::Message(_) => return,
                    Action::StageInstance(_) => return,
                    Action::CreatorMonetization(_) => return,
                    _ => return,
                };

                let Some(msg) = msg else {
                    return;
                };

                send_message(msg, &ctx, self.config.audit_log_channel).await;
            }

            _ => return,
        }
    }
}

fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    let pattern = "{d(%Y-%m-%d %H:%M:%S)} {M} {l:5} - {m}{n}";

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
        pool: pool.clone(),
    };

    let max_age_seconds = handler.config.purge_retention_days * 24 * 60 * 60;
    let seconds_interval = handler.config.purge_interval_hours * 60 * 60;
    if max_age_seconds != 0 && seconds_interval != 0 {
        tokio::spawn(purge_thread(pool, max_age_seconds, seconds_interval));
    }

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_INVITES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MODERATION
        | GatewayIntents::GUILD_MESSAGES;

    let mut client = Client::builder(token, intents)
        .event_handler(Arc::new(handler))
        .await?;

    client.start().await?;
    Ok(())
}
