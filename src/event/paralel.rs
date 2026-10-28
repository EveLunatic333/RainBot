use std::time::SystemTime;
use std::path::Path;
use serenity::all::*;
use tracing::{info, warn, error};
use crate::{Init, PgConn, MyErr, MONITOR, BOUNTY};
use crate::reusable::component::bounty::{BBQ, BountyRefresh, Category};
use crate::reusable::postgress::server::Servers;
use crate::reusable::utils::{Color, MyTime};
use tokio::fs;

struct Server<'a> {
    pg: PgConn<'a>,
    msg: Message,
    ctx: &'a Context,
}

impl<'a> Server<'a> {
    async fn new(init: &'a Init, ctx: &'a Context) -> Result<Server<'a>, MyErr> {
        let channel = ChannelId::new(init.log_channel.info_channel);
        let msg = channel.message(&ctx.http, init.log_channel.info_channel_msg).await?;
        let pg = PgConn::create(init, init.discord.author_id.to_string()).await?;
        Ok(Server { pg, msg, ctx })
    }

    fn build_embed(&self, serv: Vec<Servers>) -> CreateEmbed {
        let mut fields = Vec::new();
        let mut pc = 0;
        for x in &serv {
            let z = format!("Description: {}\nPlayer Count: {}\nLand: {}", &x.description, x.cp, x.land);
            fields.push((&x.name, z, true));
            pc += x.cp;
        }
        let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        CreateEmbed::new()
            .title("Server Status")
            .description(format!("MHFZ Server Status updated <t:{}:R>\n**Total Player Count:** `{}`", timestamp, pc))
            .fields(fields)
            .color(Color::Random.throw())
    }

    async fn edit_msg(&mut self) -> Result<i32, MyErr> {
        let z = self.pg.get_server().await?;
        let pc = z.iter().map(|e| e.cp).sum::<i32>();
        let embed = self.build_embed(z);
        self.msg.edit(&self.ctx.http, EditMessage::new().embed(embed)).await?;
        self.pg.close().await;
        Ok(pc)
    }
}

pub async fn handle(ctx: &Context, init: &Init, state: i32, log: bool) -> i32 {
    let mon = MONITOR.lock().await;
    if !*mon {
        return 0;
    }
    match parallel_thread(ctx, init, state, log).await {
        Ok(x) => x,
        Err(_) => 0,
    }
}

async fn parallel_thread(ctx: &Context, init: &Init, state: i32, log: bool) -> Result<i32, MyErr> {
    scan_expired_bounties(ctx, init).await;
    let mut serv = Server::new(init, ctx).await?;
    let now = serv.edit_msg().await?;
    let crash = now == 0 && state != 0;
    if log || crash {
        let wish = if crash {
            UserId::new(119094696487288833).to_user(&ctx.http).await.ok()
        } else {
            None
        };
        logging(ctx, init, wish).await?;
    }
    Ok(now)
}

async fn emptying_log(path: &Path) -> Result<(), std::io::Error> {
    fs::remove_file(path).await?;
    fs::File::create(path).await?;
    Ok(())
}

async fn announce_crash(channel: &ChannelId, ctx: &Context, init: &Init) -> Result<(), MyErr> {
    let maintainer = format!("<@&{}>", init.server_role.maintainer_role);
    channel.send_message(&ctx.http, CreateMessage::new().content(format!("⚠️ SERVER MIGHT CRASH JUST ABOUT NOW {}", maintainer))).await?;
    Ok(())
}

async fn logging(ctx: &Context, init: &Init, wish: Option<User>) -> Result<(), MyErr> {
    let path = Path::new(".").join("log.txt");
    let channel = ChannelId::new(init.log_channel.erupe_channel);
    if !init.mhfz_config.sending_log {
        if wish.is_some() {
            announce_crash(&channel, ctx, init).await?;
        }
        return Ok(());
    }

    let attachment = match CreateAttachment::path(&path).await {
        Ok(att) => att,
        Err(_) => return Ok(()),
    };
    channel
        .send_message(
            &ctx.http,
            CreateMessage::new().add_file(attachment).content(&format!(
                "📂 **LOG AT <t:{}:F>**",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            )),
        )
        .await?;

    if let Some(user_wish) = wish {
        announce_crash(&channel, ctx, init).await?;
        if let Ok(att) = CreateAttachment::path(&path).await {
            let _ = user_wish
                .dm(
                    &ctx.http,
                    CreateMessage::new()
                        .content("🚨 **Server crash detected.** Please check the attached log immediately.")
                        .add_file(att),
                )
                .await;
        }
    }

    emptying_log(&path).await?;
    Ok(())
}

async fn scan_expired_bounties(ctx: &Context, init: &Init) {
    let mut expired_keys = Vec::new();
    let now = MyTime::now();
    let expire_threshold = 24 * 60 * 60;
    {
        let bounty_lock = BOUNTY.lock().await;
        for (user_id_str, submission) in bounty_lock.iter() {
            if now - submission.time > expire_threshold {
                expired_keys.push((
                    user_id_str.clone(),
                    submission.hunter[0].member.user.id,
                    submission.category.clone(),
                    submission.bbq.clone(),
                ));
            }
        }
    }
    if expired_keys.is_empty() {
        return;
    }
    let mut restored_free_slots: Vec<BBQ> = Vec::new();
    {
        let mut bounty_lock = BOUNTY.lock().await;
        for (user_id_str, discord_user_id, category, bbq) in expired_keys {
            if bounty_lock.remove(&user_id_str).is_some() {
                // Free Bounty slots are consumed on submission, so they must be
                // returned when the submission expires without a verdict.
                if category == Category::Free {
                    restored_free_slots.push(bbq);
                }
                if let Ok(dm_channel) = discord_user_id.create_dm_channel(&ctx.http).await {
                    let _ = dm_channel.send_message(&ctx.http, CreateMessage::new().content("⚠️ **System Notice:**\nYour submission has been automatically cancelled due to timeout (exceeding 24 hours without Admin review). Your *Pending* status has been reset in the database, and you may now submit a new report.")).await;
                }
                info!(user_id = %discord_user_id, "Auto-expired pending bounty submission after 24 hours.");
            }
        }
    }
    if !restored_free_slots.is_empty() {
        restore_expired_free_slots(ctx, init, restored_free_slots).await;
    }
}

/// Returns Free Bounty slots consumed by submissions that expired without a
/// verdict. All slots are restored in a single batch: one config save and one
/// cooldown board update, regardless of how many submissions expired.
async fn restore_expired_free_slots(ctx: &Context, init: &Init, bbqs: Vec<BBQ>) {
    match BountyRefresh::new(true).await {
        Ok(mut refresh) => {
            refresh.restore_slots(&bbqs);
            if let Err(e) = refresh.save(true).await {
                error!(error = %e, "Failed to persist restored Free Bounty counts after expiry");
                return;
            }
            if let Err(e) = refresh.cooldown_board(ctx, init).await {
                warn!(error = %e, "Failed to update cooldown board after restoring expired Free Bounty counts");
            }
            info!(restored = bbqs.len(), "Restored Free Bounty slots after 24h expiry.");
        }
        Err(e) => {
            error!(error = %e, "Failed to load cooldown config while restoring expired Free Bounty counts");
        }
    }
}