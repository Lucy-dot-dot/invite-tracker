use std::error::Error;

use serenity::all::{
    Channel, CreateAttachment, CreateEmbed, CreateEmbedAuthor, CreateMessage, GenericChannelId,
    GuildId, MessageId, User, UserId,
};
use serenity::futures::future::join_all;
use time::OffsetDateTime;

use crate::messages::colours::*;
use crate::messages::format_time::format_time_diff;
use crate::messages::utils::{
    build_embed_author, build_embed_author_admin, format_channel, format_user,
};

fn build_message_info(user: &Option<User>, user_id: Option<UserId>) -> String {
    match (user, user_id) {
        (Some(user), _) => {
            format!("**Message by** <@{}>({})", user.id, user.name)
        }
        (None, Some(id)) => {
            format!("**Message by** <@{id}>")
        }
        (None, None) => "**Unknown message** ".to_string(),
    }
}

pub fn build_edited_message(
    user: Option<User>,
    user_id: UserId,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: String,
    edits: i32,
) -> CreateMessage<'static> {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, Some(user_id));
    let embed_author = build_embed_author(&user, user_id);

    let formatted_channel = format_channel(&channel, channel_id);

    let edited_string = match edits {
        1 => "(edited once)".to_string(),
        2.. => format!(" (edited {edits} times)"),
        _ => String::new(),
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel_id}/{message_id}");

    let embed_description = format!(
        "**Edited message in** {formatted_channel}\n\
         {message_author} previous content**:**\n\n\
         {content}\n\n\
         -# Posted <t:{created}:f>{edited_string}\n\
         -# [Jump to message]({message_link})"
    );

    let embed = CreateEmbed::new()
        .author(embed_author)
        .color(EDIT_COLOUR)
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

pub async fn build_deleted_message(
    user: Option<User>,
    user_id: Option<UserId>,
    deleter: Option<User>,
    deleter_id: Option<UserId>,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: Option<String>,
    attachments: Option<String>,
    stickers: Option<String>,
    edits: i32,
) -> CreateMessage<'static> {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, user_id);

    let embed_author = if let Some(user_id) = user_id {
        build_embed_author_admin(&user, user_id, &deleter)
    } else {
        CreateEmbedAuthor::new("unknown author")
    };

    let deleter_info = if let Some(deleter_id) = deleter_id {
        format!("\n**Deleted by** {}", format_user(&deleter, deleter_id))
    } else {
        String::new()
    };

    let formatted_channel = format_channel(&channel, channel_id);

    let content = match content {
        Some(content) => content,
        None => "*Message content not available*".to_string(),
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let formatted_age = format_time_diff((now - created) as u64, 3);

    let edited_string = match edits {
        0 => String::new(),
        1 => " (edited)".to_string(),
        _ => format!(" (edited {edits} times)"),
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel_id}/{message_id}");

    let embed_description = format!(
        "**Deleted message in** {formatted_channel}\
         {deleter_info}\n\
         {message_author} **:**\n\n\
         {content}\n\n\
         -# Posted <t:{created}:f> up for `{formatted_age}`{edited_string}\n\
         -# [Jump to surrounding]({message_link})"
    );

    let mut embed: CreateEmbed<'_> = CreateEmbed::new()
        .author(embed_author)
        .color(NEGATIVE_COLOUR)
        .description(embed_description);

    /*// if an admin deletes the message, the pfp will be the admin's one
    if deleter_id.is_some() && let Some(user) = user {
        embed = embed.thumbnail(user.face(), None);
    }*/

    let mut message = reupload_attachements(attachments).await;

    if let Some(stickers) = stickers {
        let attachments: Vec<&str> = stickers.split("\n").collect();

        if !attachments.is_empty() {
            // First attachment goes in the main embed
            embed = embed.image(attachments[0].to_string(), None);
            message = message.embed(embed);

            // Any additional attachments get their own embeds
            for attachment in attachments.iter().skip(1) {
                let extra_embed = CreateEmbed::new()
                    .image((*attachment).to_string(), None)
                    .color(NEGATIVE_COLOUR);
                message = message.add_embed(extra_embed);
            }
            return message;
        }
    }

    message.embed(embed)
}

pub fn build_bulk_delete_message(
    messages: Vec<(UserId, Option<User>, Vec<String>)>,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    count: usize,
) -> CreateMessage<'static> {
    let mut content = String::new();

    for (user_id, user, user_messages) in messages {
        content.push_str(&match user {
            Some(user) => format!("<@{user_id}>({})", user.name),
            None => format!("<@{user_id}>"),
        });

        // if there are no messages do not put the colon
        if user_messages.len() == 0 {
            content.push_str("\n");
            continue;
        } else {
            content.push_str(":\n");
        }

        for message in user_messages {
            content.push_str(&format!("-# • {message}\n"));
        }
    }

    let formatted_channel = format_channel(&channel, channel_id);

    let embed_description = format!(
        "**{count} messages deleted in** {formatted_channel}\n\n\
         {content}"
    );

    let embed = CreateEmbed::new()
        .title("BULK MESSAGE DELETE")
        .color(NEGATIVE_COLOUR)
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

async fn reupload_attachements(attachments: Option<String>) -> CreateMessage<'static> {
    let mut builder = CreateMessage::new();

    let Some(attachments) = attachments else {
        return builder;
    };

    let download_futures = attachments.lines().map(|line| async move {
        let (url, filename) = line.split_once('|').unwrap_or((line, "unknown_attachment"));

        match download_single(url).await {
            Ok(bytes) => Some((bytes, filename.to_string())),
            Err(e) => {
                log::error!("Failed to fetch URL {}: {}", url, e);
                None
            }
        }
    });

    let results = join_all(download_futures).await;

    for (bytes, filename) in results.into_iter().flatten() {
        let attachment = CreateAttachment::bytes(bytes, filename);
        builder = builder.add_file(attachment);
    }

    builder
}
async fn download_single(url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    Ok(bytes.to_vec())
}
