use serenity::all::*;
use crate::{MyErr, SlashBundle, Components, Reg};
use crate::reusable::component::bounty::{BountyRefresh, BBQ};
use crate::reusable::postgress::PgConn;
use crate::reusable::bitwise::BitwiseError;

#[hertz::hertz_slash_normal(0, false)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ Command Disabled: The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let mut subcommand_name = "";
    for sub in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(_) = &sub.value {
            subcommand_name = sub.name.as_str();
        }
    }

    match subcommand_name {
        "refresh" => refresh(bnd).await?,
        "user" => reset(bnd).await?,
        "bounty" => bounty(bnd).await?,
        "interface" => interface(bnd).await?,
        _ => return Err(MyErr::Custom("Cannot resolve subcommand.".to_owned())),
    }
    Ok(())
}

async fn interface(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;
    let emb = CreateEmbed::new()
        .title("⚓ Bounty Cooldown Anchor")
        .description("Initializing cooldown interface from `/static/cooldown.json`...");
    
    let msg = bnd.cmd.channel_id.send_message(&bnd.ctx.http, CreateMessage::new().embed(emb)).await?;
    
    let reply = format!(
        "✅ **Cooldown Anchor Successfully Spawned!**\n\n\
        **Message ID:** `{}`\n\n\
        **Next Steps:**\n\
        1. Copy the Message ID above.\n\
        2. Paste it into `config.json` under `\"cooldown_msg\"`.\n\
        3. Restart the bot.\n\
        4. Run `/cooldown refresh` to load the actual interface and buttons from `cooldown.json`.",
        msg.id.get()
    );
    
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&reply)).await?;
    Ok(())
}

async fn refresh(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let refresh = BountyRefresh::new(false).await?;
    refresh.save(true).await?;
    
    Components::response(bnd, "✅ **Cooldown interface refreshed successfully!**", true).await?;
    refresh.cooldown(bnd).await?;
    Ok(())
}

async fn bounty(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut bbq_value = String::new();
    let mut cooldown_value = 0;
    
    if let Some(main_opt) = bnd.cmd.data.options.first() {
        if let CommandDataOptionValue::SubCommand(sub_opts) = &main_opt.value {
            for opt in sub_opts {
                match &opt.value {
                    CommandDataOptionValue::Integer(x) => cooldown_value = *x,
                    CommandDataOptionValue::String(x) => bbq_value = x.to_owned(),
                    _ => continue,
                }
            }
        }
    }

    let bbq = BBQ::new(bbq_value.parse::<u8>().map_err(|_| MyErr::Custom("Invalid BBQ value".to_string()))?)?;
    
    let mut bounty = Box::new(BountyRefresh::new(true).await?);
    bounty.set_cd(&bbq, cooldown_value as u32);
    bounty.save(true).await?;
    
    Components::response(bnd, &format!("✅ **BBQ{} cooldown set to {}**", bbq.encode(), cooldown_value), true).await?;
    bounty.cooldown(bnd).await?;
    Ok(())
}

async fn reset(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let user = bnd.cmd.data.resolved.users.values().next()
        .ok_or_else(|| MyErr::Custom("No user detected.".to_owned()))?;
    
    let mut reg = Reg::check(bnd, user).await?;
    reg.pg.reset_cooldown().await?;
    
    Components::response(bnd, &format!("{} bounty cooldown reset.", user.to_string()), false).await?;
    reg.pg.close().await;
    Ok(())
}

impl PgConn<'_> {
    pub async fn reset_cooldown(&self) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET latest_bounty_time = 0 WHERE discord_id = $1")
            .bind(&self.did)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}