use serenity::all::{
    Channel, ChannelId, Context, CreateEmbedAuthor, CreateMessage, Role, RoleId, User, UserId,
};
use tokio::time::{Duration, sleep};

const MSG_RETRY_INTERVAL: Duration = Duration::from_millis(200);

pub async fn send_message(message: CreateMessage, ctx: &Context, channel_id: ChannelId) {
    if let Err(_) = channel_id.send_message(ctx, message.clone()).await {
        sleep(MSG_RETRY_INTERVAL).await;

        if let Err(e) = channel_id.send_message(ctx, message).await {
            log::error!(
                "Unable to send message to channel {} after retry: {}",
                channel_id,
                e
            );
        }
    }
}

pub fn build_embed_author(user: &Option<User>, user_id: UserId) -> CreateEmbedAuthor {
    match (user, user_id) {
        (Some(user), _) => {
            let avatar_url = user.avatar_url().unwrap_or_else(|| user.face());
            CreateEmbedAuthor::new(&user.name).icon_url(avatar_url)
        }
        (None, user_id) => CreateEmbedAuthor::new(user_id.to_string()),
    }
}

pub fn build_embed_author_admin(
    user: &Option<User>,
    user_id: UserId,
    admin: &Option<User>,
) -> CreateEmbedAuthor {
    match (user, admin) {
        (Some(user), Some(admin)) => {
            let avatar_url = user.avatar_url().unwrap_or_else(|| user.face());
            let embed_author = format!("{} ➜ {}", &admin.name, &user.name);
            return CreateEmbedAuthor::new(embed_author).icon_url(avatar_url);
        }
        (None, Some(admin)) => {
            let avatar_url = admin.avatar_url().unwrap_or_else(|| admin.face());
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

pub fn format_channel(channel: Option<Channel>, channel_id: ChannelId) -> String {
    match channel {
        Some(Channel::Guild(gc)) => format!("<#{channel_id}>({})", gc.name),
        Some(Channel::Private(pc)) => {
            let recipient = pc.recipient;
            format!("DM with <@{}>({})", recipient.id.get(), recipient.name)
        }
        _ => format!("<#{channel_id}>"),
    }
}

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
            (Some(old), None) | (Some(old), Some(0)) => {
                format!("- **{NAME}:** *was* `{old}{UNIT}`").into()
            }
            (Some(old), Some(new)) => format!("- **{NAME}:** `{old}` ➜ `{new}{UNIT}`",).into(),
            _ => return None,
        }
    }};
}

#[macro_export]
macro_rules! format_numeric_change_operation {
    ($name:expr, $unit:expr, $old:expr, $new:expr, $operation: expr) => {{
        const NAME: &str = $name;
        const UNIT: &str = $unit;
        let op = $operation;
        let old = $old;
        let new = $new;

        match (old, new) {
            (None, Some(0)) => return None,
            (Some(0), None) => return None,
            (None, Some(new)) | (Some(0), Some(new)) => {
                format!("- **{NAME}:** `{}{UNIT}`", op(new)).into()
            }
            (Some(old), None) | (Some(old), Some(0)) => {
                format!("- **{NAME}:** *was* `{}{UNIT}`", op(old)).into()
            }
            (Some(old), Some(new)) => {
                format!("- **{NAME}:** `{}` ➜ `{}{UNIT}`", op(old), op(new)).into()
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
