use crate::{
    find_change, format_boolean_change, format_generic_change, format_numeric_change,
    format_numeric_change_operation, format_string_change,
    messages::{
        colours::*,
        utils::{build_embed_author, format_channel, format_user, perm_to_icon},
    },
    unwrap_change,
};
use serenity::all::{
    AuditLogEntry,
    Change::{self},
    Channel, ChannelAction, ChannelFlags, ChannelId, ChannelOverwriteAction, ChannelType, Context,
    CreateEmbed, CreateMessage, EntityType, GuildId, PermissionOverwrite, PermissionOverwriteType,
    Permissions, ThreadAction, User, UserId, VideoQualityMode,
    audit_log::Action,
};
use std::fmt::Write;

pub async fn build_channel_message(
    entry: &AuditLogEntry,
    user: Option<User>,
    guild_id: &GuildId,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(target_id) = entry.target_id else {
        return None;
    };

    let Some(user_id) = entry.user_id else {
        return None;
    };

    let user_str = format_user(&user, user_id);

    let channel_id = ChannelId::new(target_id.get());
    let channel = channel_id
        .to_guild_channel(&ctx.http, Some(*guild_id))
        .await
        .ok();

    let channel_type = channel
        .as_ref()
        .map(|c| c.base.kind.name().replace("_", " "))
        .unwrap_or_else(|| "channel".to_string());

    let channel_str = format_channel(&channel.map(Channel::Guild), channel_id);

    let (action, colour) = match entry.action {
        Action::Channel(ChannelAction::Create) | Action::Thread(ThreadAction::Create) => {
            ("created", POSITIVE_COLOUR)
        }
        Action::Channel(ChannelAction::Delete) | Action::Thread(ThreadAction::Delete) => {
            ("deleted", NEGATIVE_COLOUR)
        }
        Action::Channel(ChannelAction::Update) | Action::Thread(ThreadAction::Update) => {
            // ignore channel updates made by bots
            if let Some(user) = &user
                && user.bot()
            {
                return None;
            }
            ("updated", EDIT_COLOUR)
        }
        a => {
            log::error!(
                "Invalid action passed to channel message builder: {}",
                a.num()
            );
            ("unknown action", ERROR_COLOUR)
        }
    };

    let changes = entry
        .changes
        .iter()
        .filter_map(build_channel_change_line)
        .collect::<Vec<_>>()
        .join("\n");

    let embed_author = build_embed_author(&user, user_id);
    let message = format!("{user_str} **{action} {channel_type}** {channel_str}\n\n{changes}");
    let title = format!("{channel_type} {action}").to_uppercase();

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

pub async fn build_permission_override_message(
    entry: AuditLogEntry,
    user: Option<User>,
    guild_id: &GuildId,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target channel id provided");
        return None;
    };

    let Some(options) = entry.options else {
        return None;
    };

    let Some(user_id) = entry.user_id else {
        return None;
    };

    let Some(permission_target_id) = options.id else {
        return None;
    };

    let user_str = format_user(&user, user_id);

    let channel_id = ChannelId::new(target_id.get());
    let channel = channel_id
        .to_guild_channel(&ctx, Some(*guild_id))
        .await
        .ok();
    let channel = format_channel(&channel.map(Channel::Guild), channel_id);

    let permission_target_string = if let Some(role_name) = options.role_name {
        if permission_target_id.get() == guild_id.get() {
            "- **Permissions for** @everyone".to_string()
        } else {
            format!("- **Permissions for role** <@&{permission_target_id}>({role_name})")
        }
    } else {
        let user_id = UserId::new(permission_target_id.get());
        let user = user_id.to_user(&ctx).await.ok();
        format!("- **Permissions for user** {}", format_user(&user, user_id))
    };

    let (action, colour) = match entry.action {
        Action::ChannelOverwrite(ChannelOverwriteAction::Create) => ("created", POSITIVE_COLOUR),
        Action::ChannelOverwrite(ChannelOverwriteAction::Delete) => ("deleted", NEGATIVE_COLOUR),
        Action::ChannelOverwrite(ChannelOverwriteAction::Update) => ("updated", EDIT_COLOUR),
        a => {
            log::error!(
                "Invalid action passed to channel message builder: {}",
                a.num()
            );
            ("unknown action", ERROR_COLOUR)
        }
    };

    let permission_changes_str = unwrap_changes(&entry.changes);

    let embed_author = build_embed_author(&user, user_id);
    let message = format!(
        "{user_str} **{action} permission override**\n\
                    **for channel** {channel}\n\n\
                    {permission_target_string}:\n\
                    {permission_changes_str}"
    );
    let title = format!("PERMISSION OVERRIDE {}", action.to_uppercase());

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

fn build_channel_change_line(change: &Change) -> Option<String> {
    Some(match change {
        Change::UserLimit { old, new } => {
            format_numeric_change!("User limit", "", old.map(|v| v.get()), new.map(|v| v.get()))
        }
        Change::RateLimitPerUser { old, new } => format_numeric_change!("Slowmode", "s", old, new),
        Change::Name { old, new } => format_string_change!("Name", old, new),
        Change::Topic { old, new } => format_string_change!("Description", old, new),
        Change::Nsfw { old, new } => format_boolean_change!("NSFW", old, new),
        Change::Locked { old, new } => format_boolean_change!("Locked", old, new),
        Change::Archived { old, new } => format_boolean_change!("Archived", old, new),
        Change::Invitable { old, new } => format_boolean_change!("Inviteable", old, new),
        Change::RtcRegion { old, new } => format_string_change!("RTC region", old, new),

        Change::DefaultAutoArchiveDuration { old, new } => {
            format_numeric_change_operation!("Archive duration", old, new, |v| format!(
                "`{}h`",
                v / 60
            ))
        }

        Change::Position { old, new } => match (old, new) {
            (Some(old), Some(new)) => {
                if new > old {
                    "- **Rank changed:** 🠉"
                } else {
                    "- **Rank changed:** 🠋"
                }
            }
            _ => return None,
        }
        .to_string(),
        Change::Bitrate { old, new } => {
            format_numeric_change_operation!("Bitrate", old, new, |v| format!("`{}kbps`", v / 1000))
        }

        Change::VideoQualityMode { old, new } => {
            format_generic_change!("Video quality", old, new, |&q| match q {
                VideoQualityMode::Auto => "`auto`",
                VideoQualityMode::Full => "`720p`",
                _ => "`unknown",
            })
        }

        Change::Type { old, new } => format_generic_change!("Type", old, new, format_channel_type),

        Change::PermissionOverwrites { old, new } => match (old, new) {
            (_, Some(new)) => format_access_permission(new),
            (Some(old), _) => format_access_permission(old),
            _ => return None,
        },
        Change::Flags { old, new } => match (old, new) {
            (Some(old), Some(new)) => return format_flags_diff(old, new),
            (None, Some(new)) => return format_flags(new),
            (Some(old), None) => return format_flags(old),
            _ => return None,
        },

        _ => return None,
    })
}

fn format_channel_type(entity_type: &EntityType) -> String {
    match entity_type {
        EntityType::Str(entity_type) => entity_type.to_string(),
        EntityType::Int(entity_type) => ChannelType::Unknown(*entity_type as u8).name().to_string(),
        _ => "unknown".to_string(),
    }
}

fn format_flags_diff(old: &u64, new: &u64) -> Option<String> {
    let new_flags = ChannelFlags::from_bits(*new as u32);
    let changed_flags = ChannelFlags::from_bits((old ^ new) as u32);

    let Some(new_flags) = new_flags else {
        return None;
    };
    let Some(changed_flags) = changed_flags else {
        return None;
    };

    let mut result = String::new();

    for (name, flag) in changed_flags.iter_names() {
        writeln!(
            &mut result,
            "- **{name}**: `{}`",
            flag.intersects(new_flags)
        )
        .unwrap();
    }

    result.pop();
    Some(result)
}

fn format_flags(flags: &u64) -> Option<String> {
    let Some(flags) = ChannelFlags::from_bits(*flags as u32) else {
        return None;
    };

    if flags.is_empty() {
        return None;
    }

    let mut result = String::new();

    for (name, _flag) in flags.iter_names() {
        writeln!(&mut result, "- **{name}**: `true`").unwrap();
    }

    // remove the last \n
    result.pop();
    Some(result)
}

fn format_access_permission(permission_overrides: &[PermissionOverwrite]) -> String {
    let mut result = "- **Access:**".to_string();

    for permission in permission_overrides {
        let emoji = perm_to_icon(permission.allow, permission.deny, Permissions::VIEW_CHANNEL);

        match permission.kind {
            PermissionOverwriteType::Role(role_id) => {
                writeln!(&mut result, "  - Role <@&{role_id}>: {emoji}").unwrap();
            }
            PermissionOverwriteType::Member(user_id) => {
                writeln!(&mut result, "  - User <@{user_id}>: {emoji}").unwrap();
            }
            _ => {
                result.push_str("Invalid permission\n");
            }
        };
    }
    result.pop();
    result
}

fn format_permission_override(allow: Permissions, deny: Permissions) -> String {
    let combined_perms = allow | deny;

    if combined_perms.is_empty() {
        return "  - *none*".to_string();
    }

    let mut result = String::new();

    for perm in combined_perms.iter() {
        writeln!(
            &mut result,
            "  - {perm}: {}",
            perm_to_icon(allow, deny, perm),
        )
        .unwrap();
    }

    result.pop();
    result
}

fn format_permission_override_change(
    old_allow: Permissions,
    new_allow: Permissions,
    old_deny: Permissions,
    new_deny: Permissions,
) -> String {
    let perms_difference = (old_allow ^ new_allow) | (old_deny ^ new_deny);

    let mut result = String::new();

    for perm in perms_difference.iter() {
        writeln!(
            &mut result,
            "  - {perm}: {} ➜ {}",
            perm_to_icon(old_allow, old_deny, perm),
            perm_to_icon(new_allow, new_deny, perm),
        )
        .unwrap();
    }

    result.pop();
    result
}

fn unwrap_changes(changes: &[Change]) -> String {
    let allow = find_change!(changes, Change::Allow);
    let deny = find_change!(changes, Change::Deny);

    let (allow_old, allow_new) = unwrap_change!(allow, Change::Allow);
    let (deny_old, deny_new) = unwrap_change!(deny, Change::Deny);

    let is_change =
        allow_old.is_some() && allow_new.is_some() || deny_old.is_some() && deny_new.is_some();

    if is_change {
        format_permission_override_change(
            allow_old.unwrap_or_else(Permissions::empty),
            allow_new.unwrap_or_else(Permissions::empty),
            deny_old.unwrap_or_else(Permissions::empty),
            deny_new.unwrap_or_else(Permissions::empty),
        )
    } else {
        let allow = allow_new.or(*allow_old).unwrap_or_else(Permissions::empty);
        let deny = deny_new.or(*deny_old).unwrap_or_else(Permissions::empty);

        format_permission_override(allow, deny)
    }
}
