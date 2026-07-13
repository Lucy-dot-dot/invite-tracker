use humantime::format_duration;
use serenity::all::{Colour, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage, InviteCreateEvent, Member, User};
use time::{OffsetDateTime};

use super::datastructures::UsedInvite;

pub fn build_join_message(new_member: &Member, join_amount: i32, last_known_join: i64, used_invite: Option<&UsedInvite>) -> CreateMessage {
    let user_id = new_member.user.id.get();
    let account_created = new_member.user.id.created_at().unix_timestamp();
    let now = OffsetDateTime::now_utc().unix_timestamp();

    let account_created_ago_string = format_duration(std::time::Duration::new((now - account_created) as u64, 0)).to_string();

    // Suspicious-join indicators. Each pushes a human-readable reason; if any are present the
    // embed is recoloured amber and a "Suspicious" field is added so it stands out in the log.
    const NEW_ACCOUNT_THRESHOLD_SECS: i64 = 48 * 60 * 60;
    let mut reasons: Vec<String> = Vec::new();
    if now - account_created < NEW_ACCOUNT_THRESHOLD_SECS {
        reasons.push(format!("Account younger than 48h (<t:{account_created}:R>)"));
    }
    if let Some(until) = new_member.unusual_dm_activity_until {
        if until.unix_timestamp() > now {
            let until_ts = until.unix_timestamp();
            reasons.push(format!("Unusual DM activity flagged (until <t:{until_ts}:f>)"));
        }
    }

    if new_member.user.global_name.is_none() {
        reasons.push("No display name set".to_string());
    }
    let is_suspicious = !reasons.is_empty();

    // Only meaningful on a rejoin: on a first-time join prev_last_join == now, which would
    // misleadingly render as "just now".
    let last_known_join_line = if join_amount > 0 {
        format!("\n*Last known join <t:{last_known_join}:R>*\n")
    } else {
        String::new()
    };

    let invite_info = match used_invite {
        Some(inv) => format!(
            "- **Code:** `{code}` ({n_uses} uses)\n\
             - **Invited by:** <@{inviter_id}> ({inviter_name}) <t:{invite_created}:R>\n",
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
        {last_known_join_line}\n\
         **Account created:**\n\
         <t:{account_created}:f>\n\
         *(`{account_created_ago_string}` at time of joining)*\n\n\
         **Invite Info:**\n\
         {invite_info}",
    );

    let avatar_url = new_member.avatar_url().unwrap_or_else(|| new_member.user.face());
    let embed_author = CreateEmbedAuthor::new(username).icon_url(&avatar_url);
    let embed_footer = CreateEmbedFooter::new(format!("JOINED {user_id}"));

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("MEMBER JOINED")
        .color(if is_suspicious {
            Colour::new(0xFFA500)
        } else {
            Colour::new(0x00FF00)
        })
        .description(embed_description)
        .thumbnail(&avatar_url)
        .field("Display Name", new_member.display_name().to_string(), true)
        .field("Username", new_member.user.name.clone(), true)
        .field("Rejoins", join_amount.to_string(), true);

    if is_suspicious {
        embed = embed.field("Suspicious", reasons.join("\n"), false);
    }

    let embed = embed.footer(embed_footer);

    CreateMessage::new().embed(embed)
}

pub fn build_leave_message(user: &User, last_join: Option<i64>) -> CreateMessage {
    let user_id = user.id.get();
    let username = &user.name;

    let membership = match last_join {
        Some(ts) => {
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let formatted_member_age = format_duration(std::time::Duration::new((now - ts) as u64, 0)).to_string();
            format!("**Joined** <t:{ts}:f>\n\
                    **Was member for** `{formatted_member_age}`")
        },
        None => "*Unknown - no join record found.*".to_string(),
    };

    let embed_description = format!(
        "<@{user_id}> ({username})\n\n\
         **Was member for:**\n`{membership}`",
    );

    let avatar_url = user.face();
    let embed_author = CreateEmbedAuthor::new(&user.name).icon_url(&avatar_url);
    let embed_footer = CreateEmbedFooter::new(format!("LEFT {user_id}"));

    let embed = CreateEmbed::new()
        .author(embed_author)
        .title("MEMBER LEFT")
        .color(Colour::new(0xFF0000))
        .description(embed_description)
        .thumbnail(&avatar_url)
        .footer(embed_footer);

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
        format!("`{duration}`\n\
                **Expires:** <t:{expires_at}:f>",
            duration = format_duration(std::time::Duration::new(data.max_age as u64, 0)).to_string())
    };
    let max_uses = if data.max_uses == 0{
        "**∞**".to_string()
    } else {
        data.max_uses.to_string()
    };

    let embed_description = format!(
        "<@{inviter_id}> ({inviter_name})\n\n\
         **Code:** `{code}`\n\
         **Created:** <t:{created}:f>\n\
         **Duration:** {expiry}\n\
         **Max uses:** {max_uses}",
        code = data.code,
    );

    let mut embed_author = CreateEmbedAuthor::new(&inviter_name);
    if let Some(url) = &avatar_url {
        embed_author = embed_author.icon_url(url);
    }
    let embed_footer = CreateEmbedFooter::new(format!("INV_CREATED {inviter_id}"));

    let mut embed = CreateEmbed::new()
        .author(embed_author)
        .title("INVITE CREATED")
        .color(Colour::new(0x00AAFF))
        .description(embed_description)
        .footer(embed_footer);
    if let Some(url) = &avatar_url {
        embed = embed.thumbnail(url);
    }

    CreateMessage::new().embed(embed)
}