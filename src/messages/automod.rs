use crate::{
    find_change, format_boolean_change, format_generic_change_internal,
    format_string_change,
    messages::{
        colours::*,
        format_time::format_time_diff,
        utils::{build_embed_author, format_user, get_name},
    },
};
use serenity::{
    all::{
        AuditLogEntry, AutoModAction, Change, ChannelId, Context, CreateEmbed, CreateMessage,
        GenericChannelId, GuildId, RoleId, RuleId, User, audit_log::Action, automod,
    },
    small_fixed_array::{FixedArray, FixedString},
};
use std::{collections::HashSet, fmt::Write};

pub async fn build_automod_message(
    entry: AuditLogEntry,
    admin: Option<User>,
    guild_id: GuildId,
    ctx: &Context,
) -> Option<CreateMessage<'static>> {
    let Some(target_id) = entry.target_id else {
        log::error!("No target channel id provided");
        return None;
    };

    let Some(user_id) = entry.user_id else {
        return None;
    };

    let (action, colour) = match entry.action {
        Action::AutoMod(AutoModAction::RuleCreate) => ("created", POSITIVE_COLOUR),
        Action::AutoMod(AutoModAction::RuleDelete) => ("deleted", NEGATIVE_COLOUR),
        Action::AutoMod(AutoModAction::RuleUpdate) => ("updated", EDIT_COLOUR),
        _ => {
            // We don't care about other actions like messages being blocked
            return None;
        }
    };

    //let entry = &guild_id.audit_logs(&ctx.http, Some(entry.action), entry.user_id, Some(entry.id), None, NonMaxU8::new(1)).await.unwrap().entries[0];

    let user_str = format_user(&admin, user_id);

    let rule_id = RuleId::new(target_id.get());

    let rule = guild_id.automod_rule(&ctx.http, rule_id).await.ok();

    let name_change = find_change!(entry.changes, Change::Name);
    let name = get_name(name_change)
        .map(str::to_string)
        .unwrap_or_else(|| {
            rule.map(|rule| rule.name.to_string())
                .unwrap_or_else(|| "*Unknown rule*".to_string())
        });

    let changes_string = entry
        .changes
        .iter()
        .filter_map(build_automod_change_line)
        .collect::<Vec<_>>()
        .join("\n");

    let embed_author = build_embed_author(&admin, user_id);
    let message = format!(
        "{user_str} **{action} automod rule** {name}**:**\n\n\
         {changes_string}"
    );
    let title = format!("AUTOMOD RULE {}", action.to_uppercase());

    let embed = CreateEmbed::new()
        .title(title)
        .author(embed_author)
        .color(colour)
        .description(message);

    Some(CreateMessage::new().embed(embed))
}

fn build_automod_change_line(change: &Change) -> Option<String> {
    Some(match change {
        Change::Name { old, new } => format_string_change!("Name", old, new),
        Change::Enabled { old, new } => format_boolean_change!("Enabled", old, new),
        Change::Actions { old, new } => {
            let (old_alert_channel, old_message, old_timeout_duration) = unwrap_actions(old);
            let (new_alert_channel, new_message, new_timeout_duration) = unwrap_actions(new);
            let res = [
                format_generic_change_internal!(
                    "Alert channel",
                    old_alert_channel,
                    new_alert_channel,
                    |c| format!("<#{c}>")
                ),
                format_generic_change_internal!(
                    "Timeout duration",
                    old_timeout_duration,
                    new_timeout_duration,
                    |c| format!("`{}`", format_time_diff(c, 3))
                ),
                format_generic_change_internal!(
                    "Alert message",
                    old_message,
                    new_message,
                    |m| format!("\"{m}\"")
                ),
            ];

            res.into_iter().flatten().collect::<Vec<_>>().join("\n")
        }
        Change::Other {
            key,
            old_value: _,
            new_value: Some(value),
        } => {
            let list = serde_json::from_value::<Vec<String>>(value.clone()).ok();
            let Some(list) = list else {
                return None;
            };

            let label = match key.as_str() {
                "$add_keyword_filter" => "Added words",
                "$remove_keyword_filter" => "Removed words",
                "$add_regex_patterns" => "Added regex",
                "$remove_regex_patterns" => "Removed regex",
                "$add_allow_list" => "Added allowed words",
                "$remove_allow_list" => "Removed allowed words",
                _ => return Some(key.to_string()),
            };

            format_keyword_change(label, &list)
        }
        Change::ExemptRoles { old, new } => {
            let old_set: HashSet<_> = old.as_ref().into_iter().flatten().collect();
            let new_set: HashSet<_> = new.as_ref().into_iter().flatten().collect();

            // Do diff
            let removed: Vec<_> = old_set.difference(&new_set).cloned().collect();
            let added: Vec<_> = new_set.difference(&old_set).cloned().collect();

            let mut res = String::new();

            if !removed.is_empty() {
                res.push_str(&format_role_list("Removed exempt roles", &removed));
            }
            if !added.is_empty() {
                res.push_str(&format_role_list("Added exempt roles", &added));
            }

            res.pop();
            res
        }
        Change::ExemptChannels { old, new } => {
            let old_set: HashSet<_> = old.as_ref().into_iter().flatten().collect();
            let new_set: HashSet<_> = new.as_ref().into_iter().flatten().collect();

            // Do diff
            let removed: Vec<_> = old_set.difference(&new_set).cloned().collect();
            let added: Vec<_> = new_set.difference(&old_set).cloned().collect();

            let mut res = String::new();

            if !removed.is_empty() {
                res.push_str(&format_channel_list("Removed exempt channels", &removed));
            }
            if !added.is_empty() {
                res.push_str(&format_channel_list("Added exempt channels", &added));
            }

            res.pop();
            res
        }
        _ => return None,
    })
}

fn format_keyword_change(label: &str, changes: &[String]) -> String {
    let mut res = format!("- **{label}:**\n");

    for change in changes {
        writeln!(&mut res, "  - `{change}`").unwrap();
    }
    res.pop();
    res
}

fn format_channel_list(label: &str, channels: &[&ChannelId]) -> String {
    let mut res = format!("- **{label}:**\n");

    for channel in channels {
        writeln!(&mut res, "  - <#{channel}>").unwrap();
    }
    res
}

fn format_role_list(label: &str, roles: &[&RoleId]) -> String {
    let mut res = format!("- **{label}:**\n");

    for role in roles {
        writeln!(&mut res, "  - <@!{role}>").unwrap();
    }
    res
}

fn unwrap_actions(
    actions: &Option<FixedArray<automod::Action>>,
) -> (
    Option<&GenericChannelId>,
    &Option<FixedString<u16>>,
    Option<u64>,
) {
    let mut alert_channel = None;
    let mut message = &None;
    let mut timeout_duration = None;

    if let Some(actions) = actions {
        for action in actions {
            match action {
                automod::Action::Alert(c) => alert_channel = Some(c),
                automod::Action::BlockMessage { custom_message } => message = custom_message,
                automod::Action::Timeout(duration) => timeout_duration = Some(duration.as_secs()),
                _ => (),
            }
        }
    }
    (alert_channel, message, timeout_duration)
}
