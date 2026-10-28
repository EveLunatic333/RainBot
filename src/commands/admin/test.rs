use crate::reusable::component::bounty::BountyTitle;
use crate::reusable::component::Components;
use crate::{MyErr, SlashBundle};
use serenity::all::*;
use std::collections::HashMap;

#[hertz::hertz_auto]
pub async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut focus = String::new();
    for i in Components::sub_options(bnd)? {
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
        let raw_name = t.image.url.replace(".webp", " ").replace(".jpg", " ").replace(".png", " ").replace("_", "  ");
        trigger_groups.entry(t.trigger.clone()).or_default().push(raw_name);
    }

    let mut temp_options: Vec<(String, String)> = Vec::new();
    for (trigger_key, names) in trigger_groups {
        let combined_names = names.join("  &  ");
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

#[hertz::hertz_slash_normal(0, true)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !crate::commands::admin::execute_guard(bnd).await {
        return Ok(());
    }
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let mut subcommand_name = "";
    for i in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(_) = &i.value {
            subcommand_name = i.name.as_str();
            break;
        }
    }

    match subcommand_name {
        "test" => test(bnd).await?,
        _ => gift(bnd).await?,
    }
    Ok(())
}

async fn test(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    bnd.cmd.defer(&bnd.ctx.http).await.unwrap_or_default();
    let mut trigger = "";
    for i in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::String(x) = &i.value {
            trigger = x.as_str();
            break;
        }
    }

    let title = BountyTitle::new().await?;
    let mut all_titles = vec![
        &title.bronze_bounty, &title.silver_bounty, &title.gold_bounty,
        &title.bronze_trading, &title.silver_trading, &title.gold_trading,
    ];
    all_titles.extend(&title.custom);

    let matching_titles: Vec<_> = all_titles.into_iter().filter(|t| t.trigger == trigger).collect();
    if matching_titles.is_empty() {
        return Err(MyErr::Custom("Cannot get option value, make sure you properly select an option from the list.".to_owned()));
    }

    let avatar_url = bnd.cmd.user.avatar_url()
        .unwrap_or_else(|| bnd.cmd.user.default_avatar_url())
        .replace(".webp", ".png").replace(".gif", ".png");

    let mut resp = EditInteractionResponse::new();
	for (index, t) in matching_titles.iter().enumerate() {
    let ext = if t.image.animated { "gif" } else { "jpg" };
    let filename = format!("title_{}.{}", index, ext);
    let bytes = t.image.title(&avatar_url, true).await?;
    resp = resp.new_attachment(CreateAttachment::bytes(bytes, filename));
}

    bnd.cmd.edit_response(&bnd.ctx.http, resp).await?;
    Ok(())
}

async fn gift(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut trigger = "";
    let user = bnd.cmd.data.resolved.users.values().next()
        .ok_or_else(|| MyErr::Custom("Target user not resolved.".to_string()))?;

    for i in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::String(x) = &i.value {
            trigger = x.as_str();
            break;
        }
    }

    let guild_id = bnd.cmd.guild_id
        .ok_or_else(|| MyErr::Custom("Command must be used in a server.".to_string()))?;
    let mut mem = guild_id.member(&bnd.ctx.http, &user.id).await?;
    let mut pg = crate::PgConn::create(bnd.init, user.id.to_string()).await?;
    let mut event = pg.get_event().await?;
    let title = BountyTitle::new().await?;

    title.add_title(bnd, &mut event, &mut mem, trigger).await?;
    Components::response(bnd, "✅ Successfully granted the title(s)!", true).await?;
    pg.bounty_event(&event).await?;
    pg.close().await;
    Ok(())
}