use crate::{MyErr, SlashBundle, Components, ItemPedia};
use serenity::all::*;

#[hertz::hertz_auto]
async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    for i in &bnd.cmd.data.options {
        if let CommandDataOptionValue::Autocomplete { kind: _, value } = &i.value {
            let data = crate::commands::admin::market::SubCommand::Item(value.clone());
            let auto = data.predict(bnd.pedia)?;
            bnd.cmd.create_response(
                &bnd.ctx.http,
                CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(auto)),
            ).await?;
        }
    }
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    for i in &bnd.cmd.data.options {
        if let CommandDataOptionValue::String(x) = &i.value {
            let chars: Vec<char> = x.chars().collect();
            let reversed = if chars.len() >= 4 {
                format!("{}{}{}{}", chars[2], chars[3], chars[0], chars[1])
            } else {
                x.clone()
            };

            let url = format!("https://xl3lackout.github.io/MHFZ-Ferias-English-Project/sozai/sozai.htm?{}", reversed);
            let item_name = ItemPedia::search(7, x).unwrap_or_else(|| "Unknown Item");

            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().components(vec![CreateActionRow::Buttons(
                    vec![CreateButton::new_link(&url).label(item_name)],
                )]),
            );
            Components::response_adv(bnd, response).await?;
        }
    }
    Ok(())
}