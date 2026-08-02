use serenity::all::{
    AuditLogEntry, ChannelId, GuildId, Http, MessageAction, TargetId, UserId, audit_log::Action,
};
use sqlx::{PgPool, Row};

pub async fn get_message_deleted_entry(
    guild: GuildId,
    http: impl AsRef<Http>,
    channel_id: ChannelId,
    user: Option<UserId>,
    limit: Option<u8>,
    pool: &PgPool,
) -> Option<AuditLogEntry> {
    let logs = guild
        .audit_logs(
            http,
            Some(Action::Message(MessageAction::Delete)),
            None,
            None,
            limit,
        )
        .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs: {}", e);
            return None;
        }
    };

    for entry in logs.entries {
        let count = if let Some(changes) = &entry.changes {
            changes.len() as i32
        } else if let Some(options) = &entry.options
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

        if old_count.is_none() {
            continue;
        }

        if let Some(options) = &entry.options
            && let Some(msg_channel_id) = options.channel_id
            && msg_channel_id != channel_id
        {
            continue;
        }

        if let Some(expected_user) = user
            && let Some(target_id) = &entry.target_id
            && target_id.get() != expected_user.get()
        {
            continue;
        }

        return Some(entry);
    }

    return None;
}
