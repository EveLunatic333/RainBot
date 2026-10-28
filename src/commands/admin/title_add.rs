use crate::event::interaction::SlashBundle;
use crate::MyErr;
use crate::commands::admin::execute_guard;
use serde_json::{json, Value};
use serenity::all::{AttachmentId, CommandDataOptionValue, CreateInteractionResponseFollowup};
use tokio::fs;
use tracing::{info, warn};

async fn send_followup(bnd: &SlashBundle<'_>, content: &str) -> Result<(), MyErr> {
    let followup = CreateInteractionResponseFollowup::new()
        .content(content)
        .ephemeral(true);
    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
    Ok(())
}

pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }

    bnd.cmd.defer_ephemeral(&bnd.ctx.http).await?;
    let opts = bnd.cmd.data.options.clone();
    let get_attachment_id = |name: &str| -> Option<AttachmentId> {
        opts.iter().find(|o| o.name == name).and_then(|o| {
            if let CommandDataOptionValue::Attachment(att_id) = &o.value {
                Some(*att_id)
            } else { None }
        })
    };
    
    let get_str = |name: &str| -> Option<String> {
        opts.iter().find(|o| o.name == name).and_then(|o| {
            if let CommandDataOptionValue::String(s) = &o.value {
                Some(s.clone())
            } else { None }
        })
    };
    
    let get_int = |name: &str| -> Option<i64> {
        opts.iter().find(|o| o.name == name).and_then(|o| {
            if let CommandDataOptionValue::Integer(v) = &o.value {
                Some(*v)
            } else { None }
        })
    };
    
    let get_bool = |name: &str| -> Option<bool> {
        opts.iter().find(|o| o.name == name).and_then(|o| {
            if let CommandDataOptionValue::Boolean(v) = &o.value {
                Some(*v)
            } else { None }
        })
    };

    let attachment_id = match get_attachment_id("image") {
        Some(id) => id,
        None => {
            send_followup(bnd, "❌ **Error:** Image attachment is required.").await?;
            return Ok(());
        }
    };

    let attachment = bnd.cmd.data.resolved.attachments
        .get(&attachment_id)
        .cloned()
        .ok_or_else(|| MyErr::Custom("Failed to find attachment in resolved data".into()))?;

    let diameter = get_int("diameter").ok_or_else(|| MyErr::Custom("Diameter is required".into()))?;
    let x_start = get_int("x_start").ok_or_else(|| MyErr::Custom("x_start is required".into()))?;
    let y_start = get_int("y_start").ok_or_else(|| MyErr::Custom("y_start is required".into()))?;
    let animated = get_bool("animated").unwrap_or(false);
    let bg_width = get_int("bg_width").ok_or_else(|| MyErr::Custom("bg_width is required".into()))?;
    let bg_height = get_int("bg_height").ok_or_else(|| MyErr::Custom("bg_height is required".into()))?;

    let category = get_str("bounty").ok_or_else(|| MyErr::Custom("Bounty category is required".into()))?;
    let bounty_no = get_str("bounty_no").ok_or_else(|| MyErr::Custom("Bounty number is required".into()))?;
    let role_id = get_int("role_id").unwrap_or(0) as u64;

    let prefix = match category.as_str() {
        "Bronze" => "0",
        "Silver" => "1",
        "Gold" => "2",
        "Event" => "4", 
        "Free" => "0",
        _ => "3"
    };

    let suffix = bounty_no.trim_start_matches(|c: char| !c.is_digit(10));
    if suffix.is_empty() {
        send_followup(bnd, "❌ **Error:** Invalid Bounty Number format. Must contain digits (e.g., BBQ12).").await?;
        return Ok(());
    }

    let trigger = format!("{}_{}", prefix, suffix);
    let file_name = &attachment.filename;
    let image_name = file_name.rsplit_once('.').map(|(name, _)| name).unwrap_or(file_name).to_string();
    let image_dir = "image/bounty_title";
    if let Err(e) = fs::create_dir_all(image_dir).await {
        warn!("Failed to create image directory: {:?}", e);
        send_followup(bnd, "❌ **Error:** Could not create image directory.").await?;
        return Ok(());
    }

    let image_bytes = attachment.download().await
        .map_err(|e| {
            warn!("Failed to download attachment: {:?}", e);
            MyErr::Custom("Failed to download image".into())
        })?;

    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("png");
    let file_path = format!("{}/{}.{}", image_dir, image_name, extension);

    if let Err(e) = fs::write(&file_path, &image_bytes).await {
        warn!("Failed to save image to {}: {:?}", file_path, e);
        send_followup(bnd, "❌ **Error:** Could not save image file.").await?;
        return Ok(());
    }

    info!("Saved image to: {}", file_path);

    let json_path = "static/bounty_title.json";

    let content = match fs::read_to_string(json_path).await {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read bounty_title.json: {:?}", e);
            send_followup(bnd, "❌ **Error:** Could not read bounty_title.json file.").await?;
            return Ok(());
        }
    };

    let mut data: Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to parse bounty_title.json: {:?}", e);
            send_followup(bnd, "❌ **Error:** bounty_title.json format is invalid.").await?;
            return Ok(());
        }
    };

    let new_entry = json!({
        "image": {
            "url": image_name,
            "diameter": diameter,
            "x_start": x_start,
            "y_start": y_start,
            "animated": animated,
            "bg_width": bg_width,
            "bg_height": bg_height
        },
        "trigger": trigger,
        "role_id": role_id,
        "db_code": 0
    });

    if let Some(custom_arr) = data["custom"].as_array_mut() {
        custom_arr.push(new_entry);
    } else {
        data["custom"] = json!([new_entry]);
    }

    let new_content = serde_json::to_string_pretty(&data)?;
    if let Err(e) = fs::write(json_path, new_content).await {
        warn!("Failed to write bounty_title.json: {:?}", e);
        send_followup(bnd, "❌ **Error:** Could not save changes to bounty_title.json.").await?;
        return Ok(());
    }

    info!("Added new title trigger: {} with image: {}", trigger, image_name);

    send_followup(bnd, &format!(
        "✅ **Title Added Successfully!**\n\n\
        📷 **Image:** `{}`\n\
        💾 **Saved to:** `{}`\n\
        🚩 **Trigger:** `{}`\n\
        🆔 **Role ID:** `{}`\n\
        📐 **Coords:** X: {}, Y: {}\n\
        📏 **Size:** {}x{}",
        image_name, file_path, trigger, role_id, x_start, y_start, bg_width, bg_height
    )).await?;

    Ok(())
}