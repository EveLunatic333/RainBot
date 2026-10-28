use serenity::all::*;
use crate::{MyErr, SlashBundle};
use crate::reusable::component::Components;
use crate::reusable::postgress::PgConn;

#[hertz::hertz_slash_normal(0, false)]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut is_distributor = false;
    if let Some(member) = &bnd.cmd.member {
        if member.roles.contains(&RoleId::new(bnd.init.server_role.weekly_distributor)) {
            is_distributor = true;
        }
    }

    let mut bounty = false;
    let mut all = false;
    let mut amount = 0;
    let mut user = vec![bnd.cmd.user.id];

    for option in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommandGroup(groups) = &option.value {
            if option.name == "bounty" {
                bounty = true;
            }
            for group in groups {
                if let CommandDataOptionValue::SubCommand(subs) = &group.value {
                    if group.name == "all" {
                        all = true;
                    }
                    for sub in subs {
                        match &sub.value {
                            CommandDataOptionValue::Integer(val) => amount = *val,
                            CommandDataOptionValue::String(val) => user = Components::get_mentions(val),
                            _ => continue,
                        }
                    }
                }
            }
        }
    }

    if is_distributor && user.contains(&bnd.cmd.user.id) {
        return Err(MyErr::Custom(
            "❌ SECURITY CLEARANCE EXCEPTION\nRestricted Operation: Self-targeting detected.\nYour current clearance level (Weekly Distributor) does not allow you to modify your own bounty or ticket balance.\nAdministrative Policy:\nTo maintain a fair economy, resource adjustments must be verified by another authorized distributor or administrator.\nSelf-assignment of server assets is automatically blocked by the system core.\nPlease remove yourself from the target list and try again.".to_string()
        ));
    }

    if !is_distributor && !crate::commands::admin::execute_guard(bnd).await {
        return Ok(());
    }

    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, serenity::builder::CreateInteractionResponse::Message(
            serenity::builder::CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let mut pg = PgConn::create(bnd.init, String::new()).await?;
    for uid in user {
        pg.did = uid.get().to_string();
        if bounty {
            if all {
                pg.bounty_all(amount as i32).await?;
            } else {
                pg.bounty_transaction(-1 * amount as i32).await?;
            }
        } else if all {
            pg.ticket_all(amount as i32).await?;
        } else {
            pg.buy_ticket(amount as i32).await?;
        }
    }
    pg.close().await;

    Components::response(bnd, "Succeeded", true).await?;
    Ok(())
}