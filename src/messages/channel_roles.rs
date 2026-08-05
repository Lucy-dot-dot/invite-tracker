use serenity::all::{
    AuditLogEntry, Change, ChannelAction, ChannelId, ChannelType, Colour, Context, CreateEmbed,
    CreateMessage, EntityType, User, audit_log::Action,
};

use crate::messages::utils::{build_embed_author, format_channel, format_user};

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
    let message = format!("{user_str} **{action}** {channel}\n\n{changes}");
    let title = format!("CHANNEL {}", action.to_uppercase());

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

fn build_channel_change_line(change: &Change) -> Option<String> {
    Some(match change {
        Change::Bitrate { old, new } => match (old, new) {
            (Some(old), Some(new)) => {
                format!("- **Bitrate:** `{}` 🠞 `{} kbps`", old / 1000, new / 1000)
            }
            (None, Some(new)) => format!("- **Bitrate:** `{} kbps`", new / 1000),
            (Some(old), None) => format!("- **Bitrate reset:** *was* `{} kbps`", old / 1000),
            _ => return None,
        },
        Change::Type { old, new } => match (old, new) {
            (Some(old), Some(new)) => format!(
                "- **Type:** `{}` 🠞 `{}`",
                format_channel_type(old),
                format_channel_type(new)
            ),
            (_, Some(new)) => format!("- **Type:** `{}`", format_channel_type(new)),
            _ => return None,
        },
        Change::UserLimit { old, new } => match (old, new) {
            (None, Some(0)) => return None,
            (None, Some(new)) | (Some(0), Some(new)) => format!("- **User limit:** `{new}`"),
            (Some(old), None) | (Some(old), Some(0)) => {
                format!("- **Slowmode disabled:** *was* `{old}s`")
            }
            (Some(old), Some(new)) => format!("- **Slowmode:** `{old}` 🠞 `{new}`",),
            _ => return None,
        },
        Change::Name { old, new } => match (old, new) {
            (Some(old), Some(new)) => format!("- **Name:** \"{old}\" 🠞 \"{new}\"",),
            (None, Some(new)) => format!("- **Name:** \"{new}\""),
            (Some(old), None) => format!("- **Name:** *was* \"{old}\""),
            _ => return None,
        },
        Change::Description { old, new } | Change::Topic { old, new } => match (old, new) {
            (Some(old), Some(new)) => format!("- **Description:** \"{old}\" 🠞 \"{new}\"",),
            (None, Some(new)) => format!("- **Description:** \"{new}\""),
            (Some(old), None) => format!("- **Description removed:** *was* \"{old}\""),
            _ => return None,
        },
        Change::RateLimitPerUser { old, new } => match (old, new) {
            (None, Some(0)) => return None, // going from nothing to 0 means it was never enabled
            (None, Some(new)) | (Some(0), Some(new)) => format!("- **Slowmode:** `{new}s`"),
            (Some(old), None) | (Some(old), Some(0)) => {
                format!("- **Slowmode disabled:** *was* `{old}s`")
            }
            (Some(old), Some(new)) => format!("- **Slowmode:** `{old}` 🠞 `{new}s`",),
            _ => return None,
        },
        Change::Nsfw { old, new } => match (old, new) {
            (Some(old), None) => format!("- **NSFW:** *was {old}*"),
            (_, Some(new)) => format!("- **NSFW:** *{new}*"),
            _ => return None,
        },
        Change::Position { old: _, new } => match new {
            Some(_) => "- **Position changed**".to_string(),
            _ => return None,
        },

        // TODO
        Change::PermissionOverwrites { old: _, new: _ } => return None,
        Change::Flags { old: _, new: _ } => return None,

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
