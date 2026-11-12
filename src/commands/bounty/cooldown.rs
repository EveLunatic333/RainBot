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
        4. Run `/cooldown refresh` to fill this message with the BBQ cooldown list from `cooldown.json`.",
        msg.id.get()
    );
    
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&reply)).await?;
    Ok(())
}

async fn refresh(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let refresh = BountyRefresh::new(false).await?;
    refresh.save(true).await?;

    // Answer the interaction before touching the board: Discord gives three
    // seconds, and the board edit below is a separate HTTP round trip that can
    // outlast it.
    //
    // Point the operator at the board being refreshed. The message id comes from
    // config, because the board is a fixed message edited in place.
    let board_url = board_link(bnd);
    let content = match &board_url {
        Some(url) => format!(
            "✅ **Cooldown interface refreshed successfully!**\n\n\
             🔗 **Board:** [Jump to the cooldown board]({})\n\
             The board was updated in place; this link opens it directly.",
            url
        ),
        None => "✅ **Cooldown interface refreshed successfully!**".to_owned(),
    };

    let mut message = CreateInteractionResponseMessage::new()
        .content(content)
        .ephemeral(true);
    if let Some(url) = &board_url {
        message = message.components(vec![CreateActionRow::Buttons(vec![
            CreateButton::new_link(url)
                .label("Open Cooldown Board")
                .emoji('🔗'),
        ])]);
    }
    Components::response_adv(bnd, CreateInteractionResponse::Message(message)).await?;

    refresh.cooldown(bnd).await?;
    Ok(())
}

/// The jump link Discord expects for one message.
///
/// `None` when no board has been spawned yet: `cooldown_msg: 0` would produce a
/// `/channels/<g>/<c>/0` link that opens nothing.
fn board_url_for(guild: u64, channel: u64, message: u64) -> Option<String> {
    if message == 0 {
        return None;
    }
    Some(format!(
        "https://discord.com/channels/{}/{}/{}",
        guild, channel, message
    ))
}

/// A clickable link to the configured cooldown board message, if one can be
/// built from what this interaction knows.
fn board_link(bnd: &SlashBundle<'_>) -> Option<String> {
    board_url_for(
        bnd.cmd.guild_id?.get(),
        bnd.init.bounty.cooldown_ch,
        bnd.init.bounty.cooldown_msg,
    )
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

        // Cooldown is tracked per group now, so clearing the legacy column alone
        // would leave every group row in place and the reset would look ignored.
        let mut conn = self.pool.acquire().await?;
        crate::reusable::postgress::gacha::clear_cooldown_history(&mut conn, &self.did).await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_link_has_the_jump_url_shape() {
        let url = board_url_for(1565626318626889779, 1565638194786074744, 1570450649110417410)
            .expect("a configured board must produce a link");
        assert_eq!(
            url,
            "https://discord.com/channels/1565626318626889779/1565638194786074744/1570450649110417410"
        );
        assert_eq!(url.split('/').count(), 7, "host + channels + guild/channel/message");
    }

    #[test]
    fn an_unconfigured_board_id_produces_no_link() {
        assert!(board_url_for(1, 2, 0).is_none());
    }

    #[test]
    fn a_configured_board_id_produces_a_link() {
        assert!(board_url_for(1, 2, 3).is_some());
    }
}