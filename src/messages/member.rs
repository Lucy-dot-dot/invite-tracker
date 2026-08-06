use serenity::all::{
    AuditLogEntry, Change, Colour, Context, CreateEmbed, CreateMessage, User, UserId,
};

use crate::messages::utils::{build_embed_author_admin, format_user};

pub async fn build_role_change_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    ctx: &Context,
) -> Option<CreateMessage> {
    if let Some(admin) = &admin
        && admin.bot
    {
        return None;
    };

    let Some(target_id) = entry.target_id else {
        return None;
    };

    // Ignore self-roles, like in onboarding or server guide
    if target_id.get() == entry.user_id.get() {
        return None;
    }

    let user_id = UserId::new(target_id.get());
    let user = user_id.to_user(&ctx).await.ok();

    let user_str = format_user(&user, user_id);
    let admin_str = format_user(&admin, entry.user_id);

    let embed_author = build_embed_author_admin(&user, user_id, &admin);

    let Some(changes) = entry.changes else {
        return None;
    };

    let added = changes
        .iter()
        .find(|&x| matches!(x, Change::RolesAdded { old: _, new: _ }));

    let removed = changes
        .iter()
        .find(|&x| matches!(x, Change::RolesRemove { old: _, new: _ }));

    let (title, header, colour) = match (added, removed) {
        (Some(_), None) => (
            "MEMBER ROLE ADDED",
            format!("{admin_str} **added roles to** {user_str}"),
            Colour::new(0x00FF00),
        ),
        (None, Some(_)) => (
            "MEMBER ROLE REMOVED",
            format!("{admin_str} **removed roles from** {user_str}"),
            Colour::new(0xFF0000),
        ),
        _ => (
            "ROLES UPDATED",
            format!("{admin_str} **updated roles for** {user_str}"),
            Colour::new(0xFFAA00),
        ),
    };

    let mut lines = Vec::new();

    if let Some(Change::RolesAdded {
        old: _,
        new: Some(new_roles),
    }) = added
    {
        lines.push("- **Roles added:**".to_string());
        for role in new_roles {
            lines.push(format!("  - <@&{}>({})", role.id, role.name));
        }
    }

    if let Some(Change::RolesRemove {
        old: _,
        new: Some(new_roles),
    }) = removed
    {
        lines.push("- **Roles removed:**".to_string());
        for role in new_roles {
            lines.push(format!("  - ~~<@&{}>({})~~", role.id, role.name));
        }
    }

    let message = format!("{header}\n\n{}", lines.join("\n"));

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}
