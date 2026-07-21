use humantime::format_duration;
use serenity::all::{
    Channel, ChannelId, Colour, CreateEmbed, CreateEmbedAuthor, CreateMessage, GuildId, InviteCreateEvent, Member, MessageId, User, UserId
};
use time::OffsetDateTime;

use super::datastructures::UsedInvite;


fn build_author_info(
    user: Option<User>,
    user_id: Option<UserId>,
) -> (String, CreateEmbedAuthor) {
    match (user, user_id) {
            (Some(user), _) => {
                let msg = format!(
                    "Message by <@{}>({})",
                    user.id.get(),
                    user.name
                );
                let avatar_url = user.avatar_url().unwrap_or_else(|| user.face());
                let author = CreateEmbedAuthor::new(user.name).icon_url(avatar_url);
                (msg, author)
            }
            (None, Some(id)) => {
                let msg = format!("Message by <@{id}>");
                let author = CreateEmbedAuthor::new(id.to_string());
                (msg, author)
            }
            (None, None) => {
                let msg = "Unknown message".to_string();
                let author = CreateEmbedAuthor::new("unknown");
                (msg, author)
            }
        }
}
fn format_channel(
    channel: Option<Channel>,
    channel_id: ChannelId
) -> String {
    match channel {
        Some(Channel::Guild(gc)) => format!("<#{channel_id}>({})", gc.name),
        Some(Channel::Private(pc)) => {
            let recipient = pc.recipient;
            format!("DM with <@{}>({})", recipient.id.get(), recipient.name)
        }
        Some(_) => "unknown channel".to_string(),
        None => format!("<#{channel_id}>"),
}
}

pub fn build_join_message(
    new_member: &Member,
    join_amount: i32,
    last_known_join: i64,
    used_invite: Option<&UsedInvite>,
) -> CreateMessage {
    let user_id = new_member.user.id.get();
    let account_created = new_member.user.id.created_at().unix_timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let account_age = now - account_created;

    let account_created_ago_string =
        format_duration(std::time::Duration::new(account_age as u64, 0)).to_string();

    // Suspicious-join indicators. Each pushes a human-readable reason; if any are present the
    // embed is recoloured amber and a "Suspicious" field is added so it stands out in the log.
    const NEW_ACCOUNT_THRESHOLD_SECS: i64 = 48 * 60 * 60;
    let mut suspicions: Vec<String> = Vec::new();
    if account_age < NEW_ACCOUNT_THRESHOLD_SECS {
        let h = account_age / (60 * 60);
        let m = (account_age / 60) % 60;
        suspicions.push(format!("## Account younger than 48h ({h}h {m}m)"));
    }
    if let Some(until) = new_member.unusual_dm_activity_until {
        if until.unix_timestamp() > now {
            let until_ts = until.unix_timestamp();
            suspicions.push(format!(
                "Unusual DM activity flagged (until <t:{until_ts}:R>)"
            ));
        }
    }
    if new_member.user.avatar.is_none() {
        suspicions.push("## No avatar set".to_string());
    }

    if new_member.user.global_name.is_none() {
        suspicions.push("No display name set".to_string());
    }
    let is_suspicious = !suspicions.is_empty();

    // Only meaningful on a rejoin: on a first-time join prev_last_join == now, which would
    // misleadingly render as "just now".
    let last_known_join_line = if join_amount > 0 {
        format!("\n*Last known join <t:{last_known_join}:R>*")
    } else {
        String::new()
    };

    let invite_info = match used_invite {
        Some(inv) => format!(
            "- **Code:** `{code}` ({n_uses} uses)\n\
             - **By** <@{inviter_id}> ({inviter_name}) <t:{invite_created}:R>\n",
            code = inv.code,
            inviter_id = inv.inviter_id,
            inviter_name = inv.inviter_name,
            invite_created = inv.created_at,
            n_uses = inv.uses
        ),
        None => "*Could not determine which invite was used.*".to_string(),
    };

    let username = &new_member.user.name;
    // `<@id>` renders as a real, clickable user ping (right-click -> ban), not just plain text.
    let embed_description = format!(
        "<@{user_id}> ({username})\
        {last_known_join_line}\n\n\
         **Account created:**\n\
         <t:{account_created}:f>\n\
         *(`{account_created_ago_string}` at time of joining)*\n\n\
         **Invite Info:**\n\
         {invite_info}",
    );

    let avatar_url = new_member
        .avatar_url()
        .unwrap_or_else(|| new_member.user.face());
    let embed_author = CreateEmbedAuthor::new(username).icon_url(&avatar_url);

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title(if is_suspicious {
            "⚠️MEMBER JOINED"
        } else {
            "MEMBER JOINED"
        })
        .color(if is_suspicious {
            Colour::new(0xFFA500)
        } else {
            Colour::new(0x00FF00)
        })
        .description(embed_description)
        .thumbnail(&avatar_url)
        .field("Display Name", new_member.display_name().to_string(), true)
        .field("Username", new_member.user.name.clone(), true);

    if join_amount > 0 {
        embed = embed.field("Rejoins", join_amount.to_string(), true);
    }

    if is_suspicious {
        embed = embed.field("⚠️Suspicions:", suspicions.join("\n"), false);
    }

    CreateMessage::new().embed(embed)
}

pub fn build_leave_message(user: &User, last_join: Option<i64>) -> CreateMessage {
    let user_id = user.id.get();
    let username = &user.name;

    let membership = match last_join {
        Some(ts) => {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let formatted_member_age =
                format_duration(std::time::Duration::new((now - ts) as u64, 0)).to_string();
            format!(
                "**Joined:** <t:{ts}:f>\n\
                    **Was member for:** `{formatted_member_age}`"
            )
        }
        None => "*no join record found.*".to_string(),
    };

    let embed_description = format!(
        "<@{user_id}> ({username})\n\n\
         {membership}",
    );

    let avatar_url = user.face();
    let embed_author = CreateEmbedAuthor::new(&user.name).icon_url(&avatar_url);

    let embed = CreateEmbed::new()
        .author(embed_author)
        .title("MEMBER LEFT")
        .color(Colour::new(0xFF0000))
        .description(embed_description)
        .thumbnail(&avatar_url);

    CreateMessage::new().embed(embed)
}

pub fn build_invite_message(data: &InviteCreateEvent) -> CreateMessage {
    let (inviter_id, inviter_name, avatar_url) = match &data.inviter {
        Some(user) => (user.id.get(), user.name.clone(), Some(user.face())),
        None => (0, "unknown".to_string(), None),
    };

    let created = data.created_at.unix_timestamp();
    let expiry = if data.max_age == 0 {
        "**∞**".to_string()
    } else {
        let expires_at = created + data.max_age as i64;
        format!(
            "`{duration}`\n\
                **Expires:** <t:{expires_at}:R>",
            duration =
                format_duration(std::time::Duration::new(data.max_age as u64, 0)).to_string()
        )
    };
    let max_uses = if data.max_uses == 0 {
        "**∞**".to_string()
    } else {
        data.max_uses.to_string()
    };

    let embed_description = format!(
        "<@{inviter_id}> ({inviter_name})\n\n\
         **Code:** `{code}`\n\
         **Max uses:** {max_uses}\n\
         **Duration:** {expiry}",
        code = data.code,
    );

    let mut embed_author = CreateEmbedAuthor::new(&inviter_name);
    if let Some(url) = &avatar_url {
        embed_author = embed_author.icon_url(url);
    }

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("INVITE CREATED")
        .color(Colour::new(0x00AAFF))
        .description(embed_description);
    if let Some(url) = &avatar_url {
        embed = embed.thumbnail(url);
    }

    CreateMessage::new().embed(embed)
}


pub fn build_edited_message(
    user: Option<User>,
    user_id: Option<UserId>,
    channel: Option<Channel>,
    channel_id: ChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: String,
    edits: i32,
) -> CreateMessage {
    let created = message_id.created_at().unix_timestamp();

    let (message_author, embed_author) = build_author_info(user, user_id);

    let channel = format_channel(channel, channel_id);

    let edited_string = match edits {
        1.. => format!(" (edited {edits} times)"),
        _ => "".to_string()
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel}/{message_id}");

    let embed_description = format!(
        "**{message_author} edited in {channel}**\n\
         {content}\n\n\
         -# Posted <t:{created}:f>{edited_string}\n\
         -# [Jump to message]({message_link})"
    );

    let embed = CreateEmbed::new()
        .author(embed_author)
        .title("MESSAGE EDITED")
        .color(Colour::new(0xFFAA00))
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

pub fn build_deleted_message(
    user: Option<User>,
    user_id: Option<UserId>,
    channel: Option<Channel>,
    channel_id: ChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: Option<String>,
    attachments: Option<String>,
    edits: i32
) -> CreateMessage {
    let created = message_id.created_at().unix_timestamp();

    let (message_author, embed_author) = build_author_info(user, user_id);

    let channel = format_channel(channel, channel_id);
    
    let content = match content {
        Some(content) => content,
        None => "*Message content not available*".to_string()
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let formatted_age =
        format_duration(std::time::Duration::new((now - created) as u64, 0)).to_string();

    let edited_string = match edits {
        0 => "".to_string(),
        1 => " (edited)".to_string(),
        _ =>  format!(" (edited {edits} times)")
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel}/{message_id}");

    let embed_description = format!(
        "**{message_author} deleted in {channel}**\n\
         {content}\n\n\
         -# Posted <t:{created}:f> up for `{formatted_age}`{edited_string}\n\
         -# [Jump to surrounding]({message_link})"
    );

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("MESSAGE DELETED")
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
    messages: Vec<(u64, String)>,
    channel: ChannelId,
    count: usize
) -> CreateMessage {

    let mut content = String::new();

    let mut current_user_id: u64 = 0;

    for (user_id, message) in messages {
        if user_id != current_user_id {
            content.push_str(&format!("-# **<@{user_id}>:**\n"));
            current_user_id = user_id;
        }

        let processed_message = message.replace('\n', " ");
        let processed_message = if processed_message.len() > 100 {
                format!("{}...", processed_message[..100].trim())
            } else {
                processed_message
            };
        content.push_str(&format!("-# • {processed_message}\n"));
    }

    let embed_description = format!(
        "**{count} messages deleted in <#{channel}>**\n\n\
         {content}"
    );

    let embed = CreateEmbed::new()
        .title("BULK MESSAGE DELETE")
        .color(Colour::new(0xFF0000))
        .description(embed_description);

    CreateMessage::new().embed(embed)
}