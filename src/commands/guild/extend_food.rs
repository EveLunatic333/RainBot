use crate::{MyErr, SlashBundle, Components, PgConn, Mybundle};
use crate::event::interaction::ComponentBundle;
use crate::reusable::component::market::Meal;
use crate::reusable::component::registered::Reg;
use crate::reusable::component::bounty::Title;
use serenity::all::*;
use serenity::model::Timestamp;
use tracing::{error, info, warn};

#[hertz::hertz_slash_reg(20, false)]
pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let mut food_id: Option<i32> = None;
    let mut days: i64 = 0;
    let mut target_guild: Option<i32> = None;

    for opt in &bnd.cmd.data.options {
        if opt.name == "extend_food" {
            if let CommandDataOptionValue::SubCommand(sub_opts) = &opt.value {
                for sub_opt in sub_opts {
                    match &sub_opt.value {
                        CommandDataOptionValue::Integer(val) => {
                            if sub_opt.name == "food" { food_id = Some(*val as i32); }
                            else if sub_opt.name == "days" { days = *val; }
                            else if sub_opt.name == "target_guild" { target_guild = Some(*val as i32); }
                        }
                        CommandDataOptionValue::Number(val) => {
                            if sub_opt.name == "food" { food_id = Some(*val as i32); }
                            else if sub_opt.name == "days" { days = *val as i64; }
                            else if sub_opt.name == "target_guild" { target_guild = Some(*val as i32); }
                        }
                        CommandDataOptionValue::String(val) => {
                            if sub_opt.name == "food" { if let Ok(id) = val.parse::<i32>() { food_id = Some(id); } }
                            else if sub_opt.name == "days" { if let Ok(d) = val.parse::<i64>() { days = d; } }
                            else if sub_opt.name == "target_guild" { if let Ok(g) = val.parse::<i32>() { target_guild = Some(g); } }
                        }
                        _ => {}
                    }
                }
                break;
            }
        }
    }

    let guild_id = match target_guild {
        Some(g) if g > 0 => g,
        _ => reg.pg.guild_id(reg.cid).await? as i32,
    };

    if guild_id == 0 {
        Components::response(bnd, "❌ **System Alert:** You are not currently a member of any guild, and no valid target guild was specified.", true).await?;
        return Ok(());
    }

    let food_id = match food_id {
        Some(id) if id > 0 => id,
        _ => {
            Components::response(bnd, "❌ **Invalid Food ID:** Please select from autocomplete.", true).await?;
            return Ok(());
        }
    };

    if days != 30 && days != 60 {
        Components::response(bnd, "⚠️ **Invalid Duration:** You can only extend for exactly 30 or 60 days.", true).await?;
        return Ok(());
    }

    let meal = Meal::new().await?;
    let meal_info = meal.meals.iter().find(|m| m.id == food_id);
    let meal_name = match meal_info {
        Some(m) => m.name.as_str(),
        None => {
            Components::response(bnd, "❌ **Food Not Found:** The selected food does not exist.", true).await?;
            return Ok(());
        }
    };

    let total_cost = (days / 30) * 300_000;
    let hours_to_add = days * 24;

    let event = reg.pg.get_event().await?;
    let title_discount = Title::new(event.title as u8).discount();
    let bonus_pct = (1.0 - title_discount) * 100.0;
    let discounted_price = (total_cost as f32 * title_discount) as i64;

    if (event.bounty as i64) < discounted_price {
        Components::response(bnd,
            &format!("❌ **Insufficient Funds:**\nRequired: {} BC\nYour Balance: {} BC",
                crate::reusable::component::market::Market::currency(discounted_price),
                crate::reusable::component::market::Market::currency(event.bounty as i64)
            ),
            true
        ).await?;
        return Ok(());
    }

    let discount_str = if bonus_pct > 0.0 {
        format!("Discount {}% for {} = {}", 
            bonus_pct as u32,
            crate::reusable::component::market::Market::currency(total_cost),
            crate::reusable::component::market::Market::currency(discounted_price)
        )
    } else {
        "No discount applied".to_string()
    };

    let confirm_embed = CreateEmbed::new()
        .title("🍽️ Guild Food Extension Receipt")
        .color(0x9b59b6)
        .field("User", format!("<@{}>", bnd.user().id.get()), false)
        .field("Guild ID", guild_id.to_string(), true)
        .field("Food", meal_name.to_string(), true)
        .field("Duration", format!("{} Days ({} Hours)", days, hours_to_add), true)
        .field("Price", format!("Per Day(s): {} x {} = {}", 
            crate::reusable::component::market::Market::currency(300_000),
            days / 30,
            crate::reusable::component::market::Market::currency(total_cost)
        ), false)
        .field("Discounted", discount_str, false)
        .field("Currency", format!("Your Coin = {} - {} = {}",
            crate::reusable::component::market::Market::currency(event.bounty as i64),
            crate::reusable::component::market::Market::currency(discounted_price),
            crate::reusable::component::market::Market::currency((event.bounty as i64) - discounted_price)
        ), false)
        .timestamp(Timestamp::now());

    let confirm_button = CreateButton::new(
        format!("extend_food_confirm:{}:{}:{}", guild_id, food_id, days)
    )
    .label("Confirm")
    .emoji(ReactionType::Unicode("👌".to_string()))
    .style(ButtonStyle::Primary);

    let cancel_button = CreateButton::new(
        format!("extend_food_cancel:{}", guild_id)
    )
    .label("Cancel")
    .emoji(ReactionType::Unicode("👎".to_string()))
    .style(ButtonStyle::Danger);

    let action_row = CreateActionRow::Buttons(vec![confirm_button, cancel_button]);

    let response = CreateInteractionResponseMessage::new()
        .embed(confirm_embed)
        .components(vec![action_row])
        .ephemeral(true);

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await?;
    Ok(())
}

pub async fn handle_confirm(bnd: &ComponentBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let custom_id = bnd.cmd.data.custom_id.as_str();
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 4 {
        return Err(MyErr::Custom("Invalid confirm button format".into()));
    }
    let guild_id: i32 = parts[1].parse().map_err(|_| MyErr::Custom("Invalid guild ID".into()))?;
    let food_id: i32 = parts[2].parse().map_err(|_| MyErr::Custom("Invalid food ID".into()))?;
    let days: i64 = parts[3].parse().map_err(|_| MyErr::Custom("Invalid days".into()))?;

    let meal = Meal::new().await?;
    let meal_info = meal.meals.iter().find(|m| m.id == food_id);
    let meal_name = match meal_info {
        Some(m) => m.name.as_str(),
        None => return Err(MyErr::Custom("Food not found".into())),
    };

    let total_cost = (days / 30) * 300_000;
    let hours_to_add = days * 24;

    let event = reg.pg.get_event().await?;
    let title_discount = Title::new(event.title as u8).discount();
    let discounted_price = (total_cost as f32 * title_discount) as i64;

    if (event.bounty as i64) < discounted_price {
        Components::response(bnd, "❌ **Insufficient Funds!** Transaction cancelled.", true).await?;
        return Ok(());
    }

    let meal_level = meal_info.map(|m| m.level).unwrap_or(1);

    let upsert_query = r#"
        INSERT INTO guild_meals (guild_id, meal_id, level, expires_in)
        VALUES ($1, $2, $3, CURRENT_TIMESTAMP + ($4::int * INTERVAL '1 hour'))
        ON CONFLICT (guild_id, meal_id, level) 
        WHERE expires_in > CURRENT_TIMESTAMP
        DO UPDATE SET expires_in = guild_meals.expires_in + ($4::int * INTERVAL '1 hour')
    "#;

    if let Err(e) = sqlx::query(upsert_query)
        .bind(guild_id)
        .bind(food_id)
        .bind(meal_level)
        .bind(hours_to_add as i32)
        .execute(&reg.pg.pool)
        .await
    {
        error!("Database upsert error: {}", e);

        warn!("UPSERT failed, falling back to SELECT + UPDATE/INSERT");
        
        let row = sqlx::query("SELECT expires_in FROM guild_meals WHERE guild_id = $1 AND meal_id = $2 AND level = $3 AND expires_in > NOW()")
            .bind(guild_id)
            .bind(food_id)
            .bind(meal_level)
            .fetch_optional(&reg.pg.pool)
            .await?;

        if let Some(_) = row {
            sqlx::query("UPDATE guild_meals SET expires_in = expires_in + ($4::int * INTERVAL '1 hour') WHERE guild_id = $1 AND meal_id = $2 AND level = $3")
                .bind(guild_id)
                .bind(food_id)
                .bind(meal_level)
                .bind(hours_to_add as i32)
                .execute(&reg.pg.pool)
                .await?;
        } else {
            sqlx::query("INSERT INTO guild_meals (guild_id, meal_id, level, expires_in) VALUES ($1, $2, $3, CURRENT_TIMESTAMP + ($4::int * INTERVAL '1 hour'))")
                .bind(guild_id)
                .bind(food_id)
                .bind(meal_level)
                .bind(hours_to_add as i32)
                .execute(&reg.pg.pool)
                .await?;
        }
    }

    if let Err(e) = reg.pg.bounty_transaction(discounted_price as i32).await {
        error!("Payment error: {}", e);
        Components::response(bnd, &format!("❌ **Payment Failed:** {}", e), true).await?;
        return Ok(());
    }

    let success_embed = CreateEmbed::new()
        .title("✅ Guild Food Extended Successfully!")
        .color(0x2ecc71)
        .field("Guild ID", guild_id.to_string(), true)
        .field("Food", meal_name.to_string(), true)
        .field("Duration", format!("{} Days", days), true)
        .field("Cost", crate::reusable::component::market::Market::currency(discounted_price), false)
        .timestamp(Timestamp::now());

    let response = CreateInteractionResponseMessage::new()
        .embed(success_embed)
        .components(vec![])
        .ephemeral(true);

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::UpdateMessage(response)).await?;
    Ok(())
}

pub async fn handle_cancel(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let custom_id = bnd.cmd.data.custom_id.as_str();
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 2 {
        return Err(MyErr::Custom("Invalid cancel button format".into()));
    }
    let guild_id = parts[1];

    let cancel_embed = CreateEmbed::new()
        .title("❌ Transaction Cancelled")
        .color(0xe74c3c)
        .description(format!("Guild food extension for Guild ID `{}` has been cancelled.", guild_id))
        .timestamp(Timestamp::now());

    let response = CreateInteractionResponseMessage::new()
        .embed(cancel_embed)
        .components(vec![])
        .ephemeral(true);

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::UpdateMessage(response)).await?;
    Ok(())
}

#[hertz::hertz_auto]
pub async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let sub_opts = Components::sub_options(bnd)?;
    let mut target_guild = 0;

    for opt in sub_opts {
        if opt.name == "target_guild" {
            match &opt.value {
                CommandDataOptionValue::Integer(val) => target_guild = *val as i32,
                CommandDataOptionValue::Number(val) => target_guild = *val as i32,
                CommandDataOptionValue::String(val) => {
                    if let Ok(v) = val.parse::<i32>() {
                        target_guild = v;
                    }
                }
                _ => {}
            }
        }
    }

    for opt in sub_opts {
        if let CommandDataOptionValue::Autocomplete { kind: _, value } = &opt.value {
            
            if opt.name == "target_guild" {
                let focus_lower = value.to_lowercase();
                let mut choices = Vec::new();
                
                if let Ok(pg) = PgConn::create(bnd.init, bnd.user().id.to_string()).await {
                    let search_term = format!("%{}%", focus_lower);
                    let query_result = sqlx::query("SELECT id, name FROM guilds WHERE LOWER(name) LIKE $1 OR CAST(id AS TEXT) LIKE $1 LIMIT 25")
                        .bind(search_term)
                        .fetch_all(&pg.pool)
                        .await;
                    
                    if let Ok(rows) = query_result {
                        use sqlx::Row;
                        for row in rows {
                            if let (Ok(g_id), Ok(g_name)) = (row.try_get::<i32, _>("id"), row.try_get::<String, _>("name")) {
                                let display_name = format!("{} (ID: {})", g_name, g_id);
                                choices.push(AutocompleteChoice::new(display_name, g_id as i64));
                            }
                        }
                    }
                }

                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Autocomplete(
                    CreateAutocompleteResponse::new().set_choices(choices)
                )).await;
            } 
            else if opt.name == "food" {
                let meal = Meal::new().await?;
                let focus_lower = value.to_lowercase();
                let mut active_food_ids: Vec<i32> = Vec::new();
                
                if let Ok(pg) = PgConn::create(bnd.init, bnd.user().id.to_string()).await {
                    
                    if target_guild == 0 {
                        if let Ok(event) = pg.get_event().await {
                            target_guild = pg.guild_id(event.char_id).await.unwrap_or(0) as i32;
                        }
                    }

                    if target_guild > 0 {
                        let query_result = sqlx::query(
                            "SELECT DISTINCT meal_id FROM guild_meals WHERE guild_id = $1 AND expires_in > NOW()"
                        )
                            .bind(target_guild)
                            .fetch_all(&pg.pool)
                            .await;
                        
                        match query_result {
                            Ok(rows) => {
                                use sqlx::Row;
                                for row in rows {
                                    if let Ok(m_id) = row.try_get::<i32, _>("meal_id") {
                                        active_food_ids.push(m_id);
                                    }
                                }
                                
                                info!(
                                    "Autocomplete: Guild {} has {} active foods: {:?}",
                                    target_guild,
                                    active_food_ids.len(),
                                    active_food_ids
                                );
                            }
                            Err(e) => {
                                error!("Failed to query active foods for guild {}: {}", target_guild, e);

                                warn!("Falling back to show all foods due to query error");
                                active_food_ids = meal.meals.iter().map(|m| m.id).collect();
                            }
                        }
                    }
                }

                let mut matched_foods: Vec<_> = meal.meals.iter()
                    .filter(|m| active_food_ids.contains(&m.id))
                    .collect();

                matched_foods.sort_by(|a, b| {
                    let a_match = if focus_lower.is_empty() {
                        false
                    } else {
                        a.name.to_lowercase().contains(&focus_lower)
                    };
                    let b_match = if focus_lower.is_empty() {
                        false
                    } else {
                        b.name.to_lowercase().contains(&focus_lower)
                    };
                    
                    match (a_match, b_match) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    }
                });

                let choices: Vec<AutocompleteChoice> = matched_foods
                    .into_iter()
                    .filter(|m| {
                        if focus_lower.is_empty() {
                            return true;
                        }
                        let name_lower = m.name.to_lowercase();
                        name_lower.contains(&focus_lower)
                    })
                    .take(25)
                    .map(|m| AutocompleteChoice::new(m.name.clone(), m.id as i64))
                    .collect();

                info!(
                    "Autocomplete result for '{}': {} choices displayed",
                    focus_lower,
                    choices.len()
                );
                
                if choices.is_empty() && !active_food_ids.is_empty() {
                    warn!(
                        "No choices displayed despite {} active foods. Focus '{}' might not match any food name.",
                        active_food_ids.len(),
                        focus_lower
                    );
                }

                let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Autocomplete(
                    CreateAutocompleteResponse::new().set_choices(choices)
                )).await;
            }
        }
    }
    Ok(())
}