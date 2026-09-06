use serenity::all::{
    AuditLogEntry, Change, Context, CreateEmbed, CreateMessage, GuildId, Permissions, RoleAction,
    RoleColours, RoleId, User, audit_log::Action,
};
use std::fmt::Write;

use crate::{
    format_boolean_change, format_generic_change, format_string_change,
    messages::{
        colours::*,
        utils::{build_embed_author, format_role, format_user, perm_to_icon},
    },
};

pub async fn build_role_message(
    entry: AuditLogEntry,
    user: Option<User>,
    guild_id: GuildId,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target role id provided");
        return None;
    };

    let Some(admin_id) = entry.user_id else {
        return None;
    };

    let (action, colour) = match entry.action {
        Action::Role(RoleAction::Create) => ("created", POSITIVE_COLOUR),
        Action::Role(RoleAction::Delete) => ("deleted", NEGATIVE_COLOUR),
        Action::Role(RoleAction::Update) => ("updated", EDIT_COLOUR),
        a => {
            log::error!(
                "Invalid action passed to channel message builder: {}",
                a.num()
            );
            ("unknown action", ERROR_COLOUR)
        }
    };

    let user_str = format_user(&user, admin_id);

    let action_string = if target_id.get() == guild_id.get() {
        "**updated permisisons for** @everyone".to_string()
    } else {
        let role_id = RoleId::new(target_id.get());
        let role = guild_id.role(&ctx.http, role_id).await.ok();
        format!("**{action} role** {}", format_role(&role, role_id))
    };

    let changes = entry
        .changes
        .iter()
        .filter_map(build_role_change_line)
        .collect::<Vec<_>>()
        .join("\n");

    let embed_author = build_embed_author(&user, admin_id);
    let message = format!("{user_str} {action_string}\n\n{changes}");
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
        Change::Hoist { old, new } => format_boolean_change!("Hoisted", old, new),
        Change::Mentionable { old, new } => format_boolean_change!("Pingable", old, new),
        Change::UnicodeEmoji { old, new } => format_string_change!("Icon", old, new),
        Change::Colors { old, new } => {
            format_generic_change!("Colour", old, new, format_role_colours)
        }

        Change::Permissions { old, new } => match (old, new) {
            (Some(old), Some(new)) => format_permission_change(old, new),
            (None, Some(new)) => format_permission(new),
            (Some(old), None) => format_permission(old),
            _ => return None,
        },

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

        _ => return None,
    })
}

fn format_role_colours(colour: &RoleColours) -> String {
    if colour.secondary_colour.is_none() && colour.tertiary_colour.is_none() {
        return format!("`#{}`", colour.primary_colour.hex());
    }
    if colour.tertiary_colour.is_none()
        && let Some(secondary) = colour.secondary_colour
    {
        return format!(
            "*Gradient* `#{}` `#{}`",
            colour.primary_colour.hex(),
            secondary.hex()
        );
    }

    "*Holographic*".to_string()
}

fn format_permission_change(old: &Permissions, new: &Permissions) -> String {
    let new = *new;
    let perms_difference = *old ^ new;

    let mut result = "- **Permissions:**\n ".to_string();

    for perm in perms_difference.iter() {
        writeln!(
            &mut result,
            "  - {perm}: {}",
            perm_to_icon(new, Permissions::empty(), perm)
        )
        .unwrap();
    }

    result.pop();
    result
}

fn format_permission(perm: &Permissions) -> String {
    if perm.is_empty() {
        return "- **Permissions:** *none*".to_string();
    }

    let mut result = "- **Permissions:**\n".to_string();

    for perm in perm.iter() {
        writeln!(&mut result, "  - {perm}: ✅").unwrap();
    }

    result.pop();
    result
}
