use serenity::all::*;
use crate::{Components, MyErr, Reg, SlashBundle};

#[hertz::hertz_slash_normal(0, false)]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
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

    let target_user = bnd.cmd.data.resolved.users.values().next()
        .ok_or_else(|| MyErr::Custom("❌ **Action Failed:** No target user provided.".to_string()))?;

    let mut reg = Reg::check_user(bnd, target_user).await?;
    reg.pg.purge().await?;
    Components::response(bnd, "user already purged", true).await?;

    if let Some(member) = &bnd.cmd.member {
        Components::remove_role(*member.clone(), bnd, bnd.init.server_role.register_role).await?;
    }

    reg.pg.close().await;
    Ok(())
}