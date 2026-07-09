use std::ops::Shr;
use std::str::FromStr;
use std::sync::Arc;
use serde::Deserialize;
use serenity::all::{ChannelId, Context, CreateMessage, Guild, GuildChannel, InviteCreateEvent, Member, Message, Ready, RoleId, UserId};
use serenity::Client;
use serenity::prelude::{EventHandler, GatewayIntents};
use sqlx::{PgPool, Row};
use time::{OffsetDateTime, UtcOffset};


mod db;

fn snowflake_to_timestamp(discord_id: u64) -> OffsetDateTime {
    let discord_epoch = OffsetDateTime::new_in_offset(time::Date::from_calendar_date(2015, time::Month::January, 1).unwrap(), time::Time::from_hms(0, 0, 0).unwrap(), UtcOffset::UTC);
    // 42 bit timestamp, so shifting it right by 22 bits gives you the timestamp
    let shifted = discord_id.shr(22);
    OffsetDateTime::from_unix_timestamp(discord_epoch.unix_timestamp() + shifted as i64).unwrap()
}


#[derive(Deserialize, Clone, Debug)]
struct Config {
    #[serde(default)]
    token: String,
    target_channel: u64
}

impl Config {
    fn resolve_token(&self) -> String {
        let trimmed = self.token.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        std::env::var("DISCORD_TOKEN")
            .expect("No token in config.toml and the DISCORD_TOKEN env var is not set")
    }
}

fn build_join_message(last_join: i64, join_amount: i32, new_member: Member) -> CreateMessage {
    let creation_time = snowflake_to_timestamp(new_member.user.id.get());

    let embed_description = format!(
        "**Account created:**\n\
    <t:{account_created}:f>\n\
    *(<t:{account_created}:R> at time of joining)*\n\n\
    **Invite Info:**\n\
    **Code:** DJs97jK3P\n\
    **Invited by:** gamxx10zz (1103596638875963462)\n\
    *Created <t:{invite_created}:R>*",
        account_created = creation_time,
        invite_created = invite_created_unix
    );

}

struct Handler {
    config: Arc<Config>,
    pool: PgPool,
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        if is_new.unwrap_or(false) {
            log::debug!("Guild {} is connected", guild.name);
            let existing = guild.invites(&ctx).await.unwrap();

            for invite in existing.iter() {
                sqlx::query("INSERT INTO invites (created_at, guild, inviter, max_age, max_usages) VALUES ($1, $2, $3, $4, $5)")
                    .bind(invite.created_at.unix_timestamp())
                    .bind(invite.guild.as_ref().map(|guild| guild.id).expect("Why does an invite don't have a guild?").get() as i64)
                    .bind(invite.inviter.as_ref().map(|content| content.id).unwrap_or(UserId::default()).get() as i64)
                    .bind(invite.max_age as i32)
                    .bind(invite.max_uses as i16)
                    .execute(&self.pool)
                    .await
                    .expect("Why should this fail?");
            }
            log::debug!("Guild is now ready to go")
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        log::debug!("Member {} joined", new_member.user.name);
        // get last join date first and then update later, during first join this is equal to the current date
        let result = sqlx::query("INSERT INTO joined_member (last_join, user_id, join_amount) VALUES ($1, $2, $3) ON CONFLICT DO UPDATE SET join_amount = join_amount + 1 RETURNING id, last_join, join_amount;")
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .bind(new_member.user.id.get() as i64)
            .bind(0)
            .fetch_one(&self.pool).await.unwrap();
        let id: sqlx::types::Uuid = result.get(0);
        let last_join: i64 = result.get(1);
        let join_amount: i32 = result.get(2);

        let guild = new_member.guild_id;
        let channels = guild.channels(&ctx).await.unwrap();
        let channel = ChannelId::new(self.config.target_channel);
        match channels.get(&channel) {
            Some(channel) => {
                let invite_creation_timestamp = sqlx::query("SELECT created_at FROM invites WHERE code = $1;")
                    .bind(new_member.)



                let msg = build_join_message(last_join, join_amount, new_member, invite_creation_timestamp);
                channel.send_message(&ctx, msg).await.expect("Why would anyone do this");

                sqlx::query("UPDATE joined_member SET last_join = $1 WHERE id = $2;")
                    .bind(last_join)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .expect("Why would this fail?");
            }
            None => {
                log::error!("Unable to find configured log channel {}", channel);
            }
        }

    }

    async fn invite_create(&self, ctx: Context, data: InviteCreateEvent) {
        let (id, name) = data.inviter.as_ref().map(|user| (user.id.get(), user.name.clone())).unwrap_or((0, String::from_str("unknown").unwrap()));
        log::debug!("Invite created by {} ({})", id, name);
        sqlx::query("INSERT INTO invites (created_at, guild, inviter, max_age, max_usages, code) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(data.created_at.unix_timestamp())
            .bind(data.guild_id.as_ref().map(|guild| guild.get()).expect("Why does an invite don't have a guild?") as i64)
            .bind(data.inviter.as_ref().map(|content| content.id).unwrap_or(UserId::default()).get() as i64)
            .bind(data.max_age as i32)
            .bind(data.max_uses as i16)
            .bind(&data.code)
            .execute(&self.pool)
            .await
            .expect("Why should this fail?");

        let channels = data.guild_id.unwrap().channels(&ctx).await.unwrap();
        let channel = ChannelId::new(self.config.target_channel);
        match channels.get(&channel) {
            Some(channel) => {
                channel.send_message(&ctx, CreateMessage::new().content("Invite created! Nice!")).await.expect("Why would anyone do this");
            }
            None => {
                log::error!("Unable to find configured log channel {}", channel);
            }
        }
    }

    async fn ready(&self, _ctx: Context, _ready: Ready) {
        log::info!("Bot is online and watching for invites coming in");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_module("tracing", log::LevelFilter::Off).init();

    let raw = std::fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&raw)?;
    let token = config.resolve_token();

    let pool = db::initialize_database_pool("").await?;

    let handler = Handler {
        config: Arc::new(config),
        pool,
    };

    let intents = GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_INVITES;

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;
    Ok(())
}