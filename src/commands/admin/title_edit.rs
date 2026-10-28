use crate::event::interaction::{ComponentBundle, SlashBundle};
use crate::MyErr;
use crate::commands::admin::execute_guard;
use crate::reusable::cache::{make_key, take, new_pending_edit, save};
use serde_json::{json, Value};
use serenity::all::{AttachmentId, ButtonStyle, CommandDataOptionValue, CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage};
use tokio::fs;
use tracing::{info, warn};

async fn send_followup(bnd: &SlashBundle<'_>, content: &str) -> Result<(), MyErr> {
    let followup = CreateInteractionResponseFollowup::new()
        .content(content)
        .ephemeral(true);
    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
    Ok(())
}

fn find_entry_keys_with_trigger(data: &Value, trigger: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Some(obj) = data.as_object() {
        for (key, value) in obj.iter() {
            if key == "custom" { continue; }
            if let Some(entry_trigger) = value["trigger"].as_str() {
                if entry_trigger == trigger {
                    let image_url = value["image"]["url"].as_str().unwrap_or("N/A");
                    let role_id = value["role_id"].as_u64().unwrap_or(0);
                    entries.push((key.clone(), format!("Image: `{}`, Role: `{}`", image_url, role_id)));
                }
            }
        }
    }
    if let Some(custom_arr) = data["custom"].as_array() {
        for (idx, value) in custom_arr.iter().enumerate() {
            if let Some(entry_trigger) = value["trigger"].as_str() {
                if entry_trigger == trigger {
                    let image_url = value["image"]["url"].as_str().unwrap_or("N/A");
                    let role_id = value["role_id"].as_u64().unwrap_or(0);
                    entries.push((format!("custom_{}", idx), format!("Image: `{}`, Role: `{}`", image_url, role_id)));
                }
            }
        }
    }
    entries
}

fn update_entry_by_key(data: &mut Value, key: &str, update_fn: impl FnOnce(&mut Value)) -> bool {
    if let Some(obj) = data.as_object_mut() {
        if key != "custom" && obj.contains_key(key) {
            if let Some(entry) = obj.get_mut(key) { update_fn(entry); return true; }
        }
    }
    if key.starts_with("custom_") {
        if let Some(idx_str) = key.strip_prefix("custom_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(custom_arr) = data["custom"].as_array_mut() {
                    if idx < custom_arr.len() { update_fn(&mut custom_arr[idx]); return true; }
                }
            }
        }
    }
    false
}

pub async fn handle_selection(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let custom_id = bnd.cmd.data.custom_id.as_str();
    let prefix = "title_edit_select_";
    if !custom_id.starts_with(prefix) { return Err(MyErr::Custom("Invalid button prefix".into())); }

    let rest = &custom_id[prefix.len()..];
    let last_underscore = rest.rfind('_').ok_or_else(|| MyErr::Custom("Invalid button format".into()))?;
    let index: usize = rest[last_underscore + 1..].parse().unwrap_or(0);
    let cache_key = &rest[..last_underscore];

    let pending = match take(cache_key).await {
        Some(p) => p,
        None => {
            let msg = CreateInteractionResponseMessage::new().content("❌ **Error:** Edit session expired. Please run `/title_edit` again.").ephemeral(true);
            bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(msg)).await?;
            return Ok(());
        }
    };

    if pending.user_id != bnd.cmd.user.id.get() {
        let msg = CreateInteractionResponseMessage::new().content("❌ **Error:** This button is not intended for your use.").ephemeral(true);
        bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(msg)).await?;
        return Ok(());
    }

    let json_path = "static/bounty_title.json";
    let json_content = match fs::read_to_string(json_path).await {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read bounty_title.json: {:?}", e);
            bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("❌ **Error:** Could not read bounty_title.json.").ephemeral(true)
            )).await?;
            return Ok(());
        }
    };

    let mut data: Value = match serde_json::from_str(&json_content) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to parse bounty_title.json: {:?}", e);
            bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("❌ **Error:** Invalid JSON format.").ephemeral(true)
            )).await?;
            return Ok(());
        }
    };

    let mut found = false;
    let mut entry_key = String::new();
    let mut match_count = 0;

    if let Some(custom_arr) = data["custom"].as_array_mut() {
        for (idx, value) in custom_arr.iter_mut().enumerate() {
            if let Some(entry_trigger) = value["trigger"].as_str() {
                if entry_trigger == pending.trigger {
                    if match_count == index {
                        entry_key = format!("custom_{}", idx);
                        found = true;

                        if let Some(img_name) = &pending.saved_image_filename {
                            value["image"]["url"] = json!(img_name);
                            info!("Applied pre-saved image: {}", img_name);
                        }
                        if let Some(d) = pending.new_diameter { value["image"]["diameter"] = json!(d); }
                        if let Some(x) = pending.new_x_start { value["image"]["x_start"] = json!(x); }
                        if let Some(y) = pending.new_y_start { value["image"]["y_start"] = json!(y); }
                        if let Some(a) = pending.new_animated { value["image"]["animated"] = json!(a); }
                        if let Some(w) = pending.new_bg_width { value["image"]["bg_width"] = json!(w); }
                        if let Some(h) = pending.new_bg_height { value["image"]["bg_height"] = json!(h); }
                        if let Some(rid) = pending.new_role_id { value["role_id"] = json!(rid); }
                        
                        break;
                    }
                    match_count += 1;
                }
            }
        }
    }

    if !found {
        bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(format!("❌ **Error:** Entry at index {} not found.", index)).ephemeral(true)
        )).await?;
        return Ok(());
    }

    let new_content = serde_json::to_string_pretty(&data)?;
    fs::write(json_path, new_content).await?;
    
    info!("User selected entry {} for trigger {} (key: {})", index, pending.trigger, entry_key);
    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().content(format!("✅ **Entry {} updated successfully!**\nTrigger: `{}`", index, pending.trigger)).ephemeral(true)
    )).await?;
    Ok(())
}

pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await { return Err(MyErr::Custom("Access Denied".into())); }
    bnd.cmd.defer_ephemeral(&bnd.ctx.http).await?;

    let opts = bnd.cmd.data.options.clone();
    let get_attachment_id = |name: &str| -> Option<AttachmentId> {
        opts.iter().find(|o| o.name == name).and_then(|o| if let CommandDataOptionValue::Attachment(a) = &o.value { Some(*a) } else { None })
    };
    let get_str = |name: &str| -> Option<String> {
        opts.iter().find(|o| o.name == name).and_then(|o| if let CommandDataOptionValue::String(s) = &o.value { Some(s.clone()) } else { None })
    };
    let get_int = |name: &str| -> Option<i64> {
        opts.iter().find(|o| o.name == name).and_then(|o| if let CommandDataOptionValue::Integer(v) = &o.value { Some(*v) } else { None })
    };
    let get_bool = |name: &str| -> Option<bool> {
        opts.iter().find(|o| o.name == name).and_then(|o| if let CommandDataOptionValue::Boolean(v) = &o.value { Some(*v) } else { None })
    };

    let category = get_str("bounty").ok_or_else(|| MyErr::Custom("Bounty category is required.".into()))?;
    let bounty_no = get_str("bounty_no").ok_or_else(|| MyErr::Custom("Bounty number is required.".into()))?;
    let attachment_id = get_attachment_id("image");
    let new_diameter = get_int("diameter");
    let new_x_start = get_int("x_start");
    let new_y_start = get_int("y_start");
    let new_animated = get_bool("animated");
    let new_bg_width = get_int("bg_width");
    let new_bg_height = get_int("bg_height");
    let new_role_id = get_int("role_id").map(|v| v as u64);

    let prefix = match category.as_str() { "Bronze" => "0", "Silver" => "1", "Gold" => "2", "Event" => "4", "Free" => "0", _ => "3" };
    let suffix = bounty_no.trim_start_matches(|c: char| !c.is_digit(10));
    if suffix.is_empty() { return send_followup(bnd, "❌ **Error:** Invalid Bounty Number format.").await; }
    let trigger = format!("{}_{}", prefix, suffix);

    let json_path = "static/bounty_title.json";
    let content = match fs::read_to_string(json_path).await {
        Ok(c) => c,
        Err(e) => { warn!("Failed to read bounty_title.json: {:?}", e); return send_followup(bnd, "❌ **Error:** Could not read bounty_title.json.").await; }
    };
    let mut data: Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => { warn!("Failed to parse bounty_title.json: {:?}", e); return send_followup(bnd, "❌ **Error:** Invalid JSON format.").await; }
    };

    let matching_entries = find_entry_keys_with_trigger(&data, &trigger);
    if matching_entries.is_empty() { return send_followup(bnd, &format!("❌ **Error:** No title found with trigger `{}`.", trigger)).await; }

    if matching_entries.len() > 1 {
        let mut saved_image_filename: Option<String> = None;
        
        if let Some(att_id) = attachment_id {
            if let Some(attachment) = bnd.cmd.data.resolved.attachments.get(&att_id).cloned() {
                let file_name = &attachment.filename;
                let image_name = file_name.rsplit_once('.').map(|(name, _)| name).unwrap_or(file_name).to_string();
                let image_dir = "image/bounty_title";
                if let Err(_e) = fs::create_dir_all(image_dir).await {
                    return send_followup(bnd, "❌ **Error:** Could not create image directory.").await;
                }
                match attachment.download().await {
                    Ok(image_bytes) => {
                        let extension = file_name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("png");
                        let file_path = format!("{}/{}.{}", image_dir, image_name, extension);
                        if let Err(_e) = fs::write(&file_path, &image_bytes).await {
                            return send_followup(bnd, "❌ **Error:** Could not save image file.").await;
                        }
                        saved_image_filename = Some(image_name);
                        info!("Pre-saved image to: {}", file_path);
                    }
                    Err(e) => { warn!("Failed to download attachment: {:?}", e); return send_followup(bnd, "❌ **Error:** Could not download image.").await; }
                }
            }
        }

        let cache_key = make_key(bnd.cmd.user.id.get(), &trigger);
        let pending = new_pending_edit(
            bnd.cmd.user.id.get(), trigger.clone(), saved_image_filename,
            new_diameter, new_x_start, new_y_start, new_animated, new_bg_width, new_bg_height, new_role_id,
        );
        save(cache_key.clone(), pending).await;
        info!("Saved pending edit for key: {}", cache_key);

        let mut selection_msg = format!("⚠️ **Multiple Entries Found for Trigger `{}`**\n\nPlease select which entry to edit:\n\n", trigger);
        for (i, (key, info)) in matching_entries.iter().enumerate() {
            let entry_type = if key.starts_with("custom_") { "Custom" } else { "Standard" };
            selection_msg.push_str(&format!("**{}** - `{}`\n• Type: {}\n• {}\n\n", i + 1, key, entry_type, info));
        }

        let mut buttons = Vec::new();
        for (i, _) in matching_entries.iter().enumerate() {
            if i < 5 {
                buttons.push(CreateButton::new(format!("title_edit_select_{}_{}", cache_key, i)).label(format!("{}", i + 1)).style(ButtonStyle::Primary));
            }
        }

        let followup = CreateInteractionResponseFollowup::new().content(&selection_msg).ephemeral(true).components(vec![CreateActionRow::Buttons(buttons)]);
        bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
        return Ok(());
    }

    let entry_key = matching_entries[0].0.clone();
    let updated = update_entry_by_key(&mut data, &entry_key, |entry| {
        if let Some(d) = new_diameter { entry["image"]["diameter"] = json!(d); }
        if let Some(x) = new_x_start { entry["image"]["x_start"] = json!(x); }
        if let Some(y) = new_y_start { entry["image"]["y_start"] = json!(y); }
        if let Some(a) = new_animated { entry["image"]["animated"] = json!(a); }
        if let Some(w) = new_bg_width { entry["image"]["bg_width"] = json!(w); }
        if let Some(h) = new_bg_height { entry["image"]["bg_height"] = json!(h); }
        if let Some(rid) = new_role_id { entry["role_id"] = json!(rid); }
    });

    if !updated { return send_followup(bnd, "❌ **Error:** Failed to locate entry for editing.").await; }

    if let Some(att_id) = attachment_id {
        let attachment = bnd.cmd.data.resolved.attachments.get(&att_id).cloned()
            .ok_or_else(|| MyErr::Custom("Failed to find attachment".into()))?;
        let file_name = &attachment.filename;
        let image_name = file_name.rsplit_once('.').map(|(name, _)| name).unwrap_or(file_name).to_string();
        let image_dir = "image/bounty_title";
        fs::create_dir_all(image_dir).await?;
        let image_bytes = attachment.download().await.map_err(|e| MyErr::Custom(format!("Failed to download: {:?}", e)))?;
        let extension = file_name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("png");
        let file_path = format!("{}/{}.{}", image_dir, image_name, extension);
        fs::write(&file_path, &image_bytes).await?;
        
        update_entry_by_key(&mut data, &entry_key, |entry| { entry["image"]["url"] = json!(image_name); });
    }

    let new_content = serde_json::to_string_pretty(&data)?;
    fs::write(json_path, new_content).await?;
    info!("Updated title trigger: {} (entry: {})", trigger, entry_key);
    send_followup(bnd, &format!("✅ **Title `{}` (entry: `{}`) updated successfully!**", trigger, entry_key)).await
}