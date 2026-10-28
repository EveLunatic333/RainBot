use serenity::builder::{CreateAttachment, EditInteractionResponse, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::all::*;
use crate::{Reg, SlashBundle, MyErr};
use crate::reusable::image_edit::huntercard::{HunterCardEngine, RoleRenderData};
use crate::reusable::component::sightengine;
use image::Rgba;
use tokio::{fs, sync::Mutex};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{warn, instrument};
use futures::future::join_all;

static FILE_LOCKS: LazyLock<Mutex<HashMap<i32, Arc<Mutex<()>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn format_dots(num: i64) -> String {
    let s = num.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(bytes.len() + bytes.len() / 3);
    let len = bytes.len();
    for i in 0..len {
        result.push(bytes[i] as char);
        let remaining = len - i - 1;
        if remaining > 0 && remaining % 3 == 0 && bytes[i] != b'-' {
            result.push('.');
        }
    }
    result
}

fn calculate_hr(hrp: i32) -> String {
    match hrp {
        n if n >= 999 => "7",
        n if n >= 300 => "6",
        n if n >= 100 => "5",
        n if n >= 51  => "4",
        n if n >= 31  => "3",
        n if n >= 11  => "2",
        _             => "1",
    }.to_string()
}

fn format_playtime(seconds: i32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    format!("{h}H:{m:02}M:{s:02}S")
}

async fn send_ephemeral_response(bnd: &SlashBundle<'_>, content: &str) {
    let _ = bnd.cmd.create_response(
        &bnd.ctx.http,
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(content).ephemeral(true))
    ).await;
}

async fn fetch_image_bytes(url: &str) -> Option<Vec<u8>> {
    reqwest::get(url).await.ok()?.bytes().await.ok().map(|b| b.to_vec())
}

async fn fetch_role_icons_parallel(ctx: &serenity::prelude::Context, guild_id: GuildId, user_role_ids: &[u64]) -> Vec<RoleRenderData> {
    let mut sorted_roles: Vec<Role> = ctx.cache.guild(guild_id)
        .map(|g| user_role_ids.iter().filter_map(|rid| g.roles.get(&RoleId::new(*rid)).cloned()).collect())
        .unwrap_or_else(|| {
            tracing::warn!("Guild {} not found in cache", guild_id.get());
            Vec::new()
        });

    sorted_roles.sort_by(|a, b| b.position.cmp(&a.position));

    let icon_futures = sorted_roles.iter().take(6).map(|r| async move {
        let icon_bytes = fetch_role_icon(r).await;
        RoleRenderData {
            name: r.name.clone(),
            color: Rgba([r.colour.r(), r.colour.g(), r.colour.b(), 255]),
            icon_bytes,
        }
    });

    join_all(icon_futures).await
}

async fn fetch_role_icon(role: &Role) -> Option<Vec<u8>> {
    if let Some(hash) = &role.icon {
        let url = format!("https://cdn.discordapp.com/role-icons/{}/{}.png", role.id, hash);
        if let Some(bytes) = fetch_image_bytes(&url).await {
            return Some(bytes);
        }
    }
    if let Some(emoji) = &role.unicode_emoji {
        let hex = emoji.chars()
            .filter(|&c| c as u32 != 0xfe0f)
            .map(|c| format!("{:x}", c as u32))
            .collect::<Vec<_>>()
            .join("-");
        let url = format!("https://cdnjs.cloudflare.com/ajax/libs/twemoji/14.0.2/72x72/{hex}.png");
        return fetch_image_bytes(&url).await;
    }
    None
}

#[hertz::hertz_slash_reg(10, true)]
#[instrument(skip(bnd, reg), fields(discord_id = %bnd.cmd.user.id))]
pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    if !bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true) {
        send_ephemeral_response(bnd, &format!("❌ **Command Disabled:** `{cmd_name}` is deactivated by Administrator.")).await;
        return Ok(());
    }

    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;

    let subcommand = bnd.cmd.data.options.first()
        .ok_or_else(|| MyErr::Custom("Missing subcommand option".into()))?;
    let sub_name = subcommand.name.as_str();
    let sub_opts = if let CommandDataOptionValue::SubCommand(opts) = &subcommand.value {
        opts.clone()
    } else {
        Vec::new()
    };

    let storage_dir = Path::new(".").join("static").join("huntercards");
    fs::create_dir_all(&storage_dir).await.map_err(|e| MyErr::Custom(format!("Failed to create storage directory: {e}")))?;

    match sub_name {
        "edit" => handle_edit_subcommand(bnd, reg, &storage_dir, &sub_opts).await,
        "show" | "" => handle_show_subcommand(bnd, reg, &storage_dir, &sub_opts).await,
        _ => Ok(()),
    }
}

#[instrument(skip(bnd, reg, storage_dir, sub_opts))]
async fn handle_edit_subcommand(bnd: &SlashBundle<'_>, reg: &Reg<'_>, storage_dir: &Path, sub_opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let lock = {
        let mut lock_map = FILE_LOCKS.lock().await;
        lock_map.entry(reg.cid).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    };
    let _guard = lock.lock().await;
    let mut update_count = 0;

    for opt in sub_opts {
        match opt.name.as_str() {
            "character" | "equipment" => {
                if let CommandDataOptionValue::Attachment(att_id) = &opt.value {
                    if let Some(att) = bnd.cmd.data.resolved.attachments.get(att_id) {
                        // Moderation degrades to "allow" when the API keys are not
                        // configured, so a fresh clone without moderation still works.
                        let flagged = match sightengine::SIGHTENGINE_KEYS.as_ref() {
                            Some(keys) => sightengine::scan_image(&att.url, &keys.api_user, &keys.api_secret).await,
                            None => false,
                        };
                        if flagged {
                            let msg = format!("🛑 **Access Denied:** {} image violates content safety policies.", opt.name);
                            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(msg)).await.ok();
                            return Ok(());
                        }
                        if let Ok(bytes) = att.download().await {
                            let suffix = if opt.name == "character" { "char" } else { "equip" };
                            let file_path = storage_dir.join(format!("{}_{}.jpg", reg.cid, suffix));
                            if fs::write(&file_path, &bytes).await.is_ok() {
                                update_count += 1;
                            } else {
                                warn!(cid = reg.cid, file = ?file_path, "Failed to write asset file");
                            }
                        }
                    }
                }
            }
            "note" => {
                if let CommandDataOptionValue::String(s) = &opt.value {
                    let file_path = storage_dir.join(format!("{}_note.txt", reg.cid));
                    if fs::write(&file_path, s).await.is_ok() {
                        update_count += 1;
                    }
                }
            }
            "title" => {
                if let CommandDataOptionValue::String(s) = &opt.value {
                    let words: Vec<&str> = s.split_whitespace().collect();
                    if words.len() > 3 {
                        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ Failed: Hunter Title is limited to maximum 3 words.")).await.ok();
                        return Ok(());
                    }
                    let file_path = storage_dir.join(format!("{}_title.txt", reg.cid));
                    if fs::write(&file_path, words.join("  ")).await.is_ok() {
                        update_count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("✅ Hunter Card updated! ({update_count}/4 sections modified)"))).await.ok();
    Ok(())
}

#[instrument(skip(bnd, reg, storage_dir, sub_opts))]
async fn handle_show_subcommand(bnd: &SlashBundle<'_>, reg: &Reg<'_>, storage_dir: &Path, sub_opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let mut target_id = reg.cid;
    let mut target_user = bnd.cmd.user.clone();
    let mut user_roles: Vec<u64> = bnd.cmd.member.as_ref()
        .map(|m| m.roles.iter().map(|r| r.get()).collect())
        .unwrap_or_default();

    for opt in sub_opts {
        if opt.name == "user" {
            if let CommandDataOptionValue::User(uid) = &opt.value {
                if let Ok(cid) = reg.pg.get_cid_from_discord(uid.get()).await {
                    target_id = cid;
                    if let Some(user) = bnd.cmd.data.resolved.users.get(uid) {
                        target_user = user.clone();
                    }
                    if let Some(member) = bnd.cmd.data.resolved.members.get(uid) {
                        user_roles = member.roles.iter().map(|r| r.get()).collect();
                    }
                } else {
                    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("❌ User <@{}> is not a registered Hunter.", uid.get()))).await.ok();
                    return Ok(());
                }
            }
        }
    }

    let char_img = fs::read(storage_dir.join(format!("{target_id}_char.jpg"))).await.ok();
    let equip_img = fs::read(storage_dir.join(format!("{target_id}_equip.jpg"))).await.ok();
    let note = fs::read_to_string(storage_dir.join(format!("{target_id}_note.txt"))).await.unwrap_or_else(|_| "-".into());
    let title = fs::read_to_string(storage_dir.join(format!("{target_id}_title.txt"))).await.unwrap_or_else(|_| " ".into());

    let card = reg.pg.get_card(target_id).await?;
    let target_did_str = target_user.id.get().to_string();
    let event = reg.pg.get_target_event(&target_did_str).await?;

    let mut avatar_url = target_user.face().replace(".webp", ".png");
    if let Some(pos) = avatar_url.find("?size=") {
        avatar_url.truncate(pos);
    }
    avatar_url.push_str("?size=256");
    let avatar_bytes = fetch_image_bytes(&avatar_url).await;

    let roles_render = if let Some(gid) = bnd.cmd.guild_id {
        fetch_role_icons_parallel(&bnd.ctx, gid, &user_roles).await
    } else {
        Vec::new()
    };

    let data_map = HashMap::from([
        ("name".into(), card.name),
        ("username".into(), card.username),
        ("hr".into(), calculate_hr(card.hrp)),
        ("gr".into(), card.gr.to_string()),
        ("guild".into(), card.guild_name.unwrap_or_else(|| "No Guild".into())),
        ("playtime".into(), format_playtime(card.playtime)),
        ("bc".into(), format_dots(event.bounty as i64)),
        ("gacha_tkt".into(), format_dots(event.gacha as i64)),
        ("hunter_note".into(), note),
        ("hunter_title".into(), title),
        ("premi_gacha".into(), format_dots(event.gacha_premium as i64)),
        ("bronze".into(), format!("BBQ{}", event.bronze)),
        ("silver".into(), format!("BBQ{}", event.silver)),
        ("gold".into(), format!("BBQ{}", event.gold)),
    ]);

    match HunterCardEngine::generate(user_roles, data_map, char_img, equip_img, avatar_bytes, roles_render).await {
        Ok(img) => {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let unique_filename = format!("huntercard_{timestamp}.jpg");
            let attachment = CreateAttachment::bytes(img, unique_filename);
            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().new_attachment(attachment)).await.ok();
        }
        Err(e) => {
            warn!(cid = target_id, error = %e, "HunterCard render engine failed");
            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("❌ Render Engine Failure: {e}"))).await.ok();
        }
    }

    Ok(())
}