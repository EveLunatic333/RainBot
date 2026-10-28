use serenity::all::*;
use tracing::{warn, error};
use std::time::Duration;
use serde::{Serialize, Deserialize};
use crate::MyErr;
use crate::event::interaction::{SlashBundle, ComponentBundle};
use crate::reusable::component::Components;
use crate::reusable::component::shutdown::Shutdown;
use crate::reusable::config::LfgRoles;
use crate::reusable::utils::MyTime;

const DEFAULT_PARTY_SIZE: usize = 4;
const MIN_PARTY_SIZE: usize = 2;
const MAX_PARTY_SIZE: usize = 4;
const COLOR_ACTIVE: u32 = 0x3498DB;
const COLOR_EDITED: u32 = 0xE67E22;
const COLOR_ASSEMBLED: u32 = 0x2ECC71;
const COLOR_CLOSED: u32 = 0xED4245;
const EDIT_TIMEOUT_SECS: u64 = 60;
const MAX_MEMBERS_FIELD_LENGTH: usize = 900;
const MEMBERS_FIELD_NAME: &str = "👥 Members";
const PARTY_INACTIVITY_SECS: i64 = 30 * 60;
const APPROVAL_MODE_MARKER: &str = "🔐 Approval mode";
const MAX_PANEL_ROWS: usize = 5;
const AUTHOR_ACTIVE: &str = "⚔️ Party Recruitment";
const AUTHOR_EDITED: &str = "⚔️ Party Recruitment (Edited)";
const AUTHOR_ASSEMBLED: &str = "🎉 Party Assembled — Recruitment Open";
const AUTHOR_CLOSED: &str = "🛑 Party Recruitment Closed";
const AUTHOR_CLOSED_INACTIVE: &str = "⏰ Party Recruitment Closed (Inactive)";

/// Tracks an active party recruitment post so inactive parties can be
/// auto-closed by the background sweep. Stored in the global `PARTIES` map,
/// keyed by the recruitment message id, and persisted through the CACHE file.
#[derive(Serialize, Deserialize, Clone)]
pub struct PartyRecord {
    pub message_id: u64,
    pub channel_id: u64,
    pub last_activity: i64,
    /// True when the party was created with `join_mode: approval`.
    #[serde(default)]
    pub approval_mode: bool,
    /// Players that asked to join an approval-mode party and have not been
    /// approved or rejected yet.
    #[serde(default)]
    pub pending: Vec<PendingRequest>,
}

/// A single pending join request in an approval-mode party.
#[derive(Serialize, Deserialize, Clone)]
pub struct PendingRequest {
    pub user_id: u64,
    pub requested_at: i64,
}

/// Everything the button handlers need to know about the current embed state.
struct ParsedPartyState {
    quest: String,
    location: String,
    notes: Option<String>,
    footer_text: String,
    members: Vec<String>,
    current_color: u32,
    current_author: String,
    was_assembled: bool,
    approval_mode: bool,
}

fn build_party_buttons(size: usize, leader_id: u64, join_disabled: bool, approval_mode: bool, pending_count: usize) -> Vec<CreateActionRow> {
    let join_label = if approval_mode { "Request to Join" } else { "Join Party" };
    let join_btn = Components::normal_button(join_label, &format!("party_{}_{}_j", size, leader_id), ButtonStyle::Success, "✅").disabled(join_disabled);
    let leave_btn = Components::normal_button("Leave Party", &format!("party_{}_{}_l", size, leader_id), ButtonStyle::Danger, "🚪");
    let edit_btn = Components::normal_button("Edit Party", &format!("party_{}_{}_e", size, leader_id), ButtonStyle::Secondary, "✏️");
    let close_btn = Components::normal_button("Close Party", &format!("party_{}_{}_c", size, leader_id), ButtonStyle::Danger, "🔒");
    let cancel_btn = Components::normal_button("Cancel Recruitment", &format!("party_{}_{}_x", size, leader_id), ButtonStyle::Danger, "🗑️");
    if approval_mode {
        // Approval mode needs a sixth button, so split into two rows
        // (Discord allows at most 5 buttons per row).
        let requests_btn = Components::normal_button(&format!("Requests ({})", pending_count), &format!("party_{}_{}_q", size, leader_id), ButtonStyle::Primary, "📋");
        vec![
            CreateActionRow::Buttons(vec![join_btn, leave_btn]),
            CreateActionRow::Buttons(vec![requests_btn, edit_btn, close_btn, cancel_btn]),
        ]
    } else {
        vec![CreateActionRow::Buttons(vec![join_btn, leave_btn, edit_btn, close_btn, cancel_btn])]
    }
}

fn parse_party_custom_id(custom_id: &str) -> Option<(usize, String, String)> {
    let mut parts = custom_id.splitn(3, '_');
    let prefix = parts.next()?;
    let size_str = parts.next()?;
    let rest = parts.next()?;
    if prefix != "party" {
        return None;
    }
    let size: usize = size_str.parse().ok()?;
    let mut rest_parts = rest.rsplitn(2, '_');
    let action = rest_parts.next()?.to_string();
    let leader_id = rest_parts.next()?.to_string();
    Some((size, leader_id, action))
}

/// Parses `papp_{partyMessageId}_{requesterUserId}_{a|r}` panel button ids.
fn parse_approval_custom_id(custom_id: &str) -> Option<(u64, u64, String)> {
    let mut parts = custom_id.splitn(4, '_');
    let prefix = parts.next()?;
    if prefix != "papp" {
        return None;
    }
    let party_msg_id: u64 = parts.next()?.parse().ok()?;
    let requester_id: u64 = parts.next()?.parse().ok()?;
    let action = parts.next()?.to_string();
    Some((party_msg_id, requester_id, action))
}

/// Reads the party size and leader id from the post's own buttons. Used by
/// the approval panel handlers, which only know the party message id.
fn party_meta_from_message(msg: &Message) -> Option<(usize, u64)> {
    for row in &msg.components {
        for comp in &row.components {
            if let ActionRowComponent::Button(btn) = comp {
                if let ButtonKind::NonLink { custom_id, .. } = &btn.data {
                    if let Some((size, leader_id_str, _)) = parse_party_custom_id(custom_id) {
                        if let Ok(leader_id) = leader_id_str.parse::<u64>() {
                            return Some((size, leader_id));
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_edit_input(input: &str) -> Option<(String, String)> {
    let mut parts = input.splitn(2, '|');
    let quest = parts.next()?.trim();
    let location = parts.next()?.trim();
    if quest.is_empty() || location.is_empty() {
        return None;
    }
    Some((quest.to_string(), location.to_string()))
}

fn get_embed_field_by_name<'a>(embed: &'a Embed, target_name: &str) -> Option<&'a str> {
    embed
        .fields
        .iter()
        .find(|f| f.name == target_name)
        .map(|f| f.value.as_str())
}

/// Finds the members field by name prefix so the lookup keeps working even
/// though the field name carries a changing "(x/y)" count suffix. This also
/// keeps legacy posts created before this update fully readable.
fn find_members_field<'a>(embed: &'a Embed) -> Option<&'a EmbedField> {
    embed.fields.iter().find(|f| f.name.starts_with(MEMBERS_FIELD_NAME))
}

/// Parses member mentions out of the members field value. Only real mention
/// lines are accepted, which filters out legacy placeholders such as "(1/4)"
/// and any corrupted fragments, so the roster can never contain garbage.
fn parse_members(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in value.lines() {
        let t = line.trim();
        if t.starts_with("<@") && t.ends_with('>') && !out.iter().any(|m| m == t) {
            out.push(t.to_string());
        }
    }
    out
}

fn format_members_list(members: &[String]) -> String {
    let mut result = String::new();
    for member in members {
        if !result.is_empty() {
            result.push('\n');
        }
        if result.len() + member.len() > MAX_MEMBERS_FIELD_LENGTH {
            break;
        }
        result.push_str(member);
    }
    result
}

/// Recovers the original party size from the "(x/y)" suffix of a members
/// field. Used by the inactivity sweep, which has no button data to read it from.
fn parse_party_size_from_field(embed: &Embed) -> Option<usize> {
    let field = find_members_field(embed)?;
    let open = field.name.rfind('(')?;
    let close = field.name.rfind(')')?;
    if close <= open {
        return None;
    }
    let inner = field.name.get(open + 1..close)?;
    inner.split('/').nth(1)?.trim().parse::<usize>().ok()
}

fn build_party_embed(
    author_name: &str,
    color: u32,
    quest: &str,
    location: &str,
    members: &[String],
    size: usize,
    notes: Option<&str>,
    footer_text: &str,
) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .author(CreateEmbedAuthor::new(author_name))
        .title("📜 Quest")
        .description(format!("# {}", quest))
        .field("🌍 Location", location, false)
        .field(
            format!("{} ({}/{})", MEMBERS_FIELD_NAME, members.len(), size),
            format_members_list(members),
            false,
        )
        .color(color)
        .footer(CreateEmbedFooter::new(footer_text));
    if let Some(notes) = notes {
        embed = embed.field("📝 Notes", notes.to_string(), false);
    }
    embed
}

/// Extracts all reusable state from a party recruitment embed in one place,
/// so every handler parses it identically.
fn parse_party_state(embed: &Embed) -> ParsedPartyState {
    let footer_text = embed.footer.as_ref().map_or_else(String::new, |f| f.text.clone());
    let current_author = embed.author.as_ref().map_or_else(|| AUTHOR_ACTIVE.to_string(), |a| a.name.clone());
    ParsedPartyState {
        quest: embed.description.clone().unwrap_or_default().trim_start_matches("# ").to_string(),
        location: get_embed_field_by_name(embed, "🌍 Location").unwrap_or_default().to_string(),
        notes: get_embed_field_by_name(embed, "📝 Notes").map(str::to_string),
        members: parse_members(find_members_field(embed).map(|f| f.value.as_str()).unwrap_or_default()),
        current_color: embed.colour.map_or(COLOR_ACTIVE, |c| c.0),
        was_assembled: current_author.contains("Assembled"),
        approval_mode: footer_text.contains(APPROVAL_MODE_MARKER),
        footer_text,
        current_author,
    }
}

/// Maps the `role_mention` slash option (see the choices registered in
/// `misc/mod.rs`) to the configured LFG role id.
fn lfg_role_id(roles: &LfgRoles, key: &str) -> Option<u64> {
    match key {
        "raviente_raid" => Some(roles.raviente_raid),
        "party_helper" => Some(roles.party_helper),
        "bounty_hunter" => Some(roles.bounty_hunter),
        "road_runner" => Some(roles.road_runner),
        "gacha_enjoyer" => Some(roles.gacha_enjoyer),
        "musou_slayer" => Some(roles.musou_slayer),
        _ => None,
    }
}

/// Registers or refreshes the activity timestamp of an active party post.
/// Also adopts legacy posts (created before this update) on first interaction.
async fn touch_party(message_id: u64, channel_id: u64) {
    let mut parties = crate::PARTIES.lock().await;
    let key = message_id.to_string();
    match parties.get_mut(&key) {
        Some(rec) => rec.last_activity = MyTime::now(),
        None => {
            parties.insert(
                key,
                PartyRecord {
                    message_id,
                    channel_id,
                    last_activity: MyTime::now(),
                    approval_mode: false,
                    pending: Vec::new(),
                },
            );
        }
    }
}

async fn forget_party(message_id: u64) {
    crate::PARTIES.lock().await.remove(&message_id.to_string());
}

async fn pending_count_of(message_id: u64) -> usize {
    crate::PARTIES.lock().await.get(&message_id.to_string()).map_or(0, |r| r.pending.len())
}

async fn pending_requests_of(message_id: u64) -> Vec<PendingRequest> {
    crate::PARTIES.lock().await.get(&message_id.to_string()).map(|r| r.pending.clone()).unwrap_or_default()
}

/// Removes one pending request (if present) and refreshes the activity
/// timestamp, since handling a request is party activity.
async fn remove_pending(message_id: u64, user_id: u64) {
    let mut parties = crate::PARTIES.lock().await;
    if let Some(rec) = parties.get_mut(&message_id.to_string()) {
        rec.pending.retain(|p| p.user_id != user_id);
        rec.last_activity = MyTime::now();
    }
}

/// Per-message lock guard: prevents two simultaneous button clicks on the same
/// recruitment post from reading the same embed state and overwriting each
/// other's update (same pattern as the bounty SUBMIT_LOCK).
async fn reserve_party_lock(key: &str) -> bool {
    crate::PARTY_LOCK.lock().await.insert(key.to_owned())
}

async fn release_party_lock(key: &str) {
    crate::PARTY_LOCK.lock().await.remove(key);
}

/// Builds the leader-only ephemeral review panel for pending join requests.
/// Discord allows at most 5 action rows per message, so only the first
/// MAX_PANEL_ROWS requests get buttons; the rest are listed as text and get
/// buttons once the rows above are handled.
fn build_requests_panel(party_msg_id: u64, pending: &[PendingRequest]) -> (CreateEmbed, Vec<CreateActionRow>) {
    let mut embed = CreateEmbed::new()
        .title(format!("📋 Join Requests ({})", pending.len()))
        .color(COLOR_ACTIVE);
    let mut rows: Vec<CreateActionRow> = Vec::new();
    if pending.is_empty() {
        embed = embed.description("No pending requests right now.");
    }
    for (idx, req) in pending.iter().enumerate() {
        if idx < MAX_PANEL_ROWS {
            embed = embed.field(format!("<@{}>", req.user_id), format!("Requested <t:{}:R>", req.requested_at), false);
            rows.push(CreateActionRow::Buttons(vec![
                Components::normal_button("Approve", &format!("papp_{}_{}_a", party_msg_id, req.user_id), ButtonStyle::Success, "✅"),
                Components::normal_button("Reject", &format!("papp_{}_{}_r", party_msg_id, req.user_id), ButtonStyle::Danger, "❌"),
            ]));
        } else {
            embed = embed.field(format!("<@{}>", req.user_id), format!("Requested <t:{}:R> *(buttons appear once the rows above are handled)*", req.requested_at), false);
        }
    }
    (embed, rows)
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let quest = bnd.cmd.data.options.first().map(|opt| opt.value.as_str().unwrap_or_default().to_string()).unwrap_or_default();
    let location = bnd.cmd.data.options.get(1).map(|opt| opt.value.as_str().unwrap_or_default().to_string()).unwrap_or_default();
    let size = bnd.cmd.data.options.get(2).and_then(|opt| opt.value.as_i64()).map(|val| val as usize).unwrap_or(DEFAULT_PARTY_SIZE).clamp(MIN_PARTY_SIZE, MAX_PARTY_SIZE);
    let role_mention = bnd.cmd.data.options.get(3).and_then(|opt| opt.value.as_str()).map(str::to_string);
    let notes = bnd.cmd.data.options.get(4).and_then(|opt| opt.value.as_str()).map(str::to_string);
    let approval_mode = bnd.cmd.data.options.get(5).and_then(|opt| opt.value.as_str()).map_or(false, |v| v == "approval");

    // The leader is a real member from the very start: no placeholder text
    // is ever placed in the member list, so counts are always truthful.
    let leader_id = bnd.cmd.user.id.get();
    let members = vec![format!("<@{}>", leader_id)];

    // The footer carries the mode marker so it survives restarts even if the
    // CACHE file is lost; handlers detect approval mode from it.
    let mut footer_text = format!("Created by <@{}> • <t:{}:R>", leader_id, MyTime::now());
    if approval_mode {
        footer_text.push_str(&format!(" • {}", APPROVAL_MODE_MARKER));
    }
    let embed = build_party_embed(AUTHOR_ACTIVE, COLOR_ACTIVE, &quest, &location, &members, size, notes.as_deref(), &footer_text);

    let mut message = CreateMessage::new().embed(embed);
    message = message.components(build_party_buttons(size, leader_id, false, approval_mode, 0));

    if let Some(role_key) = role_mention.as_deref() {
        if let Some(role_id) = lfg_role_id(&bnd.init.lfg_roles, role_key) {
            message = message.content(format!("📢 <@&{}>", role_id));
        }
    }

    let target_channel = ChannelId::new(bnd.init.log_channel.party_ch);
    match target_channel.send_message(&bnd.ctx.http, message).await {
        Ok(sent) => {
            {
                let mut parties = crate::PARTIES.lock().await;
                parties.insert(
                    sent.id.get().to_string(),
                    PartyRecord {
                        message_id: sent.id.get(),
                        channel_id: target_channel.get(),
                        last_activity: MyTime::now(),
                        approval_mode,
                        pending: Vec::new(),
                    },
                );
            }
            let _ = Shutdown::save().await;
            let response = CreateInteractionResponseMessage::new()
                .content(format!("✅ Party recruitment posted in <#{}>!", target_channel))
                .ephemeral(true);
            bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await?;
        }
        Err(e) => {
            let error_msg = "❌ Failed to post party recruitment. Please try again.";
            error!(channel_id = %target_channel, error = %e, "Failed to send party recruitment message");
            let response = CreateInteractionResponseMessage::new().content(error_msg).ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        }
    }
    Ok(())
}

#[hertz::hertz_button_normal(0, false)]
pub async fn button(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let (size, leader_id_str, action) = match parse_party_custom_id(&bnd.cmd.data.custom_id) {
        Some((s, lid, act)) if act.len() == 1 => (s, lid, act),
        _ => {
            warn!(custom_id = %bnd.cmd.data.custom_id, "Malformed party custom_id");
            return Ok(());
        }
    };

    let lock_key = bnd.cmd.message.id.to_string();
    if !reserve_party_lock(&lock_key).await {
        let response = CreateInteractionResponseMessage::new()
            .content("⏳ This party is being updated right now. Please click again.")
            .ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }
    let result = handle_party_button(bnd, size, leader_id_str, action).await;
    release_party_lock(&lock_key).await;
    result
}

async fn handle_party_button(bnd: &ComponentBundle<'_>, size: usize, leader_id_str: String, action: String) -> Result<(), MyErr> {
    let msg = &bnd.cmd.message;
    let embed = match msg.embeds.first() {
        Some(e) => e,
        None => {
            warn!(message_id = %msg.id, "Party message has no embed");
            return Ok(());
        }
    };

    let state = parse_party_state(embed);
    let ParsedPartyState { quest, location, notes, footer_text, members, current_color, current_author, was_assembled, approval_mode } = state;
    let pending_count = pending_count_of(msg.id.get()).await;

    let user_id = bnd.cmd.user.id.get();
    let user_mention = format!("<@{}>", user_id);
    let is_leader = user_id.to_string() == leader_id_str;
    let leader_id_u64: u64 = leader_id_str.parse().unwrap_or(0);

    match action.as_str() {
        "e" => {
            if !is_leader {
                let response = CreateInteractionResponseMessage::new()
                    .content("❌ Only the party leader can edit this party.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            handle_edit(bnd, size, leader_id_u64, notes.as_deref(), &footer_text, approval_mode).await?;
            touch_party(msg.id.get(), msg.channel_id.get()).await;
            let _ = Shutdown::save().await;
            Ok(())
        }
        "x" => {
            if !is_leader {
                let response = CreateInteractionResponseMessage::new()
                    .content("❌ Only the party leader can cancel this recruitment.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            let response = CreateInteractionResponseMessage::new()
                .content("🗑️ Recruitment cancelled. Deleting the post...")
                .ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            forget_party(msg.id.get()).await;
            let _ = Shutdown::save().await;
            let _ = msg.delete(&bnd.ctx.http).await;
            Ok(())
        }
        "c" => {
            if !is_leader {
                let response = CreateInteractionResponseMessage::new()
                    .content("❌ Only the party leader can close this party.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            // Closing keeps the real roster and count instead of resetting them.
            let closed_embed = build_party_embed(AUTHOR_CLOSED, COLOR_CLOSED, &quest, &location, &members, size, notes.as_deref(), &footer_text);
            let res_msg = CreateInteractionResponseMessage::new().embed(closed_embed).components(Vec::<CreateActionRow>::new());
            if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::UpdateMessage(res_msg)).await {
                warn!(message_id = %msg.id, error = %e, "Failed to close party embed");
            }
            forget_party(msg.id.get()).await;
            let _ = Shutdown::save().await;
            Ok(())
        }
        "q" => {
            if !is_leader {
                let response = CreateInteractionResponseMessage::new()
                    .content("🛑 Only the party leader can review join requests.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            let pending = pending_requests_of(msg.id.get()).await;
            let (panel_embed, panel_rows) = build_requests_panel(msg.id.get(), &pending);
            let response = CreateInteractionResponseMessage::new().embed(panel_embed).components(panel_rows).ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            touch_party(msg.id.get(), msg.channel_id.get()).await;
            Ok(())
        }
        "j" => {
            if members.contains(&user_mention) {
                let response = CreateInteractionResponseMessage::new()
                    .content("ℹ️ You are already registered in this party.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            if members.len() >= size {
                let response = CreateInteractionResponseMessage::new()
                    .content("❌ This party is already full.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            if approval_mode {
                return handle_join_request(bnd, msg, size, leader_id_u64, user_id, &user_mention, &quest, &location, notes.as_deref(), &footer_text, &members, &current_author, current_color, members.len()).await;
            }
            let mut members = members;
            members.push(user_mention.clone());
            let notify = format!("📥 <@{}>, **{}** joined your party ({}/{}).", leader_id_u64, user_mention, members.len(), size);
            finish_update(bnd, msg, size, leader_id_u64, &quest, &location, notes.as_deref(), &footer_text, current_color, &current_author, was_assembled, false, 0, Some(notify), members).await
        }
        "l" => {
            if is_leader {
                let response = CreateInteractionResponseMessage::new()
                    .content("❌ As the party leader, you cannot leave. Use **Close Party** or **Cancel Recruitment** instead.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            if !members.contains(&user_mention) {
                let response = CreateInteractionResponseMessage::new()
                    .content("ℹ️ You are not registered in this party.")
                    .ephemeral(true);
                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
                return Ok(());
            }
            let mut members = members;
            members.retain(|m| m != &user_mention);
            let notify = format!("📤 <@{}>, **{}** left your party ({}/{}).", leader_id_u64, user_mention, members.len(), size);
            finish_update(bnd, msg, size, leader_id_u64, &quest, &location, notes.as_deref(), &footer_text, current_color, &current_author, was_assembled, approval_mode, pending_count, Some(notify), members).await
        }
        _ => {
            warn!(action = %action, "Unknown party action");
            Ok(())
        }
    }
}

/// Approval-mode join: registers a pending request instead of joining,
/// confirms to the requester, pings the leader, and refreshes the badge.
#[allow(clippy::too_many_arguments)]
async fn handle_join_request(
    bnd: &ComponentBundle<'_>,
    msg: &Message,
    size: usize,
    leader_id_u64: u64,
    user_id: u64,
    user_mention: &str,
    quest: &str,
    location: &str,
    notes: Option<&str>,
    footer_text: &str,
    members: &[String],
    current_author: &str,
    current_color: u32,
    member_count: usize,
) -> Result<(), MyErr> {
    let already_pending = crate::PARTIES.lock().await.get(&msg.id.to_string()).map_or(false, |r| r.pending.iter().any(|p| p.user_id == user_id));
    if already_pending {
        let response = CreateInteractionResponseMessage::new()
            .content("ℹ️ You already sent a join request to this party. Please wait for the leader's decision.")
            .ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }

    {
        let mut parties = crate::PARTIES.lock().await;
        let key = msg.id.to_string();
        let now = MyTime::now();
        match parties.get_mut(&key) {
            Some(rec) => {
                rec.pending.push(PendingRequest { user_id, requested_at: now });
                rec.last_activity = now;
            }
            None => {
                parties.insert(
                    key,
                    PartyRecord {
                        message_id: msg.id.get(),
                        channel_id: msg.channel_id.get(),
                        last_activity: now,
                        approval_mode: true,
                        pending: vec![PendingRequest { user_id, requested_at: now }],
                    },
                );
            }
        }
    }
    let _ = Shutdown::save().await;

    // Respond to the interaction first so Discord's response deadline is met,
    // then ping the leader and refresh the Requests badge on the post.
    let response = CreateInteractionResponseMessage::new()
        .content(format!("✅ Request sent to <@{}>. You will be notified when they decide.", leader_id_u64))
        .ephemeral(true);
    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;

    let ping = format!("📥 <@{}>, **{}** wants to join your party ({}/{}). Use **Requests** to review.", leader_id_u64, user_mention, member_count, size);
    let _ = msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(ping)).await;

    let new_embed = build_party_embed(current_author, current_color, quest, location, members, size, notes, footer_text);
    let new_count = pending_count_of(msg.id.get()).await;
    if let Err(e) = msg.channel_id.edit_message(&bnd.ctx.http, msg.id, EditMessage::new().embed(new_embed).components(build_party_buttons(size, leader_id_u64, false, true, new_count))).await {
        warn!(message_id = %msg.id, error = %e, "Failed to refresh Requests badge");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn finish_update(
    bnd: &ComponentBundle<'_>,
    msg: &Message,
    size: usize,
    leader_id_u64: u64,
    quest: &str,
    location: &str,
    notes: Option<&str>,
    footer_text: &str,
    current_color: u32,
    current_author: &str,
    was_assembled: bool,
    approval_mode: bool,
    pending_count: usize,
    notify: Option<String>,
    members: Vec<String>,
) -> Result<(), MyErr> {
    let now_full = members.len() >= size;

    let (author_name, color) = if now_full {
        (AUTHOR_ASSEMBLED.to_string(), COLOR_ASSEMBLED)
    } else if was_assembled {
        // Someone left a full party: recruitment visibly reopens.
        (AUTHOR_ACTIVE.to_string(), COLOR_ACTIVE)
    } else {
        (current_author.to_string(), current_color)
    };

    // Respond to the interaction first so Discord's response deadline is met,
    // then send the public announcement/notification messages.
    let new_embed = build_party_embed(&author_name, color, quest, location, &members, size, notes, footer_text);
    let res_msg = CreateInteractionResponseMessage::new().embed(new_embed).components(build_party_buttons(size, leader_id_u64, now_full, approval_mode, pending_count));
    if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::UpdateMessage(res_msg)).await {
        warn!(message_id = %msg.id, error = %e, "Failed to update party embed");
    }

    // Announce the transition from "not full" to "full" exactly once; the
    // recruitment itself stays open (Join disabled) so members can still swap.
    if now_full && !was_assembled {
        let ping_msg = format!("🎉 **Party Assembled!** {} @ {} — hunters: {}", quest, location, members.join(" "));
        let _ = msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(ping_msg)).await;
    }
    if let Some(notify) = notify {
        let _ = msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(notify)).await;
    }

    touch_party(msg.id.get(), msg.channel_id.get()).await;
    let _ = Shutdown::save().await;
    Ok(())
}

async fn handle_edit(
    bnd: &ComponentBundle<'_>,
    size: usize,
    leader_id_u64: u64,
    notes: Option<&str>,
    footer_text: &str,
    approval_mode: bool,
) -> Result<(), MyErr> {
    let msg = &bnd.cmd.message;
    let response = CreateInteractionResponseMessage::new()
        .content("✏️ **Edit Party**\nType the new quest and location in this channel using this format:\n`quest | location`\n*Example: `Fatalis Run | Tower`\nYou have 60 seconds to respond.*")
        .ephemeral(true);
    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await?;

    if let Some(msg_reply) = bnd.cmd.channel_id.await_reply(&bnd.ctx).author_id(bnd.cmd.user.id).timeout(Duration::from_secs(EDIT_TIMEOUT_SECS)).await {
        let reply_content = msg_reply.content.trim().to_string();
        let _ = msg_reply.delete(&bnd.ctx.http).await;

        if let Some((new_quest, new_location)) = parse_edit_input(&reply_content) {
            // Editing resets the roster (leader stays the only member) and
            // clears every pending join request, since they applied to the
            // previous quest details.
            {
                let mut parties = crate::PARTIES.lock().await;
                if let Some(rec) = parties.get_mut(&msg.id.to_string()) {
                    rec.pending.clear();
                }
            }
            let members = vec![format!("<@{}>", leader_id_u64)];
            let new_embed = build_party_embed(AUTHOR_EDITED, COLOR_EDITED, &new_quest, &new_location, &members, size, notes, footer_text);
            let res_msg = EditMessage::new().embed(new_embed).components(build_party_buttons(size, leader_id_u64, false, approval_mode, 0));
            let confirm = if approval_mode {
                "✅ Party details updated! Members and pending join requests were reset."
            } else {
                "✅ Party details updated!"
            };
            if let Err(e) = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(confirm).components(vec![])).await {
                warn!(message_id = %msg.id, error = %e, "Failed to confirm party edit");
            }
            if let Err(e) = msg.channel_id.edit_message(&bnd.ctx.http, msg.id, res_msg).await {
                warn!(message_id = %msg.id, error = %e, "Failed to update party embed after edit");
            }
        } else {
            let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ Invalid format. Party was not updated. Use `quest | location`.").components(vec![])).await;
        }
    } else {
        let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ Edit timed out! Party was not updated.").components(vec![])).await;
    }
    Ok(())
}

/// Entry point for the ephemeral review panel buttons
/// (`papp_{partyMsgId}_{requesterId}_{a|r}`).
#[hertz::hertz_button_normal(0, false)]
pub async fn approval(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let (party_msg_id, requester_id, action) = match parse_approval_custom_id(&bnd.cmd.data.custom_id) {
        Some((m, u, act)) if act == "a" || act == "r" => (m, u, act),
        _ => {
            warn!(custom_id = %bnd.cmd.data.custom_id, "Malformed party approval custom_id");
            return Ok(());
        }
    };

    let party_msg = match bnd.cmd.channel_id.message(&bnd.ctx.http, MessageId::new(party_msg_id)).await {
        Ok(m) => m,
        Err(_) => {
            let response = CreateInteractionResponseMessage::new().content("ℹ️ This party no longer exists.").ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            return Ok(());
        }
    };

    let (size, leader_id) = match party_meta_from_message(&party_msg) {
        Some(meta) => meta,
        None => {
            let response = CreateInteractionResponseMessage::new().content("ℹ️ This party post is no longer valid.").ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            return Ok(());
        }
    };

    if bnd.cmd.user.id.get() != leader_id {
        let response = CreateInteractionResponseMessage::new()
            .content("🛑 Only the party leader can review join requests.")
            .ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }

    let lock_key = party_msg_id.to_string();
    if !reserve_party_lock(&lock_key).await {
        let response = CreateInteractionResponseMessage::new()
            .content("⏳ This party is being updated right now. Please click again.")
            .ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }
    let result = if action == "a" {
        approve_request(bnd, party_msg, size, leader_id, requester_id).await
    } else {
        reject_request(bnd, party_msg, requester_id).await
    };
    release_party_lock(&lock_key).await;
    result
}

/// Approves one pending request: re-validates everything (stale panel clicks
/// must be safe), adds the requester to the roster, refreshes the post and
/// the panel, and sends the public notifications.
async fn approve_request(bnd: &ComponentBundle<'_>, mut party_msg: Message, size: usize, leader_id: u64, requester_id: u64) -> Result<(), MyErr> {
    let party_msg_id = party_msg.id.get();
    let still_pending = crate::PARTIES.lock().await.get(&party_msg_id.to_string()).map_or(false, |r| r.pending.iter().any(|p| p.user_id == requester_id));
    if !still_pending {
        let response = CreateInteractionResponseMessage::new().content("ℹ️ This request has already been handled.").ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }

    let embed = match party_msg.embeds.first() {
        Some(e) => e,
        None => {
            let response = CreateInteractionResponseMessage::new().content("ℹ️ This party post is no longer valid.").ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            return Ok(());
        }
    };
    let ParsedPartyState { quest, location, notes, footer_text, mut members, current_color, current_author, was_assembled, approval_mode } = parse_party_state(embed);
    let requester_mention = format!("<@{}>", requester_id);

    if members.contains(&requester_mention) {
        remove_pending(party_msg_id, requester_id).await;
        let _ = Shutdown::save().await;
        let response = CreateInteractionResponseMessage::new().content("ℹ️ This player is already a member of the party.").ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }
    if members.len() >= size {
        remove_pending(party_msg_id, requester_id).await;
        let _ = Shutdown::save().await;
        let response = CreateInteractionResponseMessage::new().content("🛑 The party is already full — this request was declined.").ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }

    members.push(requester_mention.clone());
    let now_full = members.len() >= size;
    let (author_name, color) = if now_full {
        (AUTHOR_ASSEMBLED.to_string(), COLOR_ASSEMBLED)
    } else if was_assembled {
        (AUTHOR_ACTIVE.to_string(), COLOR_ACTIVE)
    } else {
        (current_author, current_color)
    };

    remove_pending(party_msg_id, requester_id).await;
    let _ = Shutdown::save().await;
    let pending_after = pending_requests_of(party_msg_id).await;

    // Update the recruitment post itself (the panel interaction is ephemeral,
    // so the post is edited directly).
    let new_embed = build_party_embed(&author_name, color, &quest, &location, &members, size, notes.as_deref(), &footer_text);
    if let Err(e) = party_msg.edit(&bnd.ctx.http, EditMessage::new().embed(new_embed).components(build_party_buttons(size, leader_id, now_full, approval_mode, pending_after.len()))).await {
        warn!(message_id = %party_msg_id, error = %e, "Failed to update party embed after approval");
    }

    // Refresh the leader's panel with the remaining requests.
    let (panel_embed, panel_rows) = build_requests_panel(party_msg_id, &pending_after);
    let response = CreateInteractionResponseMessage::new().embed(panel_embed).components(panel_rows).ephemeral(true);
    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;

    // Public notifications after the interaction is answered.
    if now_full && !was_assembled {
        let ping_msg = format!("🎉 **Party Assembled!** {} @ {} — hunters: {}", quest, location, members.join(" "));
        let _ = party_msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(ping_msg)).await;
    }
    let joined_msg = format!("✅ {} joined the party ({}/{}).", requester_mention, members.len(), size);
    let _ = party_msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(joined_msg)).await;
    Ok(())
}

/// Rejects one pending request: removes it, refreshes the panel, and informs
/// the requester in the channel.
async fn reject_request(bnd: &ComponentBundle<'_>, party_msg: Message, requester_id: u64) -> Result<(), MyErr> {
    let party_msg_id = party_msg.id.get();
    let still_pending = crate::PARTIES.lock().await.get(&party_msg_id.to_string()).map_or(false, |r| r.pending.iter().any(|p| p.user_id == requester_id));
    if !still_pending {
        let response = CreateInteractionResponseMessage::new().content("ℹ️ This request has already been handled.").ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        return Ok(());
    }

    remove_pending(party_msg_id, requester_id).await;
    let _ = Shutdown::save().await;
    let pending_after = pending_requests_of(party_msg_id).await;

    let (panel_embed, panel_rows) = build_requests_panel(party_msg_id, &pending_after);
    let response = CreateInteractionResponseMessage::new().embed(panel_embed).components(panel_rows).ephemeral(true);
    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;

    let decline_msg = format!("<@{}>, your request to join this party was declined.", requester_id);
    let _ = party_msg.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().content(decline_msg)).await;
    Ok(())
}

/// Closes party recruitment posts that saw no activity for 30 minutes.
/// Runs every minute from the reminder loop in `main.rs`. Posts that are
/// already closed (no buttons left) are simply dropped from tracking.
pub async fn sweep_inactive_parties(http: &Http) {
    let now = MyTime::now();
    let expired: Vec<PartyRecord> = {
        let parties = crate::PARTIES.lock().await;
        parties
            .values()
            .filter(|r| now - r.last_activity > PARTY_INACTIVITY_SECS)
            .cloned()
            .collect()
    };
    if expired.is_empty() {
        return;
    }
    for rec in expired {
        // Skip posts currently being processed by a button click; they will
        // be re-checked on the next sweep round.
        let lock_key = rec.message_id.to_string();
        if !reserve_party_lock(&lock_key).await {
            continue;
        }
        let channel = ChannelId::new(rec.channel_id);
        match channel.message(http, MessageId::new(rec.message_id)).await {
            Ok(mut msg) => {
                if !msg.components.is_empty() {
                    if let Some(embed) = msg.embeds.first() {
                        let state = parse_party_state(embed);
                        let size = parse_party_size_from_field(embed).unwrap_or_else(|| state.members.len().max(1));
                        let closed_embed = build_party_embed(AUTHOR_CLOSED_INACTIVE, COLOR_CLOSED, &state.quest, &state.location, &state.members, size, state.notes.as_deref(), &state.footer_text);
                        let _ = msg.edit(http, EditMessage::new().embed(closed_embed).components(Vec::<CreateActionRow>::new())).await;
                        let _ = channel.send_message(http, CreateMessage::new().content("⏰ **Party recruitment closed automatically** after 30 minutes of inactivity.")).await;
                    }
                }
            }
            Err(e) => {
                warn!(message_id = rec.message_id, error = %e, "Inactive party message no longer accessible");
            }
        }
        crate::PARTIES.lock().await.remove(&rec.message_id.to_string());
        release_party_lock(&lock_key).await;
    }
    let _ = Shutdown::save().await;
}
