use serenity::all::{
    Channel, ChannelId, Colour, CreateEmbed, CreateEmbedAuthor, CreateMessage, GuildId, MessageId, User, UserId,
};
use time::OffsetDateTime;

use crate::messages::format_time::format_time_diff;
use crate::messages::utils::{build_embed_author, build_embed_author_admin, format_channel, format_user};


fn build_message_info(
    user: &Option<User>,
    user_id: Option<UserId>,
) -> String {
    match (user, user_id) {
        (Some(user), _) => {
            format!("**Message by** <@{}>({})", user.id, user.name)
        }
        (None, Some(id)) => {
            format!("**Message by** <@{id}>")
        }
        (None, None) => {
            "**Unknown message** ".to_string()
        }
    }
}

pub fn build_edited_message(
    user: Option<User>,
    user_id: UserId,
    channel: Option<Channel>,
    channel_id: ChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: String,
    edits: i32,
) -> CreateMessage {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, Some(user_id));
    let embed_author = build_embed_author(&user, user_id);

    let formatted_channel = format_channel(channel, channel_id);

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
        .color(Colour::new(0xFFAA00))
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

pub fn build_deleted_message(
    user: Option<User>,
    user_id: Option<UserId>,
    deleter: Option<User>,
    deleter_id: Option<UserId>,
    channel: Option<Channel>,
    channel_id: ChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: Option<String>,
    attachments: Option<String>,
    edits: i32,
) -> CreateMessage {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, user_id);

    let embed_author = if let Some(user_id) = user_id  {
        build_embed_author_admin(&user, user_id, &deleter)
    } else {
        CreateEmbedAuthor::new("unknown author")
    };

    let deleter_info = if let Some(deleter_id) = deleter_id {
        format!("\n**Deleted by** {}", format_user(&deleter, deleter_id))
    } else {
        String::new()
    };

    let formatted_channel = format_channel(channel, channel_id);

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

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .color(Colour::new(0xFF0000))
        .description(embed_description);

    let mut message = CreateMessage::new();

    if let Some(attachments) = attachments {
        let attachments: Vec<&str> = attachments.split("\n").collect();

        if !attachments.is_empty() {
            // First attachment goes in the main embed
            embed = embed.thumbnail(attachments[0]);
            message = message.embed(embed);

            // Any additional attachments get their own embeds
            for attachment in attachments.iter().skip(1) {
                let extra_embed = CreateEmbed::new()
                    .thumbnail(*attachment)
                    .color(Colour::new(0xFF0000));
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
    channel_id: ChannelId,
    count: usize,
) -> CreateMessage {
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

    let formatted_channel = format_channel(channel, channel_id);

    let embed_description = format!(
        "**{count} messages deleted in** {formatted_channel}\n\n\
         {content}"
    );

    let embed = CreateEmbed::new()
        .title("BULK MESSAGE DELETE")
        .color(Colour::new(0xFF0000))
        .description(embed_description);

    CreateMessage::new().embed(embed)
}
