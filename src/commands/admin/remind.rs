use crate::event::interaction::SlashBundle;
use crate::MyErr;
use crate::commands::admin::execute_guard;
use crate::reusable::component::notices;
use serenity::all::{CommandDataOption, CommandDataOptionValue, CreateEmbed, CreateInteractionResponseFollowup, ChannelType};

/// The actions `/remind` accepts, listed whenever the user picks none.
const REMIND_ACTIONS: &[&str] = &["add", "list", "edit", "remove"];

fn parse_message_content(input: &str) -> String {
    input.replace("\\n", "\n")
         .replace("\\t", "\t")
         .replace("\\r", "\r")
}

pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !execute_guard(bnd).await {
        return Err(MyErr::from(notices::admin_only("/remind")));
    }

    let scheduler = crate::SCHEDULER.get().ok_or_else(|| MyErr::from(notices::scheduler_unavailable()))?;

    let sub_opt = bnd.cmd.data.options.first()
        .ok_or_else(|| MyErr::from(notices::remind_no_subcommand(REMIND_ACTIONS)))?;

    let sub_name = sub_opt.name.as_str();
    let sub_opts = match &sub_opt.value {
        CommandDataOptionValue::SubCommand(opts) => opts,
        _ => return Err(MyErr::from(notices::remind_bad_subcommand(sub_name))),
    };

    match sub_name {
        "add" => add_reminder(bnd, scheduler, sub_opts).await,
        "list" => list_reminders(bnd, scheduler).await,
        "edit" => edit_reminder(bnd, scheduler, sub_opts).await,
        "remove" => remove_reminder(bnd, scheduler, sub_opts).await,
        _ => Err(MyErr::from(notices::remind_unknown_subcommand(sub_name, REMIND_ACTIONS))),
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
            .map_err(|_| MyErr::from(notices::remind_bad_channel_mention(input)))?);
    }
    
    let channel_name = input.trim_start_matches('#').to_lowercase();
    
    let guild_id = bnd.cmd.guild_id
        .ok_or_else(|| MyErr::from(notices::remind_needs_guild()))?;
    
    if let Some(guild) = bnd.ctx.cache.guild(guild_id) {
        for channel in guild.channels.values() {
            if channel.name.to_lowercase() == channel_name {
                if channel.kind == ChannelType::Text {
                    return Ok(channel.id.get());
                }
            }
        }
    }
    
    Err(MyErr::from(notices::remind_channel_not_found(input, "no text channel in this server has that name")))
}

async fn send_response(bnd: &SlashBundle<'_>, content: &str) -> Result<(), MyErr> {
    let followup = CreateInteractionResponseFollowup::new().content(content).ephemeral(true);
    bnd.cmd.create_followup(&bnd.ctx.http, followup).await?;
    Ok(())
}

async fn add_reminder(bnd: &SlashBundle<'_>, scheduler: &crate::reusable::component::scheduler::Scheduler, opts: &[CommandDataOption]) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer_ephemeral(&bnd.ctx.http).await;

    let r_type = get_str_opt(opts, "type").ok_or_else(|| MyErr::from(notices::remind_field_missing("type")))?;
    let channel_input = get_str_opt(opts, "channel").ok_or_else(|| MyErr::from(notices::remind_field_missing("channel")))?;
    let message = get_str_opt(opts, "message").ok_or_else(|| MyErr::from(notices::remind_field_missing("message")))?;
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
    
    let id: i32 = get_int_opt(opts, "id").ok_or_else(|| MyErr::from(notices::remind_id_missing()))? as i32;
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
    
    let id: i32 = get_int_opt(opts, "id").ok_or_else(|| MyErr::from(notices::remind_id_missing()))? as i32;
    
    match scheduler.remove_reminder(id).await {
        Ok(_) => send_response(bnd, &format!("🗑️ **Reminder {} removed!**", id)).await?,
        Err(e) => send_response(bnd, &format!("❌ **Error:** {}", e)).await?,
    }
    Ok(())
}