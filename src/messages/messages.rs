use std::error::Error;

use serenity::all::{
    Channel, CreateAttachment, CreateEmbed, CreateEmbedAuthor, CreateMessage, GenericChannelId,
    GuildId, MessageId, User, UserId,
};
use serenity::futures::future::join_all;
use similar::{ChangeTag, DiffOp, DiffTag, InlineChange, TextDiff};
use time::OffsetDateTime;

use crate::messages::colours::*;
use crate::messages::format_time::format_time_diff;
use crate::messages::utils::{
    build_embed_author, build_embed_author_admin, format_channel, format_user,
};

fn build_message_info(user: &Option<User>, user_id: Option<UserId>) -> String {
    match (user, user_id) {
        (Some(user), _) => {
            format!("**Message by** <@{}>({})", user.id, user.name)
        }
        (None, Some(id)) => {
            format!("**Message by** <@{id}>")
        }
        (None, None) => "**Unknown message** ".to_string(),
    }
}

/// Discord embeds only honour three-backtick fences, so a longer fence cannot
/// shield the block from code fences inside the diffed content. Break up any
/// run of more than two backticks by injecting reset sequences: invisible to
/// the ansi grammar, but they stop the description's markdown parser from
/// ever seeing a fence inside the body.
fn break_backtick_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0usize;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            continue;
        }
        flush_backtick_run(&mut out, run);
        run = 0;
        out.push(c);
    }
    flush_backtick_run(&mut out, run);
    out
}

/// Emits a run of backticks in chunks of at most two, separated by resets.
fn flush_backtick_run(out: &mut String, run: usize) {
    let mut remaining = run;
    while remaining > 2 {
        out.push_str("``");
        out.push_str(ANSI_RESET);
        remaining -= 2;
    }
    for _ in 0..remaining {
        out.push('`');
    }
}

fn truncate_on_char_boundary(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    let mut end = max_len;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    let mut cut = &text[..end];
    // Never leave a partial ANSI escape sequence dangling at the cut point
    if let Some(pos) = cut.rfind('\u{1b}') {
        if !cut[pos..].contains('m') {
            cut = &cut[..pos];
        }
    }

    // "\n" + reset + "…" adds 8 bytes on top of the slice; the reset keeps a
    // colour opened just before the cut from bleeding into the marker
    format!("{cut}\n{ANSI_RESET}…")
}

/// SGR sequences Discord's `ansi` code block grammar understands. The bright
/// red/green variants render as highlighted text, matching how `diff` blocks
/// look in Discord.
const ANSI_RESET: &str = "\u{1b}[0m";
const ANSI_DIM: &str = "\u{1b}[2m";
const ANSI_DELETED: &str = "\u{1b}[1;31m";
const ANSI_INSERTED: &str = "\u{1b}[1;32m";

/// Git numbers hunk ranges 1-based, except zero-length ranges which point at
/// the line before the insertion point; ",1" is omitted for single-line hunks.
fn hunk_header(group: &[DiffOp]) -> String {
    let mut old_start = usize::MAX;
    let mut old_end = 0;
    let mut new_start = usize::MAX;
    let mut new_end = 0;
    for op in group {
        let old = op.old_range();
        let new = op.new_range();
        old_start = old_start.min(old.start);
        old_end = old_end.max(old.end);
        new_start = new_start.min(new.start);
        new_end = new_end.max(new.end);
    }

    let old_count = old_end.saturating_sub(old_start);
    let new_count = new_end.saturating_sub(new_start);
    let old_display = if old_count == 0 {
        old_start
    } else {
        old_start + 1
    };
    let new_display = if new_count == 0 {
        new_start
    } else {
        new_start + 1
    };

    format!(
        "@@ -{}{} +{}{} @@",
        old_display,
        if old_count == 1 {
            String::new()
        } else {
            format!(",{old_count}")
        },
        new_display,
        if new_count == 1 {
            String::new()
        } else {
            format!(",{new_count}")
        },
    )
}

fn has_emphasis(change: &InlineChange<'_, str>) -> bool {
    change
        .iter_strings_lossy()
        .any(|(emphasized, _)| emphasized)
}

/// Unchanged line, rendered as-is in the default colour.
fn render_plain_line(change: &InlineChange<'_, str>) -> String {
    let mut line = String::new();
    for (_, value) in change.iter_strings_lossy() {
        line.push_str(&value);
    }
    line
}

/// Wholly added or removed line; the colour alone carries the meaning.
fn render_whole_line(change: &InlineChange<'_, str>, color: &str) -> String {
    let mut line = String::new();
    line.push_str(color);
    for (_, value) in change.iter_strings_lossy() {
        line.push_str(&value);
    }
    line.push_str(ANSI_RESET);
    line
}

/// Merges the two sides of an edited line pair into a single output line:
/// unchanged words appear once in the default colour, removed words are red
/// and added words green, in place. Word diffs refine both sides with the
/// same split points, so unemphasized segments line up pairwise.
fn render_merged_line(old: &InlineChange<'_, str>, new: &InlineChange<'_, str>) -> String {
    let old_segments: Vec<_> = old.iter_strings_lossy().collect();
    let new_segments: Vec<_> = new.iter_strings_lossy().collect();

    let mut line = String::new();
    let mut i = 0;
    let mut j = 0;
    while i < old_segments.len() && j < new_segments.len() {
        let (old_emphasized, old_value) = &old_segments[i];
        match (*old_emphasized, new_segments[j].0) {
            // unchanged words are identical on both sides; keep one copy
            (false, false) => {
                line.push_str(old_value);
                i += 1;
                j += 1;
            }
            (true, _) => {
                line.push_str(ANSI_DELETED);
                line.push_str(old_value);
                line.push_str(ANSI_RESET);
                i += 1;
            }
            (false, true) => {
                line.push_str(ANSI_INSERTED);
                line.push_str(&new_segments[j].1);
                line.push_str(ANSI_RESET);
                j += 1;
            }
        }
    }
    // Whatever one side still holds has no counterpart on the other
    for (_, value) in &old_segments[i..] {
        line.push_str(ANSI_DELETED);
        line.push_str(value);
        line.push_str(ANSI_RESET);
    }
    for (_, value) in &new_segments[j..] {
        line.push_str(ANSI_INSERTED);
        line.push_str(value);
        line.push_str(ANSI_RESET);
    }

    line
}

/// Word alignment found no shared words, so the pair is a total rewrite;
/// both full versions go on one line, old in red followed by new in green.
fn render_rewritten_line(old: &InlineChange<'_, str>, new: &InlineChange<'_, str>) -> String {
    let old_text: String = old
        .iter_strings_lossy()
        .map(|(_, value)| value.into_owned())
        .collect();
    let new_text: String = new
        .iter_strings_lossy()
        .map(|(_, value)| value.into_owned())
        .collect();

    let old_trimmed = old_text.trim_end_matches(['\r', '\n']);
    let new_trimmed = new_text.trim_end_matches(['\r', '\n']);

    let mut line = String::new();
    line.push_str(ANSI_DELETED);
    line.push_str(old_trimmed);
    line.push_str(ANSI_RESET);
    // A new side that is only a newline adds nothing next to the red text
    if !new_trimmed.is_empty() {
        line.push_str(ANSI_INSERTED);
        line.push_str(new_trimmed);
        line.push_str(ANSI_RESET);
    }
    line.push('\n');
    line
}

/// Builds a word-level diff between the old and new message content, rendered
/// as an `ansi` code block so Discord colours removed words red and added
/// words green while leaving unchanged text at the default colour, like
/// `git diff --word-diff=color`. Edited lines are merged into a single line
/// showing red removed words and green added words in place.
/// `budget` is the maximum number of bytes the returned block may occupy.
fn format_diff_block(old_content: &str, new_content: &str, budget: usize) -> String {
    // Message content never ends in a newline; adding one keeps the diff from
    // littering every hunk with "\ No newline at end of file" markers.
    let old_content = format!("{old_content}\n");
    let new_content = format!("{new_content}\n");
    let diff = TextDiff::from_lines(&old_content, &new_content);

    let mut body = String::new();
    for group in diff.grouped_ops(3).iter() {
        body.push_str(ANSI_DIM);
        body.push_str(&hunk_header(group));
        body.push_str(ANSI_RESET);
        body.push('\n');
        for op in group {
            match op.tag() {
                DiffTag::Equal => {
                    for change in diff.iter_inline_changes(op) {
                        body.push_str(&render_plain_line(&change));
                    }
                }
                DiffTag::Delete => {
                    for change in diff.iter_inline_changes(op) {
                        body.push_str(&render_whole_line(&change, ANSI_DELETED));
                    }
                }
                DiffTag::Insert => {
                    for change in diff.iter_inline_changes(op) {
                        body.push_str(&render_whole_line(&change, ANSI_INSERTED));
                    }
                }
                DiffTag::Replace => {
                    let changes: Vec<_> = diff.iter_inline_changes(op).collect();
                    let old_side: Vec<_> = changes
                        .iter()
                        .filter(|c| c.tag() == ChangeTag::Delete)
                        .collect();
                    let new_side: Vec<_> = changes
                        .iter()
                        .filter(|c| c.tag() == ChangeTag::Insert)
                        .collect();

                    let pairs = old_side.len().min(new_side.len());
                    for i in 0..pairs {
                        let (old, new) = (old_side[i], new_side[i]);
                        if has_emphasis(old) || has_emphasis(new) {
                            body.push_str(&render_merged_line(old, new));
                        } else {
                            body.push_str(&render_rewritten_line(old, new));
                        }
                    }
                    // Leftover lines have no counterpart and were wholly
                    // added or removed
                    for change in old_side[pairs..].iter() {
                        body.push_str(&render_whole_line(change, ANSI_DELETED));
                    }
                    for change in new_side[pairs..].iter() {
                        body.push_str(&render_whole_line(change, ANSI_INSERTED));
                    }
                }
            }
        }
    }
    let body = body.trim_end_matches(['\r', '\n']);
    let body = break_backtick_runs(body);

    let fence = "```";

    // Block layout: "{fence}ansi\n{body}\n{fence}"; +8 for the truncation
    // marker truncate_on_char_boundary appends ("\n" + reset + "…")
    let max_body = budget.saturating_sub(fence.len() * 2 + 6 + 8);
    let body = truncate_on_char_boundary(&body, max_body);

    format!("{fence}ansi\n{body}\n{fence}")
}

pub fn build_edited_message(
    user: Option<User>,
    user_id: UserId,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    guild: GuildId,
    message_id: MessageId,
    old_content: String,
    new_content: String,
    edits: i32,
) -> CreateMessage<'static> {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, Some(user_id));
    let embed_author = build_embed_author(&user, user_id);

    let formatted_channel = format_channel(&channel, channel_id);

    let edited_string = match edits {
        1 => "(edited once)".to_string(),
        2.. => format!(" (edited {edits} times)"),
        _ => String::new(),
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel_id}/{message_id}");

    // Discord caps embed descriptions at 4096 characters
    let description_header = format!(
        "**Edited message in** {formatted_channel}\n\
         {message_author} changes**:**\n"
    );
    let description_footer = format!(
        "\n-# Posted <t:{created}:f>{edited_string}\n\
         -# [Jump to message]({message_link})"
    );

    let diff_block = format_diff_block(
        &old_content,
        &new_content,
        4096 - description_header.len() - description_footer.len() - 1,
    );

    let embed_description = format!("{description_header}\n{diff_block}{description_footer}");

    let embed = CreateEmbed::new()
        .author(embed_author)
        .color(EDIT_COLOUR)
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

pub async fn build_deleted_message(
    user: Option<User>,
    user_id: Option<UserId>,
    deleter: Option<User>,
    deleter_id: Option<UserId>,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    guild: GuildId,
    message_id: MessageId,
    content: Option<String>,
    attachments: Option<String>,
    stickers: Option<String>,
    edits: i32,
) -> CreateMessage<'static> {
    let created = message_id.created_at().unix_timestamp();

    let message_author = build_message_info(&user, user_id);

    let embed_author = if let Some(user_id) = user_id {
        build_embed_author_admin(&user, user_id, &deleter)
    } else {
        CreateEmbedAuthor::new("unknown author")
    };

    let deleter_info = if let Some(deleter_id) = deleter_id {
        format!("\n**Deleted by** {}", format_user(&deleter, deleter_id))
    } else {
        String::new()
    };

    let formatted_channel = format_channel(&channel, channel_id);

    let content = match content {
        Some(content) => content,
        None => "*Message content not available*".to_string(),
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let formatted_age = format_time_diff((now - created) as u64, 3);

    let edited_string = match edits {
        0 => String::new(),
        1 => " (edited)".to_string(),
        _ => format!(" (edited {edits} times)"),
    };

    let message_link = format!("https://discord.com/channels/{guild}/{channel_id}/{message_id}");

    let embed_description = format!(
        "**Deleted message in** {formatted_channel}\
         {deleter_info}\n\
         {message_author} **:**\n\n\
         {content}\n\n\
         -# Posted <t:{created}:f> up for `{formatted_age}`{edited_string}\n\
         -# [Jump to surrounding]({message_link})"
    );

    let mut embed: CreateEmbed<'_> = CreateEmbed::new()
        .author(embed_author)
        .color(NEGATIVE_COLOUR)
        .description(embed_description);

    /*// if an admin deletes the message, the pfp will be the admin's one
    if deleter_id.is_some() && let Some(user) = user {
        embed = embed.thumbnail(user.face(), None);
    }*/

    let mut message = reupload_attachements(attachments).await;

    if let Some(stickers) = stickers {
        let attachments: Vec<&str> = stickers.split("\n").collect();

        if !attachments.is_empty() {
            // First attachment goes in the main embed
            embed = embed.image(attachments[0].to_string(), None);
            message = message.embed(embed);

            // Any additional attachments get their own embeds
            for attachment in attachments.iter().skip(1) {
                let extra_embed = CreateEmbed::new()
                    .image((*attachment).to_string(), None)
                    .color(NEGATIVE_COLOUR);
                message = message.add_embed(extra_embed);
            }
            return message;
        }
    }

    message.embed(embed)
}

pub fn build_bulk_delete_message(
    messages: Vec<(UserId, Option<User>, Vec<String>)>,
    channel: Option<Channel>,
    channel_id: GenericChannelId,
    count: usize,
) -> CreateMessage<'static> {
    let mut content = String::new();

    for (user_id, user, user_messages) in messages {
        content.push_str(&match user {
            Some(user) => format!("<@{user_id}>({})", user.name),
            None => format!("<@{user_id}>"),
        });

        // if there are no messages do not put the colon
        if user_messages.len() == 0 {
            content.push_str("\n");
            continue;
        } else {
            content.push_str(":\n");
        }

        for message in user_messages {
            content.push_str(&format!("-# • {message}\n"));
        }
    }

    let formatted_channel = format_channel(&channel, channel_id);

    let embed_description = format!(
        "**{count} messages deleted in** {formatted_channel}\n\n\
         {content}"
    );

    let embed = CreateEmbed::new()
        .title("BULK MESSAGE DELETE")
        .color(NEGATIVE_COLOUR)
        .description(embed_description);

    CreateMessage::new().embed(embed)
}

async fn reupload_attachements(attachments: Option<String>) -> CreateMessage<'static> {
    let mut builder = CreateMessage::new();

    let Some(attachments) = attachments else {
        return builder;
    };

    let download_futures = attachments.lines().map(|line| async move {
        let (url, filename) = line.split_once('|').unwrap_or((line, "unknown_attachment"));

        match download_single(url).await {
            Ok(bytes) => Some((bytes, filename.to_string())),
            Err(e) => {
                log::error!("Failed to fetch URL {}: {}", url, e);
                None
            }
        }
    });

    let results = join_all(download_futures).await;

    for (bytes, filename) in results.into_iter().flatten() {
        let attachment = CreateAttachment::bytes(bytes, filename);
        builder = builder.add_file(attachment);
    }

    builder
}
async fn download_single(url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    Ok(bytes.to_vec())
}
