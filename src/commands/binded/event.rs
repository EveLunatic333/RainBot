
use crate::event::interaction::Mybundle;
use crate::{Reg, SlashBundle, MyErr};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

#[hertz::hertz_slash_reg(10, false)]
async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let event = reg.pg.get_event().await?;
    let user = bnd.user();
    event.response(&user, bnd).await?;
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
async fn userr(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let user = bnd.cmd.data.resolved.users.iter().next()
        .map(|(_, u)| u)
        .ok_or_else(|| MyErr::Custom("No user detected.".to_string()))?;

    let mut reg = Reg::check(bnd, user).await?;
    let event = reg.pg.get_event().await?;
    event.response(user, bnd).await?;
    reg.pg.close().await;
    Ok(())
}