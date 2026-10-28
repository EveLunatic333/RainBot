use serenity::all::*;
use crate::{SlashBundle, MyErr};
use crate::reusable::component::gacha::Gacha;

#[hertz::hertz_slash_normal(10, true)]
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

    bnd.cmd.defer(&bnd.ctx.http).await.unwrap_or_default();

    match Gacha::new().await {
        Ok(_) => {
            let res = EditInteractionResponse::new().content(
                "✅ **Gacha Configuration Checked!**\nNo issues found. Total rates mathematically equal 1.0 (100%). It is safe for players to pull."
            );
            bnd.cmd.edit_response(&bnd.ctx.http, res).await?;
        }
        Err(e) => {
            let res = EditInteractionResponse::new().content(
                format!("❌ **Gacha Configuration Error Detected!**\n`\n{}\n`\nPlease fix the gacha.json file immediately.", e)
            );
            bnd.cmd.edit_response(&bnd.ctx.http, res).await?;
        }
    }

    Ok(())
}