use serenity::{
    all::{AuditLogEntry, Change, Context, CreateEmbed, CreateMessage, Timestamp, User, UserId},
    small_fixed_array::FixedString,
};

use crate::{
    find_change, format_boolean_change, format_string_change,
    messages::{
        colours::*,
        format_time::format_time_diff,
        utils::{build_embed_author, build_embed_author_admin, format_user, get_reason},
    },
};

pub async fn build_role_change_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    if let Some(admin) = &admin
        && admin.bot()
    {
        return None;
    };

    let Some(target_id) = entry.target_id else {
        return None;
    };

    let Some(admin_id) = entry.user_id else {
        return None;
    };

    // Ignore self-roles, like in onboarding or server guide
    if target_id.get() == admin_id.get() {
        return None;
    }

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();

    let user_str = format_user(&user, user_id);
    let admin_str = format_user(&admin, admin_id);

    let embed_author = build_embed_author_admin(&user, user_id, &admin);
    let avatar_url = user.map_or(String::new(), |u| u.face());

    let added = find_change!(entry.changes, Change::RolesAdded);
    let removed = find_change!(entry.changes, Change::RolesRemoved);

    let (title, header, colour) = match (added, removed) {
        (Some(_), None) => (
            "MEMBER ROLE ADDED",
            format!("{admin_str} **added roles to** {user_str}"),
            POSITIVE_COLOUR,
        ),
        (None, Some(_)) => (
            "MEMBER ROLE REMOVED",
            format!("{admin_str} **removed roles from** {user_str}"),
            NEGATIVE_COLOUR,
        ),
        _ => (
            "ROLES UPDATED",
            format!("{admin_str} **updated roles for** {user_str}"),
            EDIT_COLOUR,
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

    if let Some(Change::RolesRemoved {
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
        .description(message)
        .thumbnail(avatar_url, None);

    Some(CreateMessage::new().embed(embed))
}

pub fn build_purge_message(
    entry: AuditLogEntry,
    user: Option<User>,
) -> Option<CreateMessage<'static>> {
    let Some(options) = entry.options else {
        return None;
    };

    let Some(admin_id) = entry.user_id else {
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

    let user_str = format_user(&user, admin_id);

    let embed_author = build_embed_author(&user, admin_id);

    let message = format!("{user_str} **purged** {number} **members**{inactive_days}");

    let embed = CreateEmbed::new()
        .title("MEMBERS PURGE")
        .author(embed_author)
        .color(NEGATIVE_COLOUR)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_bot_message(
    entry: AuditLogEntry,
    user: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(user_id) = entry.user_id else {
        return None;
    };

    let user_str = format_user(&user, user_id);

    let Some(target_id) = entry.target_id else {
        return None;
    };

    let bot_id = UserId::new(target_id.get());
    let bot = bot_id.to_user(&ctx).await.ok();

    let bot_str = format_user(&bot, bot_id);
    let avatar_url = bot.map_or(String::new(), |u| u.face());

    let embed_author = build_embed_author(&user, user_id);

    let message = format!("{user_str} **added bot** {bot_str}");

    let embed = CreateEmbed::new()
        .title("BOT ADDED")
        .author(embed_author)
        .color(POSITIVE_COLOUR)
        .description(message)
        .thumbnail(avatar_url, None);

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_unban_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(admin_id) = entry.user_id else {
        return None;
    };
    let Some(target_id) = entry.target_id else {
        return None;
    };

    let admin_str = format_user(&admin, admin_id);

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();

    let user_str = format_user(&user, user_id);
    let embed_author = build_embed_author_admin(&user, user_id, &admin);
    let avatar_url = user.map_or(String::new(), |u| u.face());

    let reason = get_reason(&entry.reason);

    let message = format!("{admin_str} **unbanned** {user_str}\n**Reason:** {reason}");

    let embed = CreateEmbed::new()
        .title("MEMBER UNBANNED")
        .author(embed_author)
        .color(POSITIVE_COLOUR)
        .description(message)
        .thumbnail(avatar_url, None);

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_member_update_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(target_id) = entry.target_id else {
        return None;
    };

    let Some(admin_id) = entry.user_id else {
        return None;
    };

    // If self-action, ignore admin, just show self-change
    let (admin_string, admin) = if target_id.get() == admin_id.get() {
        (None, None)
    } else {
        (Some(format_user(&admin, admin_id)), admin)
    };

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();
    let user_string = format_user(&user, user_id);

    let embed: CreateEmbed = if entry.changes.len() == 1 {
        match &entry.changes[0] {
            Change::Mute { old, new } => {
                build_mute_message("muted", old, new, user_string, admin_string)
            }
            Change::Deaf { old, new } => {
                build_mute_message("deafened", old, new, user_string, admin_string)
            }
            Change::CommunicationDisabledUntil { old, new } => build_timeout_message(
                old,
                new,
                entry.id.created_at(),
                user_string,
                admin_string.unwrap_or(String::new()),
                entry.reason,
            ),
            Change::Nick { old, new } => {
                build_username_change(old, new, user_string, admin_string, &user)
            }
            _ => format_member_changes(
                entry.changes,
                user_string,
                admin_string.unwrap_or(String::new()),
            ),
        }
    } else {
        format_member_changes(
            entry.changes,
            user_string,
            admin_string.unwrap_or(String::new()),
        )
    };

    let embed_author = build_embed_author_admin(&user, user_id, &admin);
    let avatar_url = user.map_or(String::new(), |u| u.face());

    Some(CreateMessage::new().embed(embed.thumbnail(avatar_url, None).author(embed_author)))
}

fn format_member_changes(
    changes: Vec<Change>,
    user_string: String,
    admin_string: String,
) -> CreateEmbed<'static> {
    let changes = changes
        .iter()
        .filter_map(format_member_change)
        .collect::<Vec<_>>()
        .join("\n");
    let description = format!("{admin_string} **updated member** {user_string}**:**\n\n{changes}");

    CreateEmbed::new()
        .description(description)
        .title("MEMBER UPDATED")
        .color(EDIT_COLOUR)
}

fn build_timeout_message(
    old: &Option<Timestamp>,
    new: &Option<Timestamp>,
    now: Timestamp,
    user_string: String,
    admin_string: String,
    reason: Option<FixedString>,
) -> CreateEmbed<'static> {
    let now = now.unix_timestamp();

    let (description, title, colour) = match (old, new) {
        (_, Some(until)) => {
            let reason = get_reason(&reason);

            let until = until.unix_timestamp();
            let time_remaining = until - now;
            let formatted_time = format_time_diff(time_remaining as u64, 3);

            let description = format!(
                "{admin_string} **timed out** {user_string}**:**\n\n\
                                                **Duration:** `{formatted_time}`\n\
                                                **Expires:** <t:{until}:R>\n\
                                                **Reason:** {reason}"
            );
            (description, "MEMBER TIMED-OUT", DANGER_COLOUR)
        }
        (Some(until), _) => {
            let time_remaining = until.unix_timestamp() - now;
            let formatted_time = format_time_diff(time_remaining as u64, 3);

            let description = format!(
                "{admin_string} **removed time-out from** {user_string}**:**\n\n\
                **Time left before removal:** `{formatted_time}`"
            );
            (description, "TIMEOUT REMOVED", NEGATIVE_COLOUR)
        }
        // should not be reached but it's here anyways
        _ => ("timeout action".to_string(), "TIMEOUT ACTION", ERROR_COLOUR),
    };

    CreateEmbed::new()
        .description(description)
        .title(title)
        .color(colour)
}

fn build_mute_message(
    action: &str,
    old: &Option<bool>,
    new: &Option<bool>,
    user_string: String,
    admin_string: Option<String>,
) -> CreateEmbed<'static> {
    let (action, colour) = match (old, new) {
        (_, Some(true)) => (format!("{action}"), DANGER_COLOUR),
        (Some(true), _) => (format!("un-{action}"), POSITIVE_COLOUR),
        // should not be reached but it's here anyways
        _ => (format!("{action} action"), ERROR_COLOUR),
    };

    let title = format!("MEMBER {action} FROM VC").to_uppercase();

    let description = match admin_string {
        Some(admin_string) => format!("{admin_string} **{action}** {user_string}"),
        _ => format!("{user_string} **{action} themselves**"),
    };

    CreateEmbed::new()
        .description(description)
        .color(colour)
        .title(title)
}

fn build_username_change(
    old: &Option<FixedString>,
    new: &Option<FixedString>,
    user_string: String,
    admin_string: Option<String>,
    user: &Option<User>,
) -> CreateEmbed<'static> {
    let description = match admin_string {
        Some(admin_string) => format!("{admin_string} **changed nickname for** {user_string}**:**"),
        _ => format!("{user_string} **changed their nickname:**"),
    };

    let mut embed = CreateEmbed::new()
        .description(description)
        .title("MEMBER NICKNAME UPDATE")
        .color(EDIT_COLOUR);

    if let Some(user) = user {
        let globalname = user.global_name.as_ref().unwrap_or(&user.name).as_str();
        let mut shows_globalname = false;

        if let Some(old) = old
            && old.as_str() != globalname
        {
            embed = embed.field("Old:", old.to_string(), true);
        } else {
            shows_globalname = true;
            embed = embed.field("Old (Globalname):", globalname.to_string(), true);
        }

        if let Some(new) = new
            && new.as_str() != globalname
        {
            embed = embed.field("New:", new.to_string(), true);
        } else {
            shows_globalname = true;
            embed = embed.field("New (Globalname):", globalname.to_string(), true);
        }

        if !shows_globalname {
            embed = embed.field("Globalname:", globalname.to_string(), false);
        }
    }

    embed
}

fn format_member_change(change: &Change) -> Option<String> {
    Some(match change {
        Change::Mute { old, new } => format_boolean_change!("Muted", old, new),
        Change::Deaf { old, new } => format_boolean_change!("Deafened", old, new),
        Change::Nick { old, new } => format_string_change!("Nickname", old, new),
        Change::CommunicationDisabledUntil { old, new } => match (old, new) {
            (Some(old), Some(new)) => {
                format!("- **Timeout until:** <t:{old}:R> ➜ <t:{new}:R>").into()
            }
            (None, Some(new)) => format!("- **Timeout until:** <t:{new}:R>").into(),
            (Some(old), None) => format!("- **Timeout until:** *was* <t:{old}:R>").into(),
            _ => return None,
        },
        _ => return None,
    })
}
