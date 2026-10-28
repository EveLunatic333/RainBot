use serenity::all::*;
use crate::{MyErr, SlashBundle};
use crate::reusable::postgress::PgConn;

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

    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;

    let mut target_user_id = None;
    let mut new_password = String::new();

    for opt in &bnd.cmd.data.options {
        match opt.name.as_str() {
            "user" => {
                if let CommandDataOptionValue::User(uid) = &opt.value {
                    target_user_id = Some(uid.get());
                }
            }
            "password" => {
                if let CommandDataOptionValue::String(s) = &opt.value {
                    new_password = s.clone();
                }
            }
            _ => {}
        }
    }

    let target_uid = target_user_id.ok_or_else(|| MyErr::Custom("❌ **Action Failed:** Invalid user mention.".to_string()))?;

    let mut pg = PgConn::create(bnd.init, target_uid.to_string()).await.map_err(|_| {
        MyErr::Custom(format!("❌ **Action Failed:** The user <@{}> is not properly connected to the database.", target_uid))
    })?;

    match pg.change_user_password(&new_password, 0).await {
        Ok(_) => {
            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(
                format!("✅ **Success!** Password for <@{}> has been forcefully changed to:\n`{}`", target_uid, new_password)
            )).await?;
        }
        Err(_) => {
            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(
                format!("❌ **Database Error:** User <@{}> has not registered to the server yet.", target_uid)
            )).await?;
        }
    }

    pg.close().await;
    Ok(())
}