use crate::event::interaction::{ComponentBundle, SlashBundle};
use crate::reusable::component::bounty::BountyTitle;
use crate::MyErr;
use crate::commands::admin::execute_guard;
use serde_json::Value;
use serenity::all::*;
use std::collections::HashMap;
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
                if entry_trigger.trim() == trigger.trim() {
                    let image_url = value["image"]["url"].as_str().unwrap_or("N/A");
                    let role_id = value["role_id"].as_u64().unwrap_or(0);
                    entries.push((key.clone(), format!("Image: `{}`, Role: `{}`", image_url.trim(), role_id)));
                }
            }
        }
    }
    if let Some(custom_arr) = data["custom"].as_array() {
        for (idx, value) in custom_arr.iter().enumerate() {
            if let Some(entry_trigger) = value["trigger"].as_str() {
                if entry_trigger.trim() == trigger.trim() {
                    let image_url = value["image"]["url"].as_str().unwrap_or("N/A");
                    let role_id = value["role_id"].as_u64().unwrap_or(0);
                    entries.push((format!("custom_{}", idx), format!("Image: `{}`, Role: `{}`", image_url.trim(), role_id)));
                }
            }
        }
    }
    entries
}

fn delete_entry_by_key(data: &mut Value, key: &str) -> bool {
    if let Some(obj) = data.as_object_mut() {
        if key != "custom" && obj.contains_key(key) {
            obj.remove(key);
            return true;
        }
    }
    if key.starts_with("custom_") {
        if let Some(idx_str) = key.strip_prefix("custom_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(custom_arr) = data["custom"].as_array_mut() {
                    if idx < custom_arr.len() {
                        custom_arr.remove(idx);
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[hertz::hertz_auto]
pub async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut focus = String::new();
    for i in &bnd.cmd.data.options {
        if let CommandDataOptionValue::Autocomplete { value, .. } = &i.value {
            focus = value.to_lowercase();
            break;
        }
    }

    let title = BountyTitle::new().await?;
    let mut all_titles = vec![
        &title.bronze_bounty, &title.silver_bounty, &title.gold_bounty,
        &title.bronze_trading, &title.silver_trading, &title.gold_trading,
    ];
    all_titles.extend(&title.custom);

    let mut trigger_groups: HashMap<String, Vec<String>> = HashMap::new();
    for t in all_titles {
        let raw_name = t.image.url.replace(".webp", "  ").replace(".jpg", "  ").replace(".png", "  ").replace("_", "   ");
        trigger_groups.entry(t.trigger.clone()).or_default().push(raw_name);
    }

    let mut temp_options: Vec<(String, String)> = Vec::new();
    for (trigger_key, names) in trigger_groups {
        let combined_names = names.join("   &   ");
        let trigger_desc = BountyTitle::name(&trigger_key);
        let display_name = format!("{} ({})", combined_names, trigger_desc);

        if display_name.to_lowercase().contains(&focus) || trigger_key.contains(&focus) {
            let mut safe_name = display_name;
            if safe_name.len() > 100 {
                safe_name.truncate(97);
                safe_name.push_str("...");
            }
            temp_options.push((safe_name, trigger_key));
        }
    }

    temp_options.sort_by(|a, b| a.0.cmp(&b.0));

    let options: Vec<AutocompleteChoice> = temp_options
        .into_iter()
        .take(25)
        .map(|(name, val)| AutocompleteChoice::new(name, val))
        .collect();

    bnd.cmd.create_response(
        &bnd.ctx.http,
        CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(options))
    ).await?;

    Ok(())
}

pub async fn handle_selection(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let custom_id = bnd.cmd.data.custom_id.as_str();
    let prefix = "title_delete_select_";
    
    if !custom_id.starts_with(prefix) {
        return Err(MyErr::Custom("Invalid button prefix".into()));
    }
    
    let rest = &custom_id[prefix.len()..];
    let last_underscore = rest.rfind('_').ok_or_else(|| MyErr::Custom("Invalid button format".into()))?;
    
    let index_str = &rest[last_underscore + 1..];
    let trigger = &rest[..last_underscore];
    let index: usize = index_str.parse().unwrap_or(0);

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
    let mut image_to_delete: Option<String> = None;

    if let Some(custom_arr) = data["custom"].as_array() {
        for (idx, value) in custom_arr.iter().enumerate() {
            if let Some(entry_trigger) = value["trigger"].as_str() {
                if entry_trigger.trim() == trigger.trim() {
                    if match_count == index {
                        entry_key = format!("custom_{}", idx);
                        found = true;
                        if let Some(img) = value["image"]["url"].as_str() {
                            image_to_delete = Some(img.trim().to_string());
                        }
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

    delete_entry_by_key(&mut data, &entry_key);

    let new_content = serde_json::to_string_pretty(&data)?;
    fs::write(json_path, new_content).await?;

    if let Some(img_name) = image_to_delete {
        let image_dir = "image/bounty_title";
        let extensions = ["gif", "png", "jpg", "jpeg", "webp"];
        for ext in &extensions {
            let file_path = format!("{}/{}.{}", image_dir, img_name, ext);
            if fs::remove_file(&file_path).await.is_ok() {
                info!("Deleted image file: {}", file_path);
                break;
            }
        }
    }

    info!("Deleted title trigger: {} (entry: {})", trigger, entry_key);

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!("✅ **Title deleted successfully!**\nTrigger: `{}`\nEntry: `{}`", trigger, entry_key))
            .ephemeral(true)
    )).await?;

    Ok(())
}

pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }
    
    bnd.cmd.defer_ephemeral(&bnd.ctx.http).await?;

    let trigger = bnd.cmd.data.options.first()
        .and_then(|opt| {
            if let CommandDataOptionValue::String(s) = &opt.value {
                Some(s.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| MyErr::Custom("Trigger is required".into()))?;

    let json_path = "static/bounty_title.json";
    let content = match fs::read_to_string(json_path).await {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read bounty_title.json: {:?}", e);
            return send_followup(bnd, "❌ **Error:** Could not read bounty_title.json.").await;
        }
    };

    let data: Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to parse bounty_title.json: {:?}", e);
            return send_followup(bnd, "❌ **Error:** Invalid JSON format.").await;
        }
    };

    let matching_entries = find_entry_keys_with_trigger(&data, &trigger);

    if matching_entries.is_empty() {
        return send_followup(bnd, &format!("❌ **Error:** No title found with trigger `{}`.", trigger)).await;
    }

    if matching_entries.len() == 1 {
        let entry_key = matching_entries[0].0.clone();
        let mut data_mut: Value = serde_json::from_str(&content).unwrap();
        
        let mut image_to_delete: Option<String> = None;
        if let Some(obj) = data_mut.as_object() {
            if entry_key != "custom" {
                if let Some(value) = obj.get(&entry_key) {
                    if let Some(img) = value["image"]["url"].as_str() {
                        image_to_delete = Some(img.trim().to_string());
                    }
                }
            }
        }
        if let Some(custom_arr) = data_mut["custom"].as_array() {
            if entry_key.starts_with("custom_") {
                if let Some(idx_str) = entry_key.strip_prefix("custom_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx < custom_arr.len() {
                            if let Some(img) = custom_arr[idx]["image"]["url"].as_str() {
                                image_to_delete = Some(img.trim().to_string());
                            }
                        }
                    }
                }
            }
        }
        
        delete_entry_by_key(&mut data_mut, &entry_key);
        
        let new_content = serde_json::to_string_pretty(&data_mut)?;
        fs::write(json_path, new_content).await?;
        
        if let Some(img_name) = image_to_delete {
            let image_dir = "image/bounty_title";
            let extensions = ["gif", "png", "jpg", "jpeg", "webp"];
            for ext in &extensions {
                let file_path = format!("{}/{}.{}", image_dir, img_name, ext);
                if fs::remove_file(&file_path).await.is_ok() {
                    info!("Deleted image file: {}", file_path);
                    break;
                }
            }
        }
        
        info!("Deleted title trigger: {} (entry: {})", trigger, entry_key);
        return send_followup(bnd, &format!("✅ **Title deleted successfully!**\nTrigger: `{}`\nEntry: `{}`", trigger, entry_key)).await;
    }

    let mut selection_msg = format!("⚠️ **Multiple Entries Found for Trigger `{}`**\n\nPlease select which entry to delete:\n\n", trigger);

    for (i, (key, info)) in matching_entries.iter().enumerate() {
        let entry_type = if key.starts_with("custom_") { "Custom" } else { "Standard" };
        selection_msg.push_str(&format!("**{}** - `{}`\n• Type: {}\n• {}\n\n", i + 1, key, entry_type, info));
    }

    let mut buttons = Vec::new();
    for (i, _) in matching_entries.iter().enumerate() {
        if i < 5 {
            buttons.push(CreateButton::new(
                format!("title_delete_select_{}_{}", trigger, i)
            ).label(format!("{}", i + 1)).style(ButtonStyle::Danger));
        }
    }

    let action_row = CreateActionRow::Buttons(buttons);

    let followup = CreateInteractionResponseFollowup::new()
        .content(&selection_msg)
        .ephemeral(true)
        .components(vec![action_row]);

    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;

    Ok(())
}