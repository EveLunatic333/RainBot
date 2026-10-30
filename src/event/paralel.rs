use std::time::SystemTime;
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::Path;
use serenity::all::*;
use tracing::{info, warn, error};
use crate::{Init, PgConn, MyErr, MONITOR, BOUNTY};
use crate::reusable::component::bounty::{BBQ, BountyRefresh, Category};
use crate::reusable::postgress::server::Servers;
use crate::reusable::utils::{Color, MyTime};
use tokio::fs;

/// Id of the status board currently being maintained, shared across monitor
/// iterations. Zero means "none adopted yet".
///
/// This **must** be process-wide state. `Server` is constructed inside every
/// iteration while `Init` is cloned once at startup and never mutated, so keeping the
/// id on the struct (or reading it back from `Init`) made each iteration target the
/// stale message again and post a brand-new board every 60 seconds.
static STATUS_BOARD_ID: AtomicU64 = AtomicU64::new(0);

/// Discord rejects any embed carrying more than 25 fields (`BASE_TYPE_MAX_LENGTH`
/// on `embeds.0.fields`). The `servers` table holds one row per world channel and
/// grows well past that on a populated deployment, so the status board must not map
/// every row straight to a field — doing so made every update fail with HTTP 400.
const MAX_EMBED_FIELDS: usize = 25;

struct Server<'a> {
    pg: PgConn<'a>,
    /// Id of the board the monitor is currently maintaining.
    /// `None` when there is nothing usable yet (deleted, unreadable, or not adopted);
    /// the first `edit_msg` then posts a fresh board and records it process-wide.
    board_id: Option<u64>,
    channel_id: ChannelId,
    ctx: &'a Context,
    init: &'a Init,
}

impl<'a> Server<'a> {
    async fn new(init: &'a Init, ctx: &'a Context) -> Result<Server<'a>, MyErr> {
        let channel_id = ChannelId::new(init.log_channel.info_channel);

        // Prefer the board adopted by an earlier iteration; fall back to the configured
        // one on a cold start. `Server` is rebuilt every minute while `Init` stays
        // frozen, so without this shared state a replacement would be forgotten and
        // re-posted on every tick.
        let adopted = STATUS_BOARD_ID.load(Ordering::SeqCst);
        let candidate = if adopted != 0 {
            adopted
        } else {
            init.log_channel.info_channel_msg
        };

        // Probe only decides *whether* the id is usable. The id is kept so `edit_msg`
        // can tell "there is a board to edit" from "there is nothing yet".
        let board_id = match channel_id.message(&ctx.http, candidate).await {
            Ok(msg) => Some(msg.id.get()),
            Err(why) => {
                warn!(error = %why, "Status board could not be fetched; a replacement will be posted");
                None
            }
        };

        let pg = PgConn::create(init, init.discord.author_id.to_string()).await?;
        Ok(Server { pg, board_id, channel_id, ctx, init })
    }

    fn build_embed(&self, serv: Vec<Servers>) -> CreateEmbed {
        let (fields, pc) = build_status_fields(serv);

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

        // Try to update the board we currently own. A failure here is the *only* signal
        // that the board is unusable: it was deleted (404) or it belongs to a bot
        // identity that no longer matches the running token (403 / 50005, which even
        // ADMINISTRATOR does not override — Discord forbids editing another user's
        // message, so re-posting is the only cure).
        let attempt = match self.board_id {
            Some(id) => match self.channel_id.message(&self.ctx.http, id).await {
                Ok(mut msg) => {
                    msg.edit(&self.ctx.http, EditMessage::new().embed(embed.clone()))
                        .await
                }
                // The board vanished between iterations: treat it as unwritable so the
                // recovery path below reposts instead of aborting the monitor.
                Err(why) => Err(why),
            },
            None => Err(serenity::Error::Http(
                serenity::http::HttpError::InvalidWebhook,
            )),
        };

        if let Err(why) = attempt {
            warn!(error = %why, "Status board is not writable; posting a replacement");
            let new_msg = self
                .channel_id
                .send_message(&self.ctx.http, CreateMessage::new().embed(embed))
                .await?;

            // Adopt the new board immediately and publish the id to the other
            // iterations. Without the shared write the next tick would probe the stale
            // id, fail again, and post yet another board every minute.
            let new_id = new_msg.id.get();
            self.board_id = Some(new_id);
            STATUS_BOARD_ID.store(new_id, Ordering::SeqCst);

            // The channel id is persisted as well: the replacement could have moved the
            // board to a different channel than the one configured at startup.
            self.persist_board_id().await;
        }

        self.pg.close().await;
        Ok(pc)
    }

    /// Records the replacement board's id so the next boot edits the right message.
    /// Failures are logged, never fatal: the board itself is already live.
    ///
    /// The file is re-read and only the two id fields are patched. Writing back the
    /// in-memory `Init` would **leak secrets**: `apply_env_overrides` copies
    /// `DISCORD_TOKEN`, `POSTGRES_PASSWORD` and `CHATGPT_TOKEN` from the environment
    /// into that struct, so serializing it persists real credentials into
    /// `config.json` — a file that is meant to hold the placeholders only.
    async fn persist_board_id(&self) {
        let Some(id) = self.board_id else { return };

        let mut on_disk: serde_json::Value = match fs::read_to_string("config.json").await {
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "config.json is not valid JSON; board id not persisted");
                    return;
                }
            },
            Err(e) => {
                warn!(error = %e, "Could not read config.json; board id not persisted");
                return;
            }
        };

        let channel = self.channel_id.get();
        on_disk["log_channel"]["info_channel_msg"] = serde_json::json!(id);
        on_disk["log_channel"]["info_channel"] = serde_json::json!(channel);

        match serde_json::to_string_pretty(&on_disk) {
            Ok(json) => {
                if let Err(e) = fs::write("config.json", json).await {
                    warn!(error = %e, "Could not persist the new status board id");
                } else {
                    info!(new = id, "Status board replaced and config updated");
                }
            }
            Err(e) => warn!(error = %e, "Could not serialize config after board replacement"),
        }
    }
}

/// Builds the status-board fields and the total player count.
///
/// Split out of `Server::build_embed` so the 25-field ceiling can be tested without
/// a live Discord `Context`. Returns the fields plus the player count summed over
/// **every** world, including ones omitted from the field list.
fn build_status_fields(serv: Vec<Servers>) -> (Vec<(String, String, bool)>, i32) {
    let pc: i32 = serv.iter().map(|x| x.cp).sum();

    // Busiest worlds first, so the ones players care about survive the cut.
    let mut sorted = serv;
    sorted.sort_by(|a, b| b.cp.cmp(&a.cp).then_with(|| a.name.cmp(&b.name)));

    let total = sorted.len();
    let hidden = total.saturating_sub(MAX_EMBED_FIELDS);

    // When worlds get dropped the trailing summary needs its own slot, so only
    // MAX_EMBED_FIELDS - 1 worlds may be listed. Otherwise the summary alone would
    // push the embed back over Discord's limit.
    let world_slots = if hidden > 0 { MAX_EMBED_FIELDS - 1 } else { MAX_EMBED_FIELDS };

    let mut fields: Vec<(String, String, bool)> = sorted
        .into_iter()
        .take(world_slots)
        .map(|x| {
            (
                x.name,
                format!("Description: {}\nPlayer Count: {}\nLand: {}", x.description, x.cp, x.land),
                true,
            )
        })
        .collect();

    // Never truncate silently: say how many worlds are not shown.
    if hidden > 0 {
        fields.push((
            format!("… and {} more world(s)", total - world_slots),
            format!(
                "Showing the {} busiest of {} worlds. Use the in-game server list for the rest.",
                world_slots, total
            ),
            false,
        ));
    }

    debug_assert!(
        fields.len() <= MAX_EMBED_FIELDS,
        "status board embed exceeds Discord's field limit: {}",
        fields.len()
    );

    (fields, pc)
}

pub async fn handle(ctx: &Context, init: &Init, state: i32, log: bool) -> i32 {
    let mon = MONITOR.lock().await;
    if !*mon {
        return 0;
    }
    match parallel_thread(ctx, init, state, log).await {
        Ok(x) => x,
        Err(e) => {
            // Monitor iterations used to discard their error entirely, which is why a
            // permanently failing status-board update went unnoticed. Surface it so a
            // misconfigured board or revoked permission is visible in the log.
            warn!(error = %e, "Server monitor iteration failed");
            0
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn world(name: &str, cp: i32, land: i32) -> Servers {
        Servers {
            name: name.to_string(),
            cp,
            land,
            description: format!("desc-{name}"),
        }
    }

    fn worlds(n: usize) -> Vec<Servers> {
        (0..n).map(|i| world(&format!("W{i:02}"), i as i32, 1)).collect()
    }

    #[test]
    fn accepts_exactly_the_field_limit() {
        let (fields, _) = build_status_fields(worlds(MAX_EMBED_FIELDS));
        assert_eq!(fields.len(), MAX_EMBED_FIELDS);
    }

    #[test]
    fn never_exceeds_discord_field_limit() {
        // Regression: a real deployment had 38 worlds, every update failed with
        // HTTP 400 BASE_TYPE_MAX_LENGTH, and the error was swallowed.
        for n in [0, 1, 24, 25, 26, 38, 100] {
            let (fields, _) = build_status_fields(worlds(n));
            assert!(
                fields.len() <= MAX_EMBED_FIELDS,
                "{n} worlds produced {} fields",
                fields.len()
            );
        }
    }

    #[test]
    fn truncation_is_announced() {
        let (fields, _) = build_status_fields(worlds(38));
        let last = fields.last().expect("summary field");
        assert!(
            last.0.contains("more world"),
            "summary field missing, got {:?}",
            last.0
        );
        // 24 worlds shown + 1 summary slot = 14 worlds hidden.
        assert!(last.0.contains("14"), "expected 14 hidden worlds, got {:?}", last.0);
    }

    #[test]
    fn player_count_covers_hidden_worlds() {
        let mut list = worlds(38);
        list[37].cp = 500;
        let (_, pc) = build_status_fields(list);
        let expected: i32 = worlds(38).iter().map(|w| w.cp).sum::<i32>() + 500 - 37;
        assert_eq!(pc, expected, "total must include worlds dropped from the embed");
    }

    #[test]
    fn busiest_worlds_are_kept() {
        let mut list = worlds(38);
        list[0].cp = 999; // W00 is busiest but sorts first alphabetically
        let (fields, _) = build_status_fields(list);
        assert_eq!(fields[0].0, "W00", "busiest world must be listed first");
    }
}