use serenity::all::*;
use crate::{SlashBundle, MyErr};
use crate::reusable::component::{
    market::{Market, Meal, Trading, Tag},
    gacha::Gacha,
    bounty::{BountyRefresh, BountyTitle, Category},
    welcome::WelcomeData,
    Components,
};

#[hertz::hertz_slash_normal(0, true)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !crate::commands::admin::execute_guard(bnd).await {
        return Ok(());
    }

    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
            )
        ).await;
        return Ok(());
    }

    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;

    let mut subcommand_name = "";
    for option in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(_) = &option.value {
            subcommand_name = &option.name;
            break;
        }
    }

    match subcommand_name {
        "gacha" => Components::json_config(bnd, Gacha::default()).await?,
        "market" => Components::json_config(bnd, Market::default()).await?,
        "bounty" => bounty(bnd).await?,
        "bounty_refresh" => Components::json_config(bnd, BountyRefresh::default()).await?,
        "bounty_title" => Components::json_config(bnd, BountyTitle::default()).await?,
        "meal" => Components::json_config(bnd, Meal::default()).await?,
        "trading" => Components::json_config(bnd, Trading::default()).await?,
        "tag" => Components::json_config(bnd, Tag::default()).await?,
        "welcome_msg" => Components::json_config(bnd, WelcomeData::default()).await?,
        _ => return Err(MyErr::Custom("Unrecognized configuration command.".to_owned())),
    }

    Components::edit(bnd, "✅ **System configuration has been successfully updated.**").await?;
    Ok(())
}

async fn bounty(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
            )
        ).await;
        return Ok(());
    }

    let mut category_code = None;
    for option in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::String(value) = &option.value {
            category_code = Some(value);
            break;
        }
    }

    let code_str = category_code.ok_or_else(|| MyErr::Custom("Missing bounty category parameter.".to_owned()))?;
    let code = code_str.parse::<u8>().map_err(|_| MyErr::Custom(format!("Invalid category code: {}", code_str)))?;
    let category = Category::new(code)?;
    Components::bounty_config(bnd, &category).await?;

    Ok(())
}