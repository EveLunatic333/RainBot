use crate::reusable::component::bounty::Title;
use crate::{MyErr, SlashBundle, Reg, Components};
use crate::reusable::component::market::{Market, Trading};
use crate::reusable::component::notices;
use serenity::all::*;
use super::market::{self, Bought};
use super::meal;
use super::bar;

#[hertz::hertz_slash_reg(60, false)]
async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(msg).ephemeral(true))).await;
        return Ok(());
    }

    let mut name = "";
    for data in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(_) = &data.value {
            name = data.name.as_str();
        }
    }

    match name {
        "market" => market::slash(bnd, reg).await?,
        "bar" => bar::slash(bnd).await?,
        "restourant" => meal::slash(bnd, reg).await?,
        "jewelry" => trading(bnd, reg, "Gacha Premium".to_owned()).await?,
        "guild" => trading(bnd, reg, "RP".to_owned()).await?,
        "casino" => trading(bnd, reg, "Ticket".to_owned()).await?,
        _ => return Err(notices::trading_disabled().into()),
    }
    Ok(())
}

#[hertz::hertz_auto]
async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(msg).ephemeral(true))).await;
        return Ok(());
    }

    let mut name = "";
    let mut focus = "";
    for data in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(sub) = &data.value {
            name = data.name.as_str();
            for i in sub {
                if let CommandDataOptionValue::Autocomplete { kind: _, value } = &i.value {
                    focus = value.as_str();
                }
            }
        }
    }

    match name {
        "market" => market::auto(bnd, focus).await?,
        "restourant" => meal::auto(bnd, focus).await?,
        _ => return Err(notices::trading_disabled().into()),
    }
    Ok(())
}

async fn trading(bnd: &SlashBundle<'_>, reg: &Reg<'_>, unit: String) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(msg).ephemeral(true))).await;
        return Ok(());
    }

    let trade = Trading::new().await?;
    let item;
    let name;

    match unit.as_str() {
        "Ticket" => {
            if !trade.casino.enabled { return Err(notices::trading_section_closed("Casino").into()); }
            item = trade.casino.clone();
            name = "Gacha Ticket".to_string();
        }
        "RP" => {
            if !trade.guild.enabled { return Err(notices::trading_section_closed("Guild trade RP").into()); }
            item = trade.guild.clone();
            name = "Guild RP".to_string();
        }
        _ => {
            if !trade.jewelry.enabled { return Err(notices::trading_section_closed("Jewelry").into()); }
            item = trade.jewelry.clone();
            name = "Gacha Premium".to_string();
        }
    }

    let mut bought = 0;
    for data in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::Integer(x) = &data.value {
            if *x < 0 { return Err(notices::trading_bad_quantity().into()); }
            bought = *x;
        }
    }

    let (co, event) = tokio::join!(reg.pg.get_coin(), reg.pg.get_event());
    let (coin, event) = (co?, event?);

    let title = Title::new(event.title as u8).discount();
    let bon = (1.0 - title) * 100.0;
    let disc = format!("{}%", bon as u32);

    let total = bought * item.price as i64;
    let discounted = (total as f32 * title) as i64;
    let change = coin as i64 - discounted;

    if change < 0 {
        return Err(notices::market_insufficient_balance_formatted(coin as i64, discounted, Market::currency).into());
    }

    let receipt = Bought::new(name, bought, total, change, coin, item.price, unit.to_owned(), discounted, disc);

    if receipt.confirmation(bnd).await? {
        match unit.as_str() {
            "Ticket" => reg.pg.buy_ticket(bought as i32).await?,
            "RP" => {
                if !reg.pg.guild_rp(reg.cid, bought as i32).await? {
                    return Err(notices::trading_no_guild(true).into());
                }
            }
            _ => reg.pg.jelewelry(reg.cid, bought as i32).await?,
        }
        reg.pg.bounty_transaction(discounted as i32).await?;
    }

    Ok(())
}