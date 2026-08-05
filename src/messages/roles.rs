use serenity::all::{
    AuditLogEntry, Change, Colour, Context, CreateEmbed, CreateMessage, GuildId, Permissions, RoleAction, RoleId, User, audit_log::Action,
};

use crate::{format_boolean_change, format_string_change, messages::{utils::{build_embed_author, format_role, format_user}}};

pub async fn build_role_message(
    entry: AuditLogEntry,
    user: Option<User>,
    guild_id: GuildId,
    ctx: &Context,
) -> Option<CreateMessage> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target role id provided");
        return None;
    };

    let user_str = format_user(&user, entry.user_id);

    let role_id = RoleId::new(target_id.get());
    let role = guild_id.role(&ctx, role_id).await.ok();
    let role_str = format_role(&role, role_id);

    let (action, colour) = match entry.action {
        Action::Role(RoleAction::Create) => ("created", Colour::new(0x00FF00)),
        Action::Role(RoleAction::Delete) => ("deleted", Colour::new(0xFF0000)),
        Action::Role(RoleAction::Update) => {
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
            .filter_map(build_role_change_line)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let embed_author = build_embed_author(&user, entry.user_id);
    let message = format!("{user_str} **{action} role** {role_str}\n\n{changes}");
    let title = format!("ROLE {}", action.to_uppercase());

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

fn build_role_change_line(change: &Change) -> Option<String> {
    Some(match change {
        Change::Name { old, new } => format_string_change!("Name", old, new),
        Change::Hoist { old, new } => format_boolean_change!("Show sepatately", old, new),
        Change::Mentionable { old, new } => format_boolean_change!("Pingable", old, new),
        Change::UnicodeEmoji { old, new } => format_string_change!("icon", old, new),

        Change::Permissions { old, new }  =>  match (old, new) {
            (Some(old), Some(new)) => return format_permission_change(old, new),
            (None, Some(new)) => return format_permission(new),
            (Some(old), None) => return format_permission(old),
            _ => return None
        },
        
        Change::Color { old, new } => return None,
        Change::Position { old, new } => match (old, new) {
            (Some(old), Some(new)) => if new > old {
                "- **Rank changed:** 🠉"
            } else {
                "- **Rank changed:** 🠋"
            },
            (None, Some(new)) => "- **Rank changed**",
            _ => return None

        }.to_string(),

        _ => return None,
    })
}

fn format_permission_change(old: &Permissions, new: &Permissions) -> Option<String> {
    let new = *new;
    let perms_difference = *old ^ new;

    let mut result = Vec::new();

    result.push("- **Permissions:**".to_string());

    for perm in perms_difference.iter(){
        result.push(format!("  - {perm}: {}", if perm.intersects(new) {
            "✅"
        } else {
            "`╱`"
        }));
    }

    if result.is_empty(){
        return None;
    }
    
    Some(result.join("\n"))
}

fn format_permission(perm: &Permissions) -> Option<String> {
    let mut result = Vec::new();

    result.push("- **Permissions:**".to_string());

    for perm in perm.iter(){
        result.push(format!("  - {perm}: ✅"));
    }

    if result.is_empty(){
        return None;
    }
    
    Some(result.join("\n"))
}