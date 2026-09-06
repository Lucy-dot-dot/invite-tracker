use serenity::{
    all::{
        AuditLogEntry, AuditLogEntryId, Context, GenericChannelId, GuildId, MemberAction,
        MessageAction, UserId, audit_log::Action,
    },
    nonmax::NonMaxU8,
};
use sqlx::PgPool;

// Max number of logs to look through
const NUMBER_OF_LOG_LIMIT: Option<NonMaxU8> = NonMaxU8::new(10);

pub async fn init_audit_log(guild_id: GuildId, ctx: &Context, pool: &PgPool) {
    let logs = guild_id
        .audit_logs(&ctx.http, None, None, None, None, NUMBER_OF_LOG_LIMIT)
        .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs for init: {}", e);
            return;
        }
    };

    for entry in logs.entries {
        let count = if let Some(options) = &entry.options
            && let Some(count) = options.count
        {
            count.get() as i32
        } else {
            1
        };

        if let Err(e) = sqlx::query("SELECT update_audit_count($1, $2)")
            .bind(entry.id.get() as i64)
            .bind(count)
            .execute(pool)
            .await
        {
            log::error!("Failed to upsert audit log: {}", e);
            continue;
        };
    }
}

pub async fn get_message_deleted_entry(
    guild_id: &GuildId,
    channel_id: &GenericChannelId,
    user_id: &Option<UserId>,
    ctx: &Context,
    pool: &PgPool,
) -> Option<AuditLogEntry> {
    let logs = guild_id
        .audit_logs(
            &ctx.http,
            Some(Action::Message(MessageAction::Delete)),
            None,
            None,
            None,
            NUMBER_OF_LOG_LIMIT,
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
        if let Some(options) = &entry.options
            && let Some(msg_channel_id) = options.channel_id
            && msg_channel_id.get() != channel_id.get()
        {
            continue;
        }

        if let Some(expected_user) = user_id
            && let Some(target_id) = &entry.target_id
            && target_id.get() != expected_user.get()
        {
            continue;
        }

        let count = if let Some(options) = &entry.options
            && let Some(count) = options.count
        {
            count.get() as i32
        } else {
            1
        };

        if is_new_entry(&entry.id, count, pool).await {
            return Some(entry);
        }
    }

    return None;
}

pub async fn get_ban_or_kick_event(
    guild_id: &GuildId,
    user_id: &UserId,
    ctx: &Context,
    pool: &PgPool,
) -> Option<AuditLogEntry> {
    let logs = guild_id
        .audit_logs(&ctx.http, None, None, None, None, NUMBER_OF_LOG_LIMIT)
        .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs for user leave: {}", e);
            return None;
        }
    };

    for entry in logs.entries {
        // skip entries for other targets
        if let Some(target_id) = &entry.target_id
            && target_id.get() != user_id.get()
        {
            continue;
        }

        // skip entries that are not a bank or kick event
        match &entry.action {
            Action::Member(MemberAction::BanAdd) | Action::Member(MemberAction::Kick) => (),
            _ => continue,
        }

        if is_new_entry(&entry.id, 1, pool).await {
            return Some(entry);
        }
    }
    return None;
}

pub async fn get_bulk_delete_entry(
    guild_id: &GuildId,
    channel_id: &GenericChannelId,
    msg_count: usize,
    ctx: &Context,
    pool: &PgPool,
) -> Option<AuditLogEntry> {
    let logs = guild_id
        .audit_logs(
            &ctx.http,
            Some(Action::Message(MessageAction::BulkDelete)),
            None,
            None,
            None,
            NUMBER_OF_LOG_LIMIT,
        )
        .await;

    let logs = match logs {
        Ok(logs) => logs,
        Err(e) => {
            log::error!("Failed to fetch audit logs for bulk delete: {}", e);
            return None;
        }
    };

    for entry in logs.entries {
        if let Some(options) = &entry.options
            && let Some(msg_channel_id) = options.channel_id
            && msg_channel_id.get() != channel_id.get()
        {
            continue;
        }

        if let Some(options) = &entry.options
            && let Some(count) = options.count
            && msg_count == count.get() as usize
        {
        } else {
            continue;
        };

        if is_new_entry(&entry.id, msg_count as i32, pool).await {
            return Some(entry);
        }
    }

    return None;
}

async fn is_new_entry(id: &AuditLogEntryId, count: i32, pool: &PgPool) -> bool {
    match sqlx::query_scalar::<_, Option<i32>>("SELECT update_audit_count($1, $2)")
        .bind(id.get() as i64)
        .bind(count)
        .fetch_one(pool)
        .await
    {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(e) => {
            log::error!("Failed to upsert audit log: {}", e);
            false
        }
    }
}
