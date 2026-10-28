use crate::{MyErr, SlashBundle, Reg};
use serenity::all::*;
use sqlx::Row;
use tracing::error;
use chrono::{Utc, DateTime, Duration};

fn decode_weapon_feature(feature_id: i32) -> String {
    let mut active_weapons = Vec::new();
    if feature_id & 1 != 0 { active_weapons.push("<:1GS:1061171660897648670> Great Sword"); }
    if feature_id & 2 != 0 { active_weapons.push("<:1HBG:1061171657219264572> Heavy Bowgun"); }
    if feature_id & 4 != 0 { active_weapons.push("<:1Hammer:1061172365289066586> Hammer"); }
    if feature_id & 8 != 0 { active_weapons.push("<:1Lance:1061172361522589757> Lance"); }
    if feature_id & 16 != 0 { active_weapons.push("<:1SnS:1061172356233576469> Sword & Shield"); }
    if feature_id & 32 != 0 { active_weapons.push("<:1LBG:1061172362948649041> Light Bowgun"); }
    if feature_id & 64 != 0 { active_weapons.push("<:1DS:1061171938476691566> Dual Swords"); }
    if feature_id & 128 != 0 { active_weapons.push("<:1LS:1061172359760982115> Long Sword"); }
    if feature_id & 256 != 0 { active_weapons.push("<:1HH:1061172364148232222> Hunting Horn"); }
    if feature_id & 512 != 0 { active_weapons.push("<:1GL:1061171659647754260> Gunlance"); }
    if feature_id & 1024 != 0 { active_weapons.push("<:1Bow:1061172355482779668> Bow"); }
    if feature_id & 2048 != 0 { active_weapons.push("<:1Tonfa:1061169432430383194> Tonfa"); }
    if feature_id & 4096 != 0 { active_weapons.push("<:1Swaxe:1061172358351687730> Switch Axe F"); }
    if feature_id & 8192 != 0 { active_weapons.push("<:1MS:1061169434259116116> Magnet Spike"); }

    if active_weapons.is_empty() {
        return "None".to_string();
    }
    
    active_weapons.join("\n> ")
}

#[hertz::hertz_slash_reg(20, false)]
pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let current_feature = sqlx::query(
        "SELECT start_time, featured FROM feature_weapon WHERE start_time <= CURRENT_TIMESTAMP - INTERVAL '1 day' ORDER BY start_time DESC LIMIT 1"
    )
    .fetch_optional(&reg.pg.pool)
    .await;

    let next_feature = sqlx::query(
        "SELECT start_time, featured FROM feature_weapon WHERE start_time > CURRENT_TIMESTAMP - INTERVAL '1 day' ORDER BY start_time ASC LIMIT 1"
    )
    .fetch_optional(&reg.pg.pool)
    .await;

    let mut embed = CreateEmbed::new()
        .title("⚔️ Daily Active Weapon Feature ⚔️")
        .color(0x2ecc71);

    match current_feature {
        Ok(Some(row)) => {
            if let (Ok(start_time), Ok(featured_id)) = (
                row.try_get::<DateTime<Utc>, _>("start_time"),
                row.try_get::<i32, _>("featured")
            ) {
                let weapon_list = decode_weapon_feature(featured_id);
                let display_time = start_time + Duration::days(1);
                let actual_display_time = display_time - Duration::hours(12);
                
                embed = embed
                    .field("Active Weapons:", format!("> {}", weapon_list), false)
                    .field("Rotation Started:", format!("<t:{}:F> (<t:{}:R>)", actual_display_time.timestamp(), actual_display_time.timestamp()), false);
            }
        }
        Ok(None) => {
            embed = embed.field("Status", "❌ **No active weapon feature found for today.**", false);
        }
        Err(e) => {
            error!("Database error fetching current feature: {}", e);
            embed = embed.field("Status", "❌ **System Error:** Failed to fetch current weapon feature.", false);
        }
    }

    match next_feature {
        Ok(Some(row)) => {
            if let (Ok(start_time), Ok(featured_id)) = (
                row.try_get::<DateTime<Utc>, _>("start_time"),
                row.try_get::<i32, _>("featured")
            ) {
                let weapon_list = decode_weapon_feature(featured_id);
                let display_time = start_time + Duration::days(1);
                let actual_display_time = display_time - Duration::hours(12);
                
                embed = embed
                    .field("Next Rotation:", format!("> {}", weapon_list), false)
                    .field("Starting:", format!("<t:{}:F> (<t:{}:R>)", actual_display_time.timestamp(), actual_display_time.timestamp()), false);
            }
        }
        Ok(None) => {
            embed = embed.field("Next Rotation", "No upcoming rotation found.", false);
        }
        Err(e) => {
            error!("Database error fetching next feature: {}", e);
            embed = embed.field("Next Rotation", "❌ **System Error:** Failed to fetch next weapon feature.", false);
        }
    }

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(false)
    )).await?;

    Ok(())
}