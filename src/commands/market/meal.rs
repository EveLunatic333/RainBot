use crate::reusable::component::bounty::Title;
use crate::{MyErr, Reg, Components, SlashBundle};
use crate::reusable::component::market::{Meal, Trading, Market};
use serenity::all::*;
use super::market::Bought;

pub(super) async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let trade = Trading::new().await?;
    if !trade.restourant.enabled {
        return Err(MyErr::Custom("Restaurant is currently disabled.".to_string()));
    }
    let meal = Meal::new().await?;
    let mut duration_hours: i64 = 0;
    let mut food_id: i32 = 0;

    for data in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::Integer(x) = &data.value {
            let value = *x;
            if value <= 0 {
                return Err(MyErr::Custom("You cannot buy 0 or negative quantities.".to_string()));
            }
            match data.name.as_str() {
                "food" => food_id = value as i32,
                _ => duration_hours = value,
            }
        }
    }

    if duration_hours > 1440 {
        return Err(MyErr::Custom("For safety and stability reasons, you can only order food for up to 1440 hours (60 Days) maximum per order!".to_string()));
    }

    let selected_meal = meal.meals.iter().find(|&x| x.id == food_id)
        .ok_or_else(|| MyErr::Custom("Unidentified input. Make sure you properly select food options.".to_string()))?;

    let (co_res, event_res) = tokio::join!(reg.pg.get_coin(), reg.pg.get_event());
    let coin = co_res?;
    let event = event_res?;

    let title_discount = Title::new(event.title as u8).discount();
    let bonus_pct = (1.0 - title_discount) * 100.0;
    let discount_str = format!("{}%", bonus_pct as u32);

    let total_price = duration_hours * (trade.restourant.price as i64);
    let discounted_price = (total_price as f32 * title_discount) as i64;
    let change = coin as i64 - discounted_price;

    if change < 0 {
        return Err(MyErr::Custom(format!(
            "You only have {} Bounty Coins, but you need {} for this transaction.",
            Market::currency(coin as i64), Market::currency(discounted_price)
        )));
    }

    let duration_sec = (duration_hours * 3600) as i32;

    let receipt = Bought::new(
        selected_meal.name.to_string(),
        duration_hours,
        total_price,
        change,
        coin,
        trade.restourant.price,
        "Hour(s)".to_string(),
        discounted_price,
        discount_str
    );

    if receipt.confirmation(bnd).await? {
        if !reg.pg.guild_food(reg.cid, food_id, selected_meal.level, duration_sec).await? {
            return Err(MyErr::Custom("You do not belong to any guild to use this command (Don't worry, your Bounty Coins have been refunded).".to_string()));
        }
        reg.pg.bounty_transaction(discounted_price as i32).await?;
    }

    Ok(())
}

pub(super) async fn auto(bnd: &SlashBundle<'_>, focus: &str) -> Result<(), MyErr> {
    let meal = Meal::new().await?;
    let focus_lower = focus.to_lowercase();
    let mut auto: Vec<_> = meal.meals.iter().filter_map(|x| {
        let name_lower = x.name.to_lowercase();
        if name_lower.starts_with(&focus_lower) || name_lower.contains(&focus_lower) {
            Some(serenity::builder::AutocompleteChoice::new(x.name.clone(), x.id.to_string()))
        } else {
            None
        }
    }).collect();

    auto.truncate(25);

    bnd.cmd.create_response(
        &bnd.ctx.http,
        CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(auto))
    ).await?;

    Ok(())
}