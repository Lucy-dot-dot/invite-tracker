use serenity::all::{
    AuditLogEntry, Change, Context, CreateEmbed, CreateMessage, EmojiAction, StickerAction,
    StickerFormatType, StickerId, User, audit_log::Action,
};

use crate::{
    find_change, format_string_change,
    messages::{
        colours::*,
        utils::{build_embed_author, format_user, get_name},
    },
};

pub async fn build_sticker_message(
    entry: AuditLogEntry,
    user: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(sticker_id) = entry.target_id else {
        log::error!("No target sticker id provided");
        return None;
    };

    let Some(user_id) = entry.user_id else {
        return None;
    };

    let user_str = format_user(&user, user_id);

    let sticker_id = StickerId::new(sticker_id.get());
    let sticker = sticker_id.to_sticker(&ctx.http).await.ok();

    let (action, colour) = match entry.action {
        Action::Sticker(StickerAction::Create) => ("created", POSITIVE_COLOUR),
        Action::Sticker(StickerAction::Delete) => ("deleted", NEGATIVE_COLOUR),
        Action::Sticker(StickerAction::Update) => ("updated", EDIT_COLOUR),
        a => {
            log::error!(
                "Invalid action passed to sticker message builder: {}",
                a.num()
            );
            ("unknown action", ERROR_COLOUR)
        }
    };

    let changes = entry
        .changes
        .iter()
        .filter_map(build_sticker_change_line)
        .collect::<Vec<_>>()
        .join("\n");

    let embed_author = build_embed_author(&user, user_id);
    let message = format!("{user_str} **{action} a sticker**\n\n{changes}");
    let title = format!("STICKER {action}").to_uppercase();

    let mut embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    if let Some(sticker) = sticker
        && let Some(url) = sticker.image_url()
    {
        embed = embed.thumbnail(url, None);
    }

    Some(CreateMessage::new().embed(embed))
}

pub fn build_emoji_message(
    entry: AuditLogEntry,
    user: Option<User>,
) -> Option<CreateMessage<'static>> {
    let Some(emoji_id) = entry.target_id else {
        log::error!("No emoji sticker id provided");
        return None;
    };

    let Some(user_id) = entry.user_id else {
        return None;
    };

    let user_str = format_user(&user, user_id);

    let (action, colour) = match entry.action {
        Action::Emoji(EmojiAction::Create) => ("created", POSITIVE_COLOUR),
        Action::Emoji(EmojiAction::Delete) => ("deleted", NEGATIVE_COLOUR),
        Action::Emoji(EmojiAction::Update) => ("updated", EDIT_COLOUR),
        a => {
            log::error!(
                "Invalid action passed to emoji message builder: {}",
                a.num()
            );
            ("unknown action", ERROR_COLOUR)
        }
    };

    let name_change = find_change!(entry.changes, Change::Name);

    let emoji_name = get_name(name_change).unwrap_or("unknown_name");

    let Some(name_change) = name_change else {
        return None;
    };
    let name_line = build_sticker_change_line(name_change).unwrap_or(String::new());

    // only show emoji if it's not a delete message
    let emoji_mention = if let Action::Emoji(EmojiAction::Delete) = entry.action {
        String::new()
    } else {
        format!("# - <:{emoji_name}:{emoji_id}>")
    };

    let embed_author = build_embed_author(&user, user_id);
    let message = format!(
        "{user_str} **{action} an emoji**: \n\
         {name_line}\n\
         {emoji_mention}"
    );
    let title = format!("EMOJI {action}").to_uppercase();

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

fn build_sticker_change_line(change: &Change) -> Option<String> {
    Some(match change {
        Change::Name { old, new } => format_string_change!("Name", old, new),
        Change::Tags { old, new } => format_string_change!("Emoji", old, new),
        Change::Description { old, new } => format_string_change!("Description", old, new),
        Change::FormatType { old, new } => match (old, new) {
            (_, Some(new)) => sticker_format_to_string(new),
            (Some(old), _) => sticker_format_to_string(old),
            _ => return None,
        },
        _ => return None,
    })
}

fn sticker_format_to_string(format: &StickerFormatType) -> String {
    let format_name = match format {
        &StickerFormatType::Png => "PNG",
        &StickerFormatType::Apng => "A-PNG",
        &StickerFormatType::Gif => "GIF",
        &StickerFormatType::Lottie => "Lottie",
        _ => "unknown",
    };

    format!("- **Format:** `{format_name}`")
}
