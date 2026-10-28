use crate::event::interaction::SlashBundle;
use crate::MyErr;
use crate::commands::admin::execute_guard;
use crate::reusable::component::Components;
use serenity::all::{RoleId, CommandDataOptionValue, CreateInteractionResponse, CreateInteractionResponseMessage};
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::info;
use crate::AUTO_CACHE;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResponse {
    pub trigger: String,
    pub response: String,
    pub embed: Option<AutoResponseEmbed>,
    pub case_sensitive: bool,
    pub regex: Option<String>,
    pub cooldown: u64,
    pub roles_required: Vec<u64>,
    pub channels_allowed: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResponseEmbed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: u32,
    pub thumbnail: Option<String>,
    pub image: Option<String>,
    pub footer: Option<String>,
    pub fields: Option<Vec<AutoResponseEmbedField>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResponseEmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResponseConfig {
    pub responses: Vec<AutoResponse>,
}

#[derive(Debug)]
pub struct AutoResponseCache {
    pub responses: HashMap<String, AutoResponse>,
    pub cooldowns: HashMap<(u64, String), i64>,
}

fn parse_escape_sequences(text: &str) -> String {
    text.replace("\\n", "\n").replace("\\t", "\t").replace("\\r", "\r")
}

fn parse_id_list(input: &str) -> Vec<u64> {
    input.split([',', ' '])
        .filter_map(|s| s.trim().parse::<u64>().ok())
        .collect()
}

fn parse_fields_input(input: &str) -> Result<Option<Vec<AutoResponseEmbedField>>, MyErr> {
    let mut fields = Vec::new();
    for field_str in input.split(";;") {
        let field_str = field_str.trim();
        if field_str.is_empty() {
            continue;
        }
        if let Some((name, value)) = field_str.split_once('|') {
            fields.push(AutoResponseEmbedField {
                name: parse_escape_sequences(name.trim()),
                value: parse_escape_sequences(value.trim()),
                inline: false,
            });
        } else {
            return Err(MyErr::Custom(format!("Invalid field format: '{}'. Use format: 'name|value'. Separate multiple fields with ';;'", field_str)));
        }
    }
    if fields.is_empty() {
        Ok(None)
    } else {
        Ok(Some(fields))
    }
}

impl AutoResponseConfig {
    pub fn path() -> std::path::PathBuf {
        Path::new(".").join("static").join("autoresponse.json")
    }

    pub async fn load() -> Result<Self, MyErr> {
        let path = Self::path();
        if !path.exists() {
            let default = AutoResponseConfig { responses: vec![] };
            default.save().await?;
            return Ok(default);
        }
        let file = tokio::fs::read_to_string(path).await?;
        let config: AutoResponseConfig = serde_json::from_str(&file)?;
        Ok(config)
    }

    pub async fn save(&self) -> Result<(), MyErr> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let string = serde_json::to_string_pretty(self)?;
        tokio::fs::write(path, string.as_bytes()).await?;
        Ok(())
    }
}

pub async fn handle_auto_response(
    ctx: &serenity::all::Context,
    msg: &serenity::all::Message,
) -> Result<(), MyErr> {
    if msg.author.bot {
        return Ok(());
    }

    let content = msg.content.trim();
    let cache = AUTO_CACHE.read().await;

    for (trigger, response) in cache.iter() {
        let matches = if response.regex.is_some() {
            if let Ok(regex) = regex::Regex::new(&response.regex.as_ref().unwrap()) {
                regex.is_match(content)
            } else {
                false
            }
        } else if response.case_sensitive {
            content == trigger
        } else {
            content.to_lowercase() == trigger.to_lowercase()
        };

        if matches {
            if !response.roles_required.is_empty() {
                if let Some(member) = msg.member.as_ref() {
                    let has_role = response.roles_required.iter().any(|role_id| member.roles.contains(&RoleId::new(*role_id)));
                    if !has_role {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            if !response.channels_allowed.is_empty() && !response.channels_allowed.contains(&msg.channel_id.get()) {
                continue;
            }

            if let Some(embed_data) = &response.embed {
                let mut embed = serenity::all::CreateEmbed::new();
                if let Some(title) = &embed_data.title {
                    embed = embed.title(parse_escape_sequences(title));
                }
                if let Some(description) = &embed_data.description {
                    embed = embed.description(parse_escape_sequences(description));
                }
                if embed_data.color != 0 {
                    embed = embed.color(embed_data.color);
                }
                if let Some(thumbnail) = &embed_data.thumbnail {
                    embed = embed.thumbnail(thumbnail);
                }
                if let Some(image) = &embed_data.image {
                    embed = embed.image(image);
                }
                if let Some(footer) = &embed_data.footer {
                    embed = embed.footer(serenity::all::CreateEmbedFooter::new(parse_escape_sequences(footer)));
                }
                if let Some(fields) = &embed_data.fields {
                    let create_fields: Vec<_> = fields.iter()
                        .map(|f| (parse_escape_sequences(&f.name), parse_escape_sequences(&f.value), f.inline))
                        .collect();
                    embed = embed.fields(create_fields);
                }
                msg.channel_id.send_message(&ctx.http, serenity::all::CreateMessage::new().add_embed(embed)).await?;
            } else {
                msg.channel_id.say(&ctx.http, parse_escape_sequences(&response.response)).await?;
            }
            info!("Auto-response triggered: {} by user {}", trigger, msg.author.name);
            break;
        }
    }

    drop(cache);
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn add_autoresponse(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }

    let mut trigger = String::new();
    let mut title = String::new();
    let mut description = String::new();
    let mut color_value = 0u32;
    let mut thumbnail_url = String::new();
    let mut image_url = String::new();
    let mut footer = String::new();
    let mut fields_input = String::new();
    let mut roles_input = String::new();
    let mut channels_input = String::new();
    let mut cooldown = 0u64;
    let mut case_sensitive = false;

    for opt in &bnd.cmd.data.options {
        match opt.name.as_str() {
            "trigger" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    trigger = val.clone();
                }
            }
            "title" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    title = val.clone();
                }
            }
            "description" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    description = val.clone();
                }
            }
            "color" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    color_value = match val.as_str() {
                        "white" => 16777215,
                        "red" => 16711680,
                        "blue" => 255,
                        "yellow" => 16776960,
                        "green" => 65280,
                        "purple" => 8388736,
                        "pink" => 16761035,
                        "orange" => 16753920,
                        "light_blue" => 1752220,
                        "gray" => 9807270,
                        _ => 0,
                    };
                }
            }
            "thumbnail" => {
                if let CommandDataOptionValue::Attachment(val) = &opt.value {
                    if let Some(att) = bnd.cmd.data.resolved.attachments.get(val) {
                        thumbnail_url = att.url.clone();
                    }
                }
            }
            "image" => {
                if let CommandDataOptionValue::Attachment(val) = &opt.value {
                    if let Some(att) = bnd.cmd.data.resolved.attachments.get(val) {
                        image_url = att.url.clone();
                    }
                }
            }
            "footer" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    footer = val.clone();
                }
            }
            "fields" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    fields_input = val.clone();
                }
            }
            "roles_required" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    roles_input = val.clone();
                }
            }
            "channels_allowed" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    channels_input = val.clone();
                }
            }
            "cooldown" => {
                if let CommandDataOptionValue::Integer(val) = &opt.value {
                    cooldown = *val as u64;
                }
            }
            "case_sensitive" => {
                if let CommandDataOptionValue::Boolean(val) = &opt.value {
                    case_sensitive = *val;
                }
            }
            _ => {}
        }
    }

    if trigger.is_empty() || description.is_empty() {
        return Components::response(bnd, "❌ **Error:** Trigger and description are required.", true).await;
    }

    let fields = if !fields_input.is_empty() {
        parse_fields_input(&fields_input)?
    } else {
        None
    };

    let embed_config = AutoResponseEmbed {
        title: if title.is_empty() { None } else { Some(title) },
        description: if description.is_empty() { None } else { Some(description) },
        color: color_value,
        thumbnail: if thumbnail_url.is_empty() { None } else { Some(thumbnail_url) },
        image: if image_url.is_empty() { None } else { Some(image_url) },
        footer: if footer.is_empty() { None } else { Some(footer) },
        fields,
    };

    let auto_response = AutoResponse {
        trigger: trigger.clone(),
        response: String::new(),
        embed: Some(embed_config),
        case_sensitive,
        regex: None,
        cooldown,
        roles_required: parse_id_list(&roles_input),
        channels_allowed: parse_id_list(&channels_input),
    };

    let mut config = AutoResponseConfig::load().await?;
    config.responses.push(auto_response.clone());
    config.save().await?;

    let mut cache = AUTO_CACHE.write().await;
    cache.insert(trigger.clone(), auto_response);
    drop(cache);

    Components::response(
        bnd,
        &format!("✅ **Auto-response added!**\nTrigger: `{}`\nCooldown: {} seconds\n\n**Note:** The trigger is active and can be used immediately!", trigger, cooldown),
        true,
    ).await?;

    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn edit_autoresponse(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }

    let mut trigger = String::new();
    let mut new_title = String::new();
    let mut new_desc = String::new();
    let mut new_color = String::new();
    let mut new_footer = String::new();
    let mut new_fields = String::new();
    let mut new_thumbnail = String::new();
    let mut new_image = String::new();
    let mut new_roles = String::new();
    let mut new_channels = String::new();
    let mut new_cooldown: Option<i64> = None;
    let mut new_case_sensitive: Option<bool> = None;

    for opt in &bnd.cmd.data.options {
        match opt.name.as_str() {
            "trigger" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    trigger = val.clone();
                }
            }
            "title" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_title = val.clone();
                }
            }
            "description" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_desc = val.clone();
                }
            }
            "color" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_color = val.clone();
                }
            }
            "footer" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_footer = val.clone();
                }
            }
            "fields" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_fields = val.clone();
                }
            }
            "thumbnail" => {
                if let CommandDataOptionValue::Attachment(val) = &opt.value {
                    if let Some(att) = bnd.cmd.data.resolved.attachments.get(val) {
                        new_thumbnail = att.url.clone();
                    }
                }
            }
            "image" => {
                if let CommandDataOptionValue::Attachment(val) = &opt.value {
                    if let Some(att) = bnd.cmd.data.resolved.attachments.get(val) {
                        new_image = att.url.clone();
                    }
                }
            }
            "roles_required" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_roles = val.clone();
                }
            }
            "channels_allowed" => {
                if let CommandDataOptionValue::String(val) = &opt.value {
                    new_channels = val.clone();
                }
            }
            "cooldown" => {
                if let CommandDataOptionValue::Integer(val) = &opt.value {
                    new_cooldown = Some(*val);
                }
            }
            "case_sensitive" => {
                if let CommandDataOptionValue::Boolean(val) = &opt.value {
                    new_case_sensitive = Some(*val);
                }
            }
            _ => {}
        }
    }

    let mut config = AutoResponseConfig::load().await?;
    
    let mut response_to_edit = config
        .responses
        .iter()
        .find(|r| r.trigger == trigger)
        .ok_or_else(|| MyErr::Custom(format!("❌ Trigger `{}` not found.", trigger)))?
        .clone();

    if let Some(ref mut embed) = response_to_edit.embed {
        if !new_title.is_empty() {
            embed.title = Some(new_title.clone());
        }
        if !new_desc.is_empty() {
            embed.description = Some(new_desc.clone());
        }
        if !new_color.is_empty() {
            embed.color = match new_color.as_str() {
                "white" => 16777215,
                "red" => 16711680,
                "blue" => 255,
                "yellow" => 16776960,
                "green" => 65280,
                "purple" => 8388736,
                "pink" => 16761035,
                "orange" => 16753920,
                "light_blue" => 1752220,
                "gray" => 9807270,
                _ => embed.color,
            };
        }
        if !new_footer.is_empty() {
            embed.footer = Some(new_footer.clone());
        }
        if !new_fields.is_empty() {
            embed.fields = parse_fields_input(&new_fields)?;
        }
        if !new_thumbnail.is_empty() {
            embed.thumbnail = Some(new_thumbnail.clone());
        }
        if !new_image.is_empty() {
            embed.image = Some(new_image.clone());
        }
    }

    if !new_roles.is_empty() {
        response_to_edit.roles_required = parse_id_list(&new_roles);
    }
    if !new_channels.is_empty() {
        response_to_edit.channels_allowed = parse_id_list(&new_channels);
    }
    if let Some(cd) = new_cooldown {
        response_to_edit.cooldown = cd as u64;
    }
    if let Some(cs) = new_case_sensitive {
        response_to_edit.case_sensitive = cs;
    }

    if let Some(index) = config.responses.iter().position(|r| r.trigger == trigger) {
        config.responses[index] = response_to_edit.clone();
    }

    config.save().await?;

    let mut cache = AUTO_CACHE.write().await;
    cache.insert(trigger.clone(), response_to_edit);
    drop(cache);

    Components::response(
        bnd,
        &format!("✅ **Auto-response updated!**\nTrigger: `{}`", trigger),
        true,
    ).await?;

    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn list_autoresponse(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cache = AUTO_CACHE.read().await;
    if cache.is_empty() {
        return Components::response(bnd, "📋 **No auto-responses configured.**", true).await;
    }
    let mut desc = String::new();
    for (i, (trigger, resp)) in cache.iter().enumerate() {
        desc.push_str(&format!("{}. `{}` - CD: {}s\n", i + 1, trigger, resp.cooldown));
    }
    drop(cache);
    let embed = serenity::all::CreateEmbed::new().title("📋 Auto-Response List").description(desc).color(0x00FF00);
    Components::response_adv(bnd, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().add_embed(embed))).await?;
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn remove_autoresponse(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }
    let mut trigger = String::new();
    for opt in &bnd.cmd.data.options {
        if opt.name == "trigger" {
            if let CommandDataOptionValue::String(val) = &opt.value {
                trigger = val.clone();
            }
        }
    }
    if trigger.is_empty() {
        return Components::response(bnd, "❌ **Error:** Trigger is required.", true).await;
    }
    let mut config = AutoResponseConfig::load().await?;
    let len = config.responses.len();
    config.responses.retain(|r| r.trigger != trigger);
    if config.responses.len() == len {
        return Components::response(bnd, &format!("❌ Trigger `{}` not found.", trigger), true).await;
    }
    config.save().await?;
    let mut cache = AUTO_CACHE.write().await;
    cache.remove(&trigger);
    drop(cache);
    Components::response(bnd, &format!("✅ **Removed:** `{}`", trigger), true).await?;
    Ok(())
}