use crate::event::interaction::SlashBundle;
use crate::MyErr;
use crate::commands::admin::execute_guard;
use serenity::all::{CommandDataOption, CommandDataOptionValue, CreateEmbed, CreateInteractionResponseFollowup, ChannelType};

fn parse_message_content(input: &str) -> String {
    input.replace("\\n", "\n")
         .replace("\\t", "\t")
         .replace("\\r", "\r")
}

pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::Custom("Access Denied".into()));
    }

    let scheduler = crate::SCHEDULER.get().ok_or_else(|| MyErr::Custom("Scheduler not initialized".into()))?;

    let sub_opt = bnd.cmd.data.options.first()
        .ok_or_else(|| MyErr::Custom("Missing subcommand".into()))?;

    let sub_name = sub_opt.name.as_str();
    let sub_opts = match &sub_opt.value {
        CommandDataOptionValue::SubCommand(opts) => opts,
        _ => return Err(MyErr::Custom("Invalid subcommand structure".into())),
    };

    match sub_name {
        "add" => add_reminder(bnd, scheduler, sub_opts).await,
        "list" => list_reminders(bnd, scheduler).await,
        "edit" => edit_reminder(bnd, scheduler, sub_opts).await,
        "remove" => remove_reminder(bnd, scheduler, sub_opts).await,
        _ => Err(MyErr::Custom("Unknown subcommand".into())),
    }
}

fn get_str_opt(opts: &[CommandDataOption], name: &str) -> Option<String> {
    opts.iter().find(|o| o.name == name).and_then(|o| {
        if let CommandDataOptionValue::String(s) = &o.value {
            Some(s.clone())
        } else {
            None
        }
    })
}

fn get_int_opt(opts: &[CommandDataOption], name: &str) -> Option<i64> {
    opts.iter().find(|o| o.name == name).and_then(|o| {
        if let CommandDataOptionValue::Integer(v) = &o.value {
            Some(*v)
        } else {
            None
        }
    })
}

fn get_bool_opt(opts: &[CommandDataOption], name: &str) -> Option<bool> {
    opts.iter().find(|o| o.name == name).and_then(|o| {
        if let CommandDataOptionValue::Boolean(v) = &o.value {
            Some(*v)
        } else {
            None
        }
    })
}

async fn resolve_channel(bnd: &SlashBundle<'_>, input: &str) -> Result<u64, MyErr> {
    if let Ok(id) = input.parse::<u64>() {
        return Ok(id);
    }
    
    if input.starts_with("<#") && input.ends_with('>') {
        let inner = &input[2..input.len()-1];
        return Ok(inner.parse::<u64>()
            .map_err(|_| MyErr::Custom(format!("Invalid channel mention format: {}", input)))?);
    }
    
    let channel_name = input.trim_start_matches('#').to_lowercase();
    
    let guild_id = bnd.cmd.guild_id
        .ok_or_else(|| MyErr::Custom("This command can only be used in a server".into()))?;
    
    if let Some(guild) = bnd.ctx.cache.guild(guild_id) {
        for channel in guild.channels.values() {
            if channel.name.to_lowercase() == channel_name {
                if channel.kind == ChannelType::Text {
                    return Ok(channel.id.get());
                }
            }
        }
    }
    
    Err(MyErr::Custom(format!(
        "❌ Channel '{}' not found!\n\n💡 Usage:\n• Raw ID: `1492149802052485371`\n• Mention: `<#1492149802052485371>`\n• Name: `#announcements` or `announcements`",
        input
    )))
}

async fn send_response(bnd: &SlashBundle<'_>, content: &str) -> Result<(), MyErr> {
    let followup = CreateInteractionResponseFollowup::new().content(content).ephemeral(true);
    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
    Ok(())
}

async fn add_reminder(bnd: &SlashBundle<'_>, scheduler: &crate::reusable::component::scheduler::Scheduler, opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer_ephemeral(&bnd.ctx.http).await;

    let r_type = get_str_opt(opts, "type").ok_or_else(|| MyErr::Custom("Type is required".into()))?;
    let channel_input = get_str_opt(opts, "channel").ok_or_else(|| MyErr::Custom("Channel is required".into()))?;
    let message = get_str_opt(opts, "message").ok_or_else(|| MyErr::Custom("Message is required".into()))?;
    let role_str = get_str_opt(opts, "role");
    
    let interval = get_int_opt(opts, "interval_minutes").unwrap_or(60);

    let channel_id = resolve_channel(bnd, &channel_input).await?;

    let role_id: Option<u64> = if let Some(role_input) = role_str {
        if let Ok(id) = role_input.parse::<u64>() {
            Some(id)
        } else if role_input.starts_with("<@&") && role_input.ends_with('>') {
            let inner = &role_input[3..role_input.len()-1];
            inner.parse::<u64>().ok()
        } else {
            if let Some(guild_id) = bnd.cmd.guild_id {
                if let Some(guild) = guild_id.to_guild_cached(&bnd.ctx.cache) {
                    let role_name = role_input.trim_start_matches('@').to_lowercase();
                    guild.roles.iter()
                        .find(|(_, r)| r.name.to_lowercase() == role_name)
                        .map(|(id, _)| id.get())
                } else {
                    None
                }
            } else {
                None
            }
        }
    } else {
        None
    };

    let id = match scheduler.add_reminder(&r_type, role_id, channel_id, &message, Some(interval)).await {
        Ok(id) => id,
        Err(e) => {
            send_response(bnd, &format!("❌ **Database Error:** {}", e)).await?;
            return Ok(());
        }
    };

    send_response(bnd, &format!(
        "✅ **Reminder Added!**\n\
        ID: `{}`\n\
        Type: `{}`\n\
        Channel: `<#{}`>\n\
        Message: {}\n\
        Interval: `{} minutes`", 
        id, r_type, channel_id, message, interval
    )).await?;
    Ok(())
}

async fn list_reminders(bnd: &SlashBundle<'_>, scheduler: &crate::reusable::component::scheduler::Scheduler) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer_ephemeral(&bnd.ctx.http).await;
    
    let reminders = scheduler.list_reminders().await;
    let mut desc = String::new();
    if reminders.is_empty() {
        desc.push_str("No active reminders.");
    } else {
        for r in reminders {
            desc.push_str(&format!(
                "**ID: {}** | Type: `{}` | Channel: `<#{}`> | Interval: {}m\n", 
                r.id, r.reminder_type, r.channel_id, r.interval_minutes.unwrap_or(0)
            ));
        }
    }
    let embed = CreateEmbed::new().title("📋 Scheduled Reminders").description(desc).color(0x00FF00);
    let followup = CreateInteractionResponseFollowup::new().embed(embed).ephemeral(true);
    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
    Ok(())
}

async fn edit_reminder(bnd: &SlashBundle<'_>, scheduler: &crate::reusable::component::scheduler::Scheduler, opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer_ephemeral(&bnd.ctx.http).await;
    
    let id: i32 = get_int_opt(opts, "id").ok_or_else(|| MyErr::Custom("ID is required".into()))? as i32;
    let msg = get_str_opt(opts, "message");
    let interval = get_int_opt(opts, "interval_minutes");
    let active = get_bool_opt(opts, "active");

    let parsed_msg = msg.as_ref().map(|m| parse_message_content(m));

    match scheduler.edit_reminder(id, parsed_msg.as_deref(), interval, active).await {
        Ok(_) => send_response(bnd, &format!("✅ **Reminder {} updated!**", id)).await?,
        Err(e) => send_response(bnd, &format!("❌ **Error:** {}", e)).await?,
    }
    Ok(())
}

async fn remove_reminder(bnd: &SlashBundle<'_>, scheduler: &crate::reusable::component::scheduler::Scheduler, opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer_ephemeral(&bnd.ctx.http).await;
    
    let id: i32 = get_int_opt(opts, "id").ok_or_else(|| MyErr::Custom("ID is required".into()))? as i32;
    
    match scheduler.remove_reminder(id).await {
        Ok(_) => send_response(bnd, &format!("🗑️ **Reminder {} removed!**", id)).await?,
        Err(e) => send_response(bnd, &format!("❌ **Error:** {}", e)).await?,
    }
    Ok(())
}