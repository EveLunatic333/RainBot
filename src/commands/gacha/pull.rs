use serenity::all::*;
use serenity::builder::{CreateAttachment, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, EditInteractionResponse};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::reusable::component::gacha::Gacha;
use crate::reusable::image_edit::gacha::GachaImage;
use crate::reusable::postgress::gacha::GachaPg;
use crate::reusable::utils::Color;
use crate::{MyErr, Reg, SlashBundle, COOLDOWN};

fn create_embed(user: &User, pg: &GachaPg, pity_max: i32) -> CreateEmbed {
    CreateEmbed::new()
        .title("🎰 MHF-Z Gacha Result")
        .description(format!("Pity Status: `{}/{}`\nTickets Remaining: `{}`", pg.pity, pity_max, pg.ticket))
        .author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
        .image("attachment://gacha.jpg")
        .color(Color::Random.throw())
        .footer(CreateEmbedFooter::new("Good luck, Commander!"))
}

#[hertz::hertz_slash_reg(10, true)]
pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
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

    let user_id = bnd.cmd.user.id.get();
    let user_key = format!("gacha_{}", user_id);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let cooldown_time = 5;

    {
        let mut cd_map = COOLDOWN.lock().await;
        if let Some(&last_pull) = cd_map.get(&user_key) {
            let wait_time = cooldown_time - (now - last_pull);
            if wait_time > 0 {
                return Err(MyErr::Custom(format!("⏳ **Slow down!** Please wait `{}s` before pulling again.", wait_time)));
            }
        }
        cd_map.insert(user_key.clone(), now);
    }

    let multi = bnd.cmd.data.options.iter().any(|opt| opt.name == "multi");
    let data = reg.pg.get_pity().await?;
    let gacha = Gacha::new().await?;

    let current_cost = if multi { gacha.config.cost_multi } else { gacha.config.cost_single };
    if data.ticket < current_cost {
        COOLDOWN.lock().await.remove(&user_key);
        return Err(MyErr::Custom(format!("❌ **Insufficient Tickets!**\nRequired: `{}`\nYour Balance: `{}`", current_cost, data.ticket)));
    }

    let avatar_url = bnd.cmd.user.face().replace(".webp", ".png").replace(".gif", ".png");
    let image_engine = GachaImage::new(&avatar_url).await?;

    let (g_pg, g_data, raw_img) = if !multi {
        let (pg, res) = gacha.single_pull(&data);
        let img = image_engine.single_pull(&res, bnd.pedia, bnd.image).await?;
        (pg, vec![res], img)
    } else {
        let (pg, res) = gacha.multi_pull(&data);
        let img = image_engine.multi_pull(res.clone(), bnd.pedia, bnd.image).await?;
        (pg, res, img)
    };

    let item_codes: Vec<_> = g_data.into_iter().map(|e| e.code).collect();
    reg.pg.send_distrib(&g_pg, &item_codes, reg.cid, bnd.pedia).await?;

    let attachment = CreateAttachment::bytes(raw_img, "gacha.jpg");
    let embed = create_embed(&bnd.cmd.user, &g_pg, gacha.config.pity_threshold);

    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().new_attachment(attachment).embed(embed)).await.map_err(MyErr::from)?;
    Ok(())
}