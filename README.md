# Discord Logging Bot

A moderation-focused logging bot that posts detailed, at-a-glance information whenever
someone joins, leaves, or an invite is created. It is designed to surface the things
moderators actually act on — who invited them, how old the account is, and whether the
join looks suspicious — right inside the log channel.

---

## What It Logs

All messages are sent to a single **log channel** that you choose. Each event is a
colour-coded embed so you can scan the channel quickly:

| Colour    | Meaning           |
|-----------|-------------------|
| **Green** | A member joined (no concerns) |
| **Amber** | A member joined **and was flagged suspicious** |
| **Red**   | A member left     |
| **Blue**  | An invite was created |

### Member Joined

Posted every time someone joins the server. Includes:

- **A clickable ping of the joining user** — right-click it to ban/kick straight from
  the log, no need to copy an ID.
- **Account created** — the exact date/time, plus a relative "how long ago" so you can
  spot freshly minted accounts at a glance.
- **Invite info** — the invite code used, who created that invite (as a ping + ID), and
  when the invite was created.
- **Display Name** and **Username** as separate fields.
- **Rejoins** — how many times this person has joined before (0 on a first-ever join).
- **Last known join** — shown only on rejoins, as a relative timestamp.
- The user's avatar as a thumbnail.

If the bot cannot determine which invite was used (rare), it says so explicitly rather
than guessing.

### Suspicious Join Detection

When a join trips one or more suspicion signals, the embed turns **amber** and gains a
**Suspicious** field listing every reason. The signals are:

| Signal | Meaning |
|--------|---------|
| Account younger than 24h | Brand-new account — common for alts and raiders. |
| Unusual DM activity | Discord itself has flagged this account for unusual DM behaviour. |
| No avatar set | The account still has the default Discord avatar. |
| No display name set | The account has never set a display name. |

A single signal is not proof of bad intent (many legitimate new users have no avatar),
but amber entries are the ones worth reviewing first. Multiple signals stacked on one
join is a stronger indicator.

### Member Left

Posted when a member leaves or is removed. Includes:

- A ping and ID of the user.
- **Member since** — when they joined and how long they were a member.

### Invite Created

Posted whenever anyone creates a new invite. Includes:

- A ping of the person who created it.
- The invite **code**.
- When it was created.
- When it expires (or "Never" for permanent invites).

---

## Setup

### 1. Create the bot application

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications) and
   create a new application.
2. Under **Bot**, create a bot user and **copy the token** — you will need it in step 3.
3. Still under **Bot**, scroll to **Privileged Gateway Intents** and enable:
   - **Server Members Intent** (required to see joins/leaves).

   The bot also uses the **Guild Invites** intent; that one is on by default and needs
   no special toggle.

### 2. Invite the bot to your server

When generating the invite URL / OAuth2 URL, the bot needs these permissions:

- **View Channels** — to access your log channel.
- **Send Messages** — to post logs.
- **Embed Links** — logs use rich embeds.
- **Manage Server** — required for the bot to read the server's invite list, which is
  how it figures out which invite a new member used.

> **Manage Server** is a sensitive permission. It is required *only* so the bot can read
> invite usage counts — it does not use it to change any server settings.

### 3. Configure the bot

The bot reads a file called `config.toml`. Create it with:

```toml
# The ID of the channel where logs should be posted.
# Right-click your log channel -> Copy Channel ID (Developer Mode must be on).
target_channel = 123456789012345678

# The bot token. You can also leave this empty and provide it via the
# DISCORD_TOKEN environment variable instead.
token = ""

# Database connection string. Leave empty to use the DATABASE_URL environment
# variable, or the built-in default for Docker (see below).
database_url = ""
```

| Field          | Required? | Description |
|----------------|-----------|-------------|
| `target_channel` | Yes     | The channel ID for all log messages. |
| `token`          | Yes*    | The bot token. *Can be omitted if `DISCORD_TOKEN` is set in the environment.* |
| `database_url`   | Yes*    | Postgres connection string. *Can be omitted if `DATABASE_URL` is set, or the Docker default is used.* |

The bot needs a PostgreSQL database to remember invite usage and rejoin counts. The
included `docker-compose.yml` spins one up automatically.

### 4. Run it

The easiest way is Docker Compose, which starts both the database and the bot:

```sh
docker compose up -d --build
```

This builds the bot image and starts it alongside the Postgres database. The bot will
retry connecting to the database for up to a few minutes, so startup order is not a
concern.

To stop it:

```sh
docker compose down
```

Database data is kept in a named volume (`postgres_data`) and survives restarts.

---

## Tips for Moderators

- **Invite attribution is best-effort.** The bot compares invite use counts before and
  after a join. In rare race conditions (e.g. two simultaneous joins) it may not be able
  to pin down the exact invite, and will say so honestly.
- **Single-use invites.** Discord deletes these the moment they are used. The bot handles
  this case specifically and can still attribute the join in most situations.
