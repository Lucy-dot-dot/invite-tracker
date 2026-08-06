use serenity::all::{
    AuditLogEntry, Change, Colour, Context, CreateEmbed, CreateMessage, User, UserId,
};

use crate::messages::utils::{build_embed_author, build_embed_author_admin, format_user};

pub async fn build_role_change_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage> {
    if let Some(admin) = &admin
        && admin.bot
    {
        return None;
    };

    let Some(target_id) = entry.target_id else {
        return None;
    };

    // Ignore self-roles, like in onboarding or server guide
    if target_id.get() == entry.user_id.get() {
        return None;
    }

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();

    let user_str = format_user(&user, user_id);
    let admin_str = format_user(&admin, entry.user_id);

    let embed_author = build_embed_author_admin(&user, user_id, &admin);

    let Some(changes) = entry.changes else {
        return None;
    };

    let added = changes
        .iter()
        .find(|&x| matches!(x, Change::RolesAdded { old: _, new: _ }));

    let removed = changes
        .iter()
        .find(|&x| matches!(x, Change::RolesRemove { old: _, new: _ }));

    let (title, header, colour) = match (added, removed) {
        (Some(_), None) => (
            "MEMBER ROLE ADDED",
            format!("{admin_str} **added roles to** {user_str}"),
            Colour::new(0x00FF00),
        ),
        (None, Some(_)) => (
            "MEMBER ROLE REMOVED",
            format!("{admin_str} **removed roles from** {user_str}"),
            Colour::new(0xFF0000),
        ),
        _ => (
            "ROLES UPDATED",
            format!("{admin_str} **updated roles for** {user_str}"),
            Colour::new(0xFFAA00),
        ),
    };

    let mut lines = Vec::new();

    if let Some(Change::RolesAdded {
        old: _,
        new: Some(new_roles),
    }) = added
    {
        lines.push("- **Roles added:**".to_string());
        for role in new_roles {
            lines.push(format!("  - <@&{}>({})", role.id, role.name));
        }
    }

    if let Some(Change::RolesRemove {
        old: _,
        new: Some(new_roles),
    }) = removed
    {
        lines.push("- **Roles removed:**".to_string());
        for role in new_roles {
            lines.push(format!("  - ~~<@&{}>({})~~", role.id, role.name));
        }
    }

    let message = format!("{header}\n\n{}", lines.join("\n"));

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

pub fn build_purge_message(entry: AuditLogEntry, user: Option<User>) -> Option<CreateMessage> {
    let Some(options) = entry.options else {
        return None;
    };

    let number = options
        .members_removed
        .map_or(String::new(), |m| m.to_string());

    let inactive_days = if let Some(delete_member_days) = options.delete_member_days {
        format!(" **inactive for** `{delete_member_days}d`")
    } else {
        String::new()
    };

    let user_str = format_user(&user, entry.user_id);

    let embed_author = build_embed_author(&user, entry.user_id);

    let message = format!("{user_str} **purged** {number} **members**{inactive_days}");

    let embed = CreateEmbed::new()
        .title("MEMBERS PURGE")
        .author(embed_author)
        .color(Colour::new(0xFF0000))
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_bot_message(
    entry: AuditLogEntry,
    user: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage> {
    let user_str = format_user(&user, entry.user_id);

    let Some(target_id) = entry.target_id else {
        return None;
    };

    let bot_id = UserId::new(target_id.get());
    let bot = bot_id.to_user(&ctx).await.ok();

    let bot_str = format_user(&bot, bot_id);

    let embed_author = build_embed_author(&user, entry.user_id);

    let message = format!("{user_str} **added bot** {bot_str}");

    let mut embed = CreateEmbed::new()
        .title("BOT ADDED")
        .author(embed_author)
        .color(Colour::new(0x00FF00))
        .description(message);

    if let Some(bot) = &bot {
        let avatar_url = bot.avatar_url().unwrap_or_else(|| bot.face());
        embed = embed.thumbnail(avatar_url);
    }

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_unban_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage> {
    let admin_str = format_user(&admin, entry.user_id);
    let Some(target_id) = entry.target_id else {
        return None;
    };

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();
    let user_str = format_user(&user, user_id);

    let embed_author = build_embed_author_admin(&user, user_id, &admin);

    let reason = if let Some(reason) = entry.reason
        && !reason.is_empty()
    {
        reason
    } else {
        "*No reason stated".to_string()
    };

    let message = format!("{admin_str} **unbanned** {user_str}\n\n**- Reason:** {reason}");

    let embed = CreateEmbed::new()
        .title("MEMBER UNBANNED")
        .author(embed_author)
        .color(Colour::new(0x00FF00))
        .description(message);

    Some(CreateMessage::new().embed(embed))
}
