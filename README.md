# Discord Logging Bot

A moderation-focused logging bot that posts detailed, at-a-glance information whenever something changes in your discord server.

It is desigend to provide as much useful information as possible in the most readbale format.

The bot is essentially divided into 3 main sections, each configuarable with its own channel:

- Invite logging
  - Members joining/leaving
  - Members banned/kicked
  - Invites being created
- Deleted messages
  - Edited messages
  - Deleted messages
  - Deleted messages **by admin**
  - Bulk message deletes
  - Deleted attachements/media
- Audit logs
  - Creating/editing/deleting channels, roles, permissions, emojis, stickers and more
  - Unbanning, timing out, muting, and other member moderation items
  - Editing automod rules

---

## Invite logging

All messages of this category are sent to a single **log channel** that you choose. Each event is a
colour-coded embed so you can scan the channel quickly:

| Colour    | Meaning           |
|-----------|-------------------|
| **Green** | A member joined (no concerns) |
| **Amber** | A member joined **and was flagged suspicious** |
| **Red**   | A member left     |
| **Blue**  | An invite was created |

### Information provided

- **Member ping** (easy to right click and ban)
  - username
  - globalname
- **Invite used** 
  - invite code
  - number of uses
  - inviter mention and username
  - invite creation time
  - *N.B: Invite attribution is best-effort and, while reliable, can fail*
- **Account creation**
  - as timestamp of account creation
  - as age at time of joining
- **Suspicions** (see next section)
- **Ban/kick information**
  - banning admin mention and username
  - reason
  - *N.B: Ban/kick information is best-effort*
- **Number of re-join/leaves**
  - *This is stored in the database and does not retroactively count rejoins*
- **Number of messages sent** (on leave)
  - *This again comes from the database and includes at most 30d*

### Suspicious Join Detection

When a join trips one or more suspicion signals, the embed turns **amber** and gains a
**Suspicious** field listing every reason. The signals are:
- **Account younger than 48h**
- **Unusual DM activity** As flagged by discord, alongside the duration of the infraction.
- **No avatar set**
- **No display name set** (globalname matches username)

A single signal is not proof of bad intent (many legitimate new users have no avatar),
but amber entries are the ones worth reviewing first. Multiple signals stacked on one
join is a stronger indicator.

## Deleted and edited messages

This bot also logs deleted messages, every message sent is logged in the database. Messages by bots, deleted by bots are ignored to avoid spam when using commands.

*Deleted message detection is best-effort and may not be 100% reliable* 

### Information provided
- **User mention and username**
- **Deleter admin information** (if available)
  - *N.B. This is also best effort and may be incorrect or be skipped in rare cases*
- **Channel**
  - Channel mention and name (in case of deletion)
  - Parent channel in case of a thread or channel group
- **Previous content of the message**
- **Message sent timestamp**
  - as timestamp
  - as time the message was up for (deled message only)
- **A link to the message**
  - In deleted messages showing surrounding message
  - *This may not work reliably on the app, it's a known Discord mobile issue*
- **Number of previous edits to the message**
- **Stickers/gifs**
  - These are added as images in the embed, animation may or may not work depening on the kind of sticker and availability.
- **Attachments**
  - Attachments are re-uploaded only at time of deletion. Attachments are *NEVER* stored locally.
  - A maximum limit for attachment size is configurable.
  - File names and spoiler status are preserved.
  - *N.B. This is also best effort and may be only return blank files*

### Bulk message delete
Bulk message deletes are handled differently, in order to reudce clutter messages are grouped into one, de-duplicated, and trimmed if too long.

Data shown:
- Number of messages deleted
- Channel where the messages were sent in
- A ping to the person who sent the message
- Trimmed list of messages if a available.

## Audit log

Plenty of data from the **settings > audit log** is hard to read and often buried under a lot of useless data. This bot aims to show it in the most reable format in a channel so edits to the server are visible at a glance.

There's way too much to list here, but highlights include

Channel permission changes logged as:
- Permission name ✅ **➜** ❌

Only showing changes, not the whole permissions structure

Stickers and emojis are shown as images whenever possible

Member actions show the affected member's profile picture

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

*N.B.: The current setup only allows one log channel, if the bot is ivited to multiple servers, logs from all servers are sent to a single one.*

When generating the invite URL / OAuth2 URL, the bot needs these permissions:

- **View Channels** to access your log channel.
- **Send Messages** to post logs.
- **Embed Links** logs use rich embeds.
- **Manage Server** required for the bot to read the server's invite list, which is how it figures out which invite a new member used. It is also required for audit logs, ban/kick detection and admin message deletion.

### 3. Configure the bot

The bot reads a file called `config.toml`. 

An example `config.toml.example` file is present in this repo, alongside comments describing each entry.

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
