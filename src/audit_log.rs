use serenity::all::{
    AuditLogEntry, ChannelId, GuildId, Http, MemberAction, MessageAction, UserId, audit_log::Action,
};
use sqlx::{PgPool, Row};

// Max number of logs to look through 
const NUMBER_OF_LOG_LIMIT: u8 = 10;

pub async fn get_message_deleted_entry(
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: Option<UserId>,
    http: impl AsRef<Http>,
    pool: &PgPool,
) -> Option<AuditLogEntry> {
    let logs = guild_id
        .audit_logs(
            http,
            Some(Action::Message(MessageAction::Delete)),
            None,
            None,
            Some(NUMBER_OF_LOG_LIMIT),
        )
        .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs for message deleted: {}", e);
            return None;
        }
    };

    for entry in logs.entries {
        let count = if let Some(options) = &entry.options
            && let Some(count) = options.count
        {
            count as i32
        } else {
            0
        };

        let result = sqlx::query(
            "SELECT update_audit_count($1, $2)"
        )
        .bind(entry.id.get() as i64)
        .bind(count)
        .fetch_one(pool)
        .await;

        let old_count = match result {
            Ok(row) => row.get::<Option<i32>, _>(0),
            Err(e) => {
                log::error!("Failed to upsert audit log: {}", e);
                continue;
            }
        };

        // if the entry was old and didn't change
        if old_count.is_none() {
            continue;
        }

        if let Some(options) = &entry.options
            && let Some(msg_channel_id) = options.channel_id
            && msg_channel_id != channel_id
        {
            continue;
        }

        if let Some(expected_user) = user_id
            && let Some(target_id) = &entry.target_id
            && target_id.get() != expected_user.get()
        {
            continue;
        }

        return Some(entry);
    }

    return None;
}


pub async fn get_ban_or_kick_event(
    guild_id: GuildId,
    user_id: UserId,
    http: impl AsRef<Http>,
    pool: &PgPool,
) -> Option<AuditLogEntry> {

    let logs = guild_id
    .audit_logs(
        http,
        None,
        None,
        None,
        Some(NUMBER_OF_LOG_LIMIT),
    )
    .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs for user leave: {}", e);
            return None;
        }
    };

    for entry in logs.entries {
        if let Some(target_id) = &entry.target_id
            && target_id.get() != user_id.get()
        {
            continue;
        }

        let result = sqlx::query(
            "SELECT update_audit_count($1, 0)"
        )
        .bind(entry.id.get() as i64)
        .fetch_one(pool)
        .await;

        let old_count = match result {
            Ok(row) => row.get::<Option<i32>, _>(0),
            Err(e) => {
                log::error!("Failed to upsert audit log: {}", e);
                continue;
            }
        };

        // if the entry was old and didn't change
        if old_count.is_none() {
            continue;
        }

        match &entry.action {
            Action::Member(MemberAction::BanAdd) | 
            Action::Member(MemberAction::Kick) => return Some(entry),
            _ => continue,
        }

    }

    return None;
}