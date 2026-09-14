use std::fmt::Display;

use serenity::{
    all::{Change, Channel, CreateEmbedAuthor, Permissions, Role, RoleId, User, UserId},
    small_fixed_array::FixedString,
};

pub fn build_embed_author(user: &Option<User>, user_id: UserId) -> CreateEmbedAuthor<'static> {
    match (user, user_id) {
        (Some(user), _) => {
            let avatar_url = user.face();
            CreateEmbedAuthor::new(user.name.to_string()).icon_url(avatar_url)
        }
        (None, user_id) => CreateEmbedAuthor::new(user_id.to_string()),
    }
}

pub fn build_embed_author_admin(
    user: &Option<User>,
    user_id: UserId,
    admin: &Option<User>,
) -> CreateEmbedAuthor<'static> {
    match (user, admin) {
        (Some(user), Some(admin)) => {
            let avatar_url = admin.face();
            let embed_author = format!("{} ➜ {}", &admin.name, &user.name);
            return CreateEmbedAuthor::new(embed_author).icon_url(avatar_url);
        }
        (None, Some(admin)) => {
            let avatar_url = admin.face();
            let embed_author = format!("{} ➜ {}", &admin.name, user_id);
            CreateEmbedAuthor::new(embed_author).icon_url(avatar_url)
        }
        _ => build_embed_author(user, user_id),
    }
}

pub fn format_user(user: &Option<User>, user_id: UserId) -> String {
    match user {
        Some(user) => format!("<@{user_id}>({})", &user.name),
        None => format!("<@{user_id}>"),
    }
}

pub fn format_role(role: &Option<Role>, role_id: RoleId) -> String {
    match role {
        Some(role) => format!("<@&{role_id}>({})", &role.name),
        None => format!("<@&{role_id}>"),
    }
}

pub fn format_channel(channel: &Option<Channel>, channel_id: impl Display) -> String {
    let (parent_id, name) = match channel {
        Some(Channel::Guild(gc)) => (gc.parent_id, gc.base.name.as_str()),
        Some(Channel::GuildThread(thread)) => (Some(thread.parent_id), thread.base.name.as_str()),
        _ => return format!("<#{channel_id}>"),
    };

    if let Some(parent_id) = parent_id {
        format!("<#{parent_id}>**>**<#{channel_id}>({name})")
    } else {
        format!("<#{channel_id}>({name})")
    }
}

// unlike generic format 0 is treated as the default value
#[macro_export]
macro_rules! format_numeric_change {
    ($name:expr, $unit:expr, $old:expr, $new:expr) => {{
        const NAME: &str = $name;
        const UNIT: &str = $unit;
        let old = $old;
        let new = $new;

        match (old, new) {
            (None, Some(0)) => return None,
            (Some(0), None) => return None,
            (None, Some(new)) | (Some(0), Some(new)) => {
                format!("- **{NAME}:** `{new}{UNIT}`").into()
            }
            (Some(old), None) => format!("- **{NAME}:** *was* `{old}{UNIT}`").into(),
            (Some(old), Some(0)) => format!("- **{NAME} disabled:** *was* `{old}{UNIT}`").into(),
            (Some(old), Some(new)) => format!("- **{NAME}:** `{old}` ➜ `{new}{UNIT}`",).into(),
            _ => return None,
        }
    }};
}

#[macro_export]
macro_rules! format_generic_change {
    ($name:expr, $old:expr, $new:expr, $operation: expr) => {{
        const NAME: &str = $name;
        let op = $operation;
        let old = $old;
        let new = $new;

        match (old, new) {
            (None, Some(new)) => {
                format!("- **{NAME}:** `{}`", op(new)).into()
            }
            (Some(old), None) => format!("- **{NAME}:** *was* `{}`", op(old)).into(),
            (Some(old), Some(new)) => {
                format!("- **{NAME}:** `{}` ➜ `{}`", op(old), op(new)).into()
            }
            _ => return None,
        }
    }};
}

#[macro_export]
macro_rules! format_generic_change_internal {
    ($name:expr, $old:expr, $new:expr, $operation:expr) => {{
        const NAME: &str = $name;
        let op = $operation;
        let old = $old;
        let new = $new;

        match (old, new) {
            (None, Some(new)) => Some(format!("- **{NAME}:** {}", op(new))),
            (Some(old), None) => Some(format!("- **{NAME}:** *was* {}", op(old))),
            (Some(old), Some(new)) if old != new => {
                Some(format!("- **{NAME}:** {} ➜ {}", op(old), op(new)))
            }
            _ => None, // Returns Option<String> directly to the caller, not the function!
        }
    }};
}

#[macro_export]
macro_rules! format_numeric_change_operation {
    ($name:expr, $old:expr, $new:expr, $operation: expr) => {{
        const NAME: &str = $name;
        let op = $operation;
        let old = $old;
        let new = $new;

        match (old, new) {
            (None, Some(0)) => return None,
            (Some(0), None) => return None,
            (None, Some(new)) | (Some(0), Some(new)) => {
                format!("- **{NAME}:** `{}`", op(new)).into()
            }
            (Some(old), None) => format!("- **{NAME}:** *was* `{}`", op(old)).into(),
            (Some(old), Some(0)) => {
                format!("- **{NAME} disabled:** *was* `{}`", op(old)).into()
            }
            (Some(old), Some(new)) => {
                format!("- **{NAME}:** `{}` ➜ `{}`", op(old), op(new)).into()
            }
            _ => return None,
        }
    }};
}

#[macro_export]
macro_rules! format_string_change {
    ($name:expr, $old:expr, $new:expr) => {{
        const NAME: &str = $name;
        let old = $old;
        let new = $new;

        match (old, new) {
            (Some(old), Some(new)) => format!("- **{NAME}:** \"{old}\" ➜ \"{new}\"").into(),
            (None, Some(new)) => format!("- **{NAME}:** \"{new}\"").into(),
            (Some(old), None) => format!("- **{NAME}:** *was* \"{old}\"").into(),
            _ => return None,
        }
    }};
}

pub fn get_reason(reason: &Option<FixedString>) -> &str {
    if let Some(reason) = reason
        && !reason.is_empty()
    {
        reason.trim()
    } else {
        "*No reason stated*"
    }
}

#[macro_export]
macro_rules! format_boolean_change {
    ($name:expr, $old:expr, $new:expr) => {{
        const NAME: &str = $name;
        let old = $old;
        let new = $new;

        match (old, new) {
            (Some(_), Some(new)) => format!("- **{NAME}:** `{new}`").into(),
            (None, Some(true)) => format!("- **{NAME}:** `true`").into(),
            (Some(true), None) => format!("- **{NAME}:** *was* `true`").into(),
            _ => return None,
        }
    }};
}

#[macro_export]
macro_rules! find_change {
    ($changes:expr, $pattern:path) => {
        $changes.iter().find(|c| matches!(c, $pattern { .. }))
    };
}

#[macro_export]
macro_rules! plural {
    ($n:expr) => {
        if $n == 1 { "" } else { "s" }
    };
}

#[macro_export]
macro_rules! unwrap_change {
    ($change:expr, $variant:path) => {
        match $change {
            Some($variant { old, new }) => (old, new),
            _ => (&None, &None),
        }
    };
}

pub fn get_name(change: Option<&Change>) -> Option<&str> {
    Some(match change {
        Some(Change::Name {
            old: _,
            new: Some(new),
        }) => new.as_str(),
        Some(Change::Name {
            old: Some(old),
            new: _,
        }) => old.as_str(),
        _ => return None,
    })
}

pub fn perm_to_icon(allow: Permissions, deny: Permissions, perm: Permissions) -> &'static str {
    if perm.intersects(allow) {
        return "✅";
    }
    if perm.intersects(deny) {
        return "❌";
    }
    "**`∕`**"
}
