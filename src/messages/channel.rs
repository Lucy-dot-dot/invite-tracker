use serenity::all::{
    AuditLogEntry, Change, ChannelAction, ChannelFlags, ChannelId, ChannelOverwriteAction,
    ChannelType, Colour, Context, CreateEmbed, CreateMessage, EntityType, GuildId,
    PermissionOverwrite, PermissionOverwriteType, Permissions, User, UserId, audit_log::Action,
};

use crate::{
    format_boolean_change, format_numeric_change, format_numeric_change_operation,
    format_string_change,
    messages::utils::{build_embed_author, format_channel, format_role, format_user},
};

pub async fn build_channel_message(
    entry: AuditLogEntry,
    user: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target channel id provided");
        return None;
    };

    let user_str = format_user(&user, entry.user_id);

    let channel_id = ChannelId::new(target_id.get());
    let channel = channel_id.to_channel(&ctx).await.ok();
    let channel = format_channel(channel, channel_id);

    let (action, colour) = match entry.action {
        Action::Channel(ChannelAction::Create) => ("created", Colour::new(0x00FF00)),
        Action::Channel(ChannelAction::Delete) => ("deleted", Colour::new(0xFF0000)),
        Action::Channel(ChannelAction::Update) => {
            // ignore channel updates made by bots
            if let Some(user) = &user
                && user.bot
            {
                return None;
            }
            ("updated", Colour::new(0xFFAA00))
        }
        a => {
            log::error!(
                "Invalid action passed to channel message builder: {}",
                a.num()
            );
            ("unknown action", Colour::new(0x000000))
        }
    };

    let changes = if let Some(changes) = entry.changes {
        changes
            .iter()
            .filter_map(build_channel_change_line)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let embed_author = build_embed_author(&user, entry.user_id);
    let message = format!("{user_str} **{action} channel** {channel}\n\n{changes}");
    let title = format!("CHANNEL {}", action.to_uppercase());

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
    ctx: &Context,
) -> Option<CreateMessage> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target channel id provided");
        return None;
    };

    let Some(options) = entry.options else {
        return None;
    };

    let Some(permission_target_id) = options.id else {
        return None;
    };

    let user_str = format_user(&user, entry.user_id);

    let channel_id = ChannelId::new(target_id.get());
    let channel = channel_id.to_channel(&ctx).await.ok();
    let channel = format_channel(channel, channel_id);

    let permission_target_string = if let Some(role_name) = options.role_name {
        if role_name == "@everyone" {
            "- **Permissions for** @everyone".to_string()
        } else {
            format!("- **Permissions for role** <@&{permission_target_id}>({role_name})")
        }
    } else {
        let user_id = UserId::new(permission_target_id.get());
        let user = user_id.to_user(&ctx).await.ok();
        format!("- **Permissions for user** {}", format_user(&user, user_id))
    };

    let Some(changes) = entry.changes else {
        log::error!("Changes not present in permission override log");
        return None;
    };

    let (action, colour) = match entry.action {
        Action::ChannelOverwrite(ChannelOverwriteAction::Create) => {
            ("created", Colour::new(0xFFAA00))
        }
        Action::ChannelOverwrite(ChannelOverwriteAction::Delete) => {
            ("deleted", Colour::new(0xFFAA00))
        }
        Action::ChannelOverwrite(ChannelOverwriteAction::Update) => {
            ("updated", Colour::new(0xFFAA00))
        }
        a => {
            log::error!(
                "Invalid action passed to channel message builder: {}",
                a.num()
            );
            ("unknown action", Colour::new(0x000000))
        }
    };

    let allow = changes
        .iter()
        .find(|&x| matches!(x, Change::Allow { old: _, new: _ }));
    let deny = changes
        .iter()
        .find(|&x| matches!(x, Change::Deny { old: _, new: _ }));

    let permission_changes_str = match (allow, deny) {
        (
            Some(Change::Allow {
                old: old_allow,
                new: new_allow,
            }),
            Some(Change::Deny {
                old: old_deny,
                new: new_deny,
            }),
        ) => match (old_allow, new_allow, old_deny, new_deny) {
            (Some(old_allow), Some(new_allow), Some(old_deny), Some(new_deny)) => {
                format_permission_override_change(*old_allow, *new_allow, *old_deny, *new_deny)
            }
            (None, Some(new_allow), None, Some(new_deny)) => {
                format_permission_override(*new_allow, *new_deny)
            }
            (Some(old_allow), None, Some(old_deny), None) => {
                format_permission_override(*old_allow, *old_deny)
            }
            _ => "ERROR WHILE PROCESSING PERMISSION CHANGES".to_string(),
        },
        _ => "ERROR WHILE PROCESSING PERMISSION CHANGES".to_string(),
    };

    let embed_author = build_embed_author(&user, entry.user_id);
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
        Change::UserLimit { old, new } => format_numeric_change!("User limit", "", old, new),
        Change::RateLimitPerUser { old, new } => format_numeric_change!("Slowmode", "s", old, new),
        Change::Name { old, new } => format_string_change!("Name", old, new),
        Change::Topic { old, new } => format_string_change!("Description", old, new),
        Change::Nsfw { old, new } => format_boolean_change!("NSFW", old, new),
        Change::DefaultAutoArchiveDuration { old, new } => {
            format_numeric_change_operation!("Archive duration", "h", old, new, |v| v / 60)
        }
        Change::Bitrate { old, new } => {
            format_numeric_change_operation!("Bitrate", "kbps", old, new, |v| v / 1000)
        }

        Change::Type { old, new } => match (old, new) {
            (Some(old), Some(new)) => format!(
                "- **Type:** `{}` ➜ `{}`",
                format_channel_type(old),
                format_channel_type(new)
            ),
            (_, Some(new)) => format!("- **Type:** `{}`", format_channel_type(new)),
            _ => return None,
        },

        // TODO
        Change::PermissionOverwrites { old, new } => match (old, new) {
            (_, Some(new)) => format_permission_override_channel(new),
            (Some(old), _) => format_permission_override_channel(old),
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
        EntityType::Int(entity_type) => ChannelType::from(*entity_type as u8).name().to_string(),
        _ => "unknown".to_string(),
    }
}

fn format_flags_diff(old: &u64, new: &u64) -> Option<String> {
    let new_flags = ChannelFlags::from_bits(*new);
    let changed_flags = ChannelFlags::from_bits(old ^ new);

    let Some(new_flags) = new_flags else {
        return None;
    };
    let Some(changed_flags) = changed_flags else {
        return None;
    };

    let mut result = Vec::new();

    for (name, flag) in changed_flags.iter_names() {
        result.push(format!("- **{name}**: `{}`", flag.intersects(new_flags)));
    }

    if result.is_empty() {
        return None;
    }

    Some(result.join("\n"))
}

fn format_flags(flags: &u64) -> Option<String> {
    let Some(flags) = ChannelFlags::from_bits(*flags) else {
        return None;
    };

    let mut result = Vec::new();

    for (name, _flag) in flags.iter_names() {
        result.push(format!("- **{name}**: `true`"));
    }

    if result.is_empty() {
        return None;
    }

    Some(result.join("\n"))
}

fn format_permission_override_channel(permission_overrides: &Vec<PermissionOverwrite>) -> String {
    let mut result = Vec::new();
    for permission in permission_overrides {
        let permission_line = match permission.kind {
            PermissionOverwriteType::Role(role_id) => {
                format!("- **Permissions for role** <@&{role_id}>")
            }
            PermissionOverwriteType::Member(user_id) => {
                format!("- **Permissions for user** <@{user_id}>")
            }
            _ => "Invalid permission".to_string(),
        };

        result.push(permission_line);
        result.push(format_permission_override(
            permission.allow,
            permission.deny,
        ));
    }
    result.join("\n")
}

fn format_permission_override(allow: Permissions, deny: Permissions) -> String {
    let combined_perms = allow | deny;

    let mut result = Vec::new();

    for perm in combined_perms.iter() {
        result.push(format!("  - {perm}: {}", perm_to_icon(allow, deny, perm),));
    }

    if result.is_empty() {
        return "  - *none*".to_string();
    }

    result.join("\n")
}

fn format_permission_override_change(
    old_allow: Permissions,
    new_allow: Permissions,
    old_deny: Permissions,
    new_deny: Permissions,
) -> String {
    let perms_difference = (old_allow ^ new_allow) | (old_deny ^ new_deny);

    let mut result = Vec::new();

    for perm in perms_difference.iter() {
        result.push(format!(
            "  - {perm}: {} ➜ {}",
            perm_to_icon(old_allow, old_deny, perm),
            perm_to_icon(new_allow, new_deny, perm),
        ));
    }

    if result.is_empty() {
        return "  - *none*".to_string();
    }

    result.join("\n")
}

fn perm_to_icon(allow: Permissions, deny: Permissions, perm: Permissions) -> &'static str {
    if perm.intersects(allow) {
        return "✅";
    }
    if perm.intersects(deny) {
        return "❌";
    }
    "`╱`"
}
