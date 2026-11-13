use std::time::SystemTime;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::path::Path;
use serenity::all::*;
use tracing::{debug, info, warn, error};
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

/// Title every status board carries. It is also the marker used to recognise a board
/// in the channel history, so a board whose id was lost can be adopted again instead
/// of being reposted.
const BOARD_TITLE: &str = "Server Status";

/// How many consecutive iterations may fail before a board is considered lost.
///
/// One failed read is not evidence of anything: the board was reachable a minute
/// earlier, and a transient 5xx, timeout or rate limit produces exactly the same
/// error as a deleted message. Failing silently for a while is the cheap outcome;
/// abandoning a live board leaves a frozen duplicate in the channel forever.
const MAX_TOLERATED_FAILURES: u32 = 5;

/// How many recent channel messages are scanned when looking for a board to adopt.
/// Boards are rare, so a single page is plenty.
const BOARD_SCAN_LIMIT: u8 = 100;

/// Discord's error code for "Unknown Message".
const CODE_UNKNOWN_MESSAGE: isize = 10008;
/// Discord's error code for "Cannot edit a message authored by another user".
const CODE_FOREIGN_AUTHOR: isize = 50005;
/// serenity's sentinel for "the error body could not be decoded", so Discord's own
/// code was lost. Treated as transient: the transport failed, not the message.
const CODE_UNDECODABLE: isize = -1;

/// Delay before each retry of a failed board read. The first attempt already
/// happened, so the board is tried `READ_RETRY_DELAYS.len() + 1` times per iteration.
/// The total pause (7s) stays well inside the 60s monitor tick.
const READ_RETRY_DELAYS: [std::time::Duration; 2] = [
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(5),
];

/// Counts consecutive iterations in which the board could not be updated, shared
/// across iterations for the same reason [`STATUS_BOARD_ID`] is.
static BOARD_READ_FAILURES: AtomicU32 = AtomicU32::new(0);

/// What a failed board update actually tells us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeError {
    /// Proof the board cannot be used again: Discord says the message is gone, or the
    /// bot may not edit it because another account authored it.
    Permanent,
    /// A transient failure (5xx, timeout, rate limit, undecodable error body, revoked
    /// permission). The board may well still exist, so it must not be abandoned.
    Transient,
}

/// Decides whether a failed request is proof of loss or just noise, from the two
/// facts that actually distinguish them.
///
/// Kept free of serenity types so the decision itself is directly testable: serenity's
/// `ErrorResponse` is `#[non_exhaustive]` and carries no serde impls, so it cannot be
/// constructed in a test from this crate.
///
/// The fallback `Transient` is deliberate: a wrong "transient" only costs a delayed
/// recovery, while a wrong "permanent" costs a duplicate board nobody ever removes.
fn classify_parts(status: Option<u16>, code: isize) -> ProbeError {
    // Discord's own error codes are the strongest signal: 10008 Unknown Message and
    // 50005 cannot-edit-another-user's-message both mean the id will never work again,
    // whatever the transport reported.
    if code == CODE_UNKNOWN_MESSAGE || code == CODE_FOREIGN_AUTHOR {
        return ProbeError::Permanent;
    }

    // `-1` is serenity's sentinel for "the error body would not decode", so no Discord
    // code survived. The status is then all we have, and a proxy, a rate limit or an
    // outage can produce any of them — including a 404 — so none of it proves the board
    // is gone. This is the shape that duplicated the board on 13 Nov.
    if code == CODE_UNDECODABLE {
        return ProbeError::Transient;
    }

    match status {
        // 404 with a decoded body: Discord says the message is not there. The `code` is
        // normalised away here because `-1` already returned above, so reaching this arm
        // means the body decoded and Discord's own code simply was not one of the two
        // permanent ones — 0 is the common case for a bare error.
        Some(404) => ProbeError::Permanent,
        // Everything else (403 revoked permission, 5xx, 429) leaves the message in place.
        _ => ProbeError::Transient,
    }
}

/// Adapter that maps a serenity error onto [`classify_parts`].
fn classify(err: &serenity::Error) -> ProbeError {
    use serenity::http::HttpError;

    match err {
        serenity::Error::Http(http_err) => match http_err {
            // `code` is -1 when Discord's error body could not be decoded, which is the
            // 13 Nov case: the status code is all we have, and it is not proof of loss.
            HttpError::UnsuccessfulRequest(resp) => {
                classify_parts(Some(resp.status_code.as_u16()), resp.error.code)
            }
            // The sentinel this module raises itself when it has no board id at all —
            // a real loss, not the "not a valid webhook" it prints.
            HttpError::InvalidWebhook => ProbeError::Permanent,
            _ => ProbeError::Transient,
        },
        // Gateway, model and IO errors say nothing about whether the message exists.
        _ => ProbeError::Transient,
    }
}

/// True when a fetched message really is a status board: posted by this bot, with the
/// board embed. Guards against adopting an unrelated message with a matching id.
fn is_status_board(msg: &Message, bot_id: UserId) -> bool {
    msg.author.id == bot_id && msg.embeds.iter().any(|e| e.title.as_deref() == Some(BOARD_TITLE))
}

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

        // Set when `board_id` is right but `config.json` still names the dead one, so the
        // adoption is written back once the server is built.
        let mut needs_persist = false;

        // Probe only decides *whether* the id is usable. The id is kept so `edit_msg`
        // can tell "there is a board to edit" from "there is nothing yet".
        let board_id = match read_board(channel_id, ctx, candidate).await {
            Ok(msg) => {
                BOARD_READ_FAILURES.store(0, Ordering::SeqCst);
                Some(msg.id.get())
            }
            Err(why) => match classify(&why) {
                ProbeError::Permanent => {
                    // The configured id is gone for good. Before posting anything, look
                    // for a board this bot already owns: a restart or a lost id must adopt
                    // the existing board instead of adding one more to the channel.
                    match find_existing_board(channel_id, ctx).await {
                        Some(msg) => {
                            info!(
                                old = candidate,
                                adopted = msg.id.get(),
                                "Configured status board is gone; adopted the existing board instead of posting a new one"
                            );
                            BOARD_READ_FAILURES.store(0, Ordering::SeqCst);
                            needs_persist = true;
                            Some(msg.id.get())
                        }
                        None => {
                            warn!(
                                board = candidate,
                                error = %why,
                                "Status board is gone and no existing board could be adopted; a replacement will be posted"
                            );
                            None
                        }
                    }
                }
                ProbeError::Transient => {
                    // Not evidence of loss: keep the id and edit it anyway. `edit_msg`
                    // then retries and only gives up after MAX_TOLERATED_FAILURES ticks.
                    debug!(board = candidate, error = %why, "Status board could not be read; keeping it and retrying");
                    Some(candidate)
                }
            },
        };

        let pg = PgConn::create(init, init.discord.author_id.to_string()).await?;
        let server = Server { pg, board_id, channel_id, ctx, init };

        // Without this the adopted id would live only in memory: the next restart would
        // read the dead id from config again and repeat the whole search.
        if needs_persist {
            STATUS_BOARD_ID.store(board_id.unwrap_or_default(), Ordering::SeqCst);
            server.persist_board_id().await;
        }

        Ok(server)
    }

    fn build_embed(&self, serv: Vec<Servers>) -> CreateEmbed {
        let (fields, pc) = build_status_fields(serv);

        let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        CreateEmbed::new()
            .title(BOARD_TITLE)
            .description(format!("MHFZ Server Status updated <t:{}:R>\n**Total Player Count:** `{}`", timestamp, pc))
            .fields(fields)
            .color(Color::Random.throw())
    }

    async fn edit_msg(&mut self) -> Result<i32, MyErr> {
        // A board that survived a failed read still gets its update: the skip counter
        // alone must never stop the monitor from trying.
        let z = self.pg.get_server().await?;
        let pc = z.iter().map(|e| e.cp).sum::<i32>();
        let embed = self.build_embed(z);

        // Try to update the board we currently own, retrying transient failures.
        //
        // Only a `Permanent` verdict — or MAX_TOLERATED_FAILURES consecutive ticks of
        // transient failures — may post a new board. Anything less and a single blip
        // strands a live board in the channel, which is exactly how the channel ended up
        // with eight frozen boards next to the one being maintained.
        let attempt = match self.board_id {
            Some(id) => edit_board(self.channel_id, self.ctx, id, embed.clone()).await,
            // No board at all: this is a real loss, not the webhook error it prints.
            None => Err(serenity::Error::Http(
                serenity::http::HttpError::InvalidWebhook,
            )),
        };

        match attempt {
            Ok(()) => {
                BOARD_READ_FAILURES.store(0, Ordering::SeqCst);
            }
            Err(why) => {
                let verdict = classify(&why);

                if verdict == ProbeError::Transient {
                    let failures = BOARD_READ_FAILURES.fetch_add(1, Ordering::SeqCst) + 1;
                    if failures < MAX_TOLERATED_FAILURES {
                        // Keep the board, skip this update, try again on the next tick.
                        warn!(
                            board = self.board_id.unwrap_or_default(),
                            failures,
                            tolerated = MAX_TOLERATED_FAILURES,
                            error = %why,
                            "Status board update failed (transient); keeping the current board and retrying"
                        );
                        self.pg.close().await;
                        return Ok(pc);
                    }
                    warn!(
                        board = self.board_id.unwrap_or_default(),
                        failures,
                        error = %why,
                        "Status board stayed unreachable; posting a replacement"
                    );
                } else {
                    warn!(error = %why, "Status board is permanently gone; posting a replacement");
                }

                // Adopt an existing board when one is already in the channel. This is the
                // guard against duplicates: a stale id must never cost a second board.
                let existing = find_existing_board(self.channel_id, self.ctx).await;

                match existing {
                    Some(msg) => {
                        let adopted = msg.id.get();
                        if let Err(e) = edit_board(self.channel_id, self.ctx, adopted, embed.clone()).await {
                            warn!(error = %e, board = adopted, "Adopted status board is not editable; posting a replacement");
                            self.post_replacement(embed).await?;
                        } else {
                            info!(board = adopted, "Re-adopted the existing status board instead of posting a duplicate");
                            self.board_id = Some(adopted);
                            STATUS_BOARD_ID.store(adopted, Ordering::SeqCst);
                            BOARD_READ_FAILURES.store(0, Ordering::SeqCst);
                        }
                    }
                    None => self.post_replacement(embed).await?,
                }
            }
        }

        self.pg.close().await;
        Ok(pc)
    }

    /// Posts a brand-new board and records its id process-wide and on disk.
    ///
    /// The caller must have established that no usable board exists: this is the only
    /// place a duplicate can be created.
    async fn post_replacement(&mut self, embed: CreateEmbed) -> Result<(), MyErr> {
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
        BOARD_READ_FAILURES.store(0, Ordering::SeqCst);

        // The channel id is persisted as well: the replacement could have moved the
        // board to a different channel than the one configured at startup.
        self.persist_board_id().await;
        Ok(())
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

/// Reads one board, retrying a few times before reporting failure.
///
/// Any error is returned only after every attempt failed, so a caller that treats the
/// error as proof of loss has had three chances to be wrong.
async fn read_board(channel_id: ChannelId, ctx: &Context, id: u64) -> serenity::Result<Message> {
    let mut last = match channel_id.message(&ctx.http, id).await {
        Ok(msg) => return Ok(msg),
        Err(why) => why,
    };

    for delay in READ_RETRY_DELAYS {
        // A permanent verdict is not worth waiting for; retrying cannot change it.
        if classify(&last) == ProbeError::Permanent {
            return Err(last);
        }
        tokio::time::sleep(delay).await;
        match channel_id.message(&ctx.http, id).await {
            Ok(msg) => return Ok(msg),
            Err(why) => last = why,
        }
    }

    Err(last)
}

/// Edits a board, retrying a few times before reporting failure.
///
/// Re-reads the message on every attempt because `Message::edit` needs an owned
/// message and consumes it.
async fn edit_board(
    channel_id: ChannelId,
    ctx: &Context,
    id: u64,
    embed: CreateEmbed,
) -> serenity::Result<()> {
    let mut last = match edit_once(channel_id, ctx, id, embed.clone()).await {
        Ok(()) => return Ok(()),
        Err(why) => why,
    };

    for delay in READ_RETRY_DELAYS {
        if classify(&last) == ProbeError::Permanent {
            return Err(last);
        }
        tokio::time::sleep(delay).await;
        match edit_once(channel_id, ctx, id, embed.clone()).await {
            Ok(()) => return Ok(()),
            Err(why) => last = why,
        }
    }

    Err(last)
}

/// One edit attempt. Re-reads the message first because editing a board also proves
/// the message is still readable, and `Message::edit` needs an owned message.
async fn edit_once(
    channel_id: ChannelId,
    ctx: &Context,
    id: u64,
    embed: CreateEmbed,
) -> serenity::Result<()> {
    let mut msg = channel_id.message(&ctx.http, id).await?;
    msg.edit(&ctx.http, EditMessage::new().embed(embed)).await
}

/// The bot's own account id, taken from the gateway cache.
///
/// Deliberately **not** `init.discord.author_id`: that setting is the bot owner's
/// user id (it is compared against the invoking user in `clearcache` and used as the
/// database driver id), so using it here made adoption look for a board the owner had
/// posted and never find the bot's own board.
fn bot_user_id(ctx: &Context) -> UserId {
    ctx.cache.current_user().id
}

/// Finds a status board this bot already posted, most recent first.
///
/// Used before posting a replacement so a board whose id was lost is adopted rather
/// than duplicated. Returns `None` when the history is unreadable, which makes the
/// caller fall back to posting — the old behaviour.
async fn find_existing_board(channel_id: ChannelId, ctx: &Context) -> Option<Message> {
    let bot_id = bot_user_id(ctx);

    let history = match channel_id
        .messages(&ctx.http, GetMessages::new().limit(BOARD_SCAN_LIMIT))
        .await
    {
        Ok(msgs) => msgs,
        Err(why) => {
            warn!(error = %why, "Could not read the channel history to look for an existing status board");
            return None;
        }
    };

    history.into_iter().find(|msg| is_status_board(msg, bot_id))
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

    /// Builds a message carrying one embed with the given title, authored by `author`.
    fn board_message(author: u64, title: &str) -> Message {
        let mut embed = Embed::default();
        embed.title = Some(title.to_string());
        let mut msg = Message::default();
        msg.author = User::default();
        msg.author.id = UserId::new(author);
        msg.embeds = vec![embed];
        msg
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

    /// The exact shape Discord produced on 13 Nov: an error body serenity could not
    /// decode, so it stored `code: -1` and kept only the status.
    #[test]
    fn the_undecodable_body_that_caused_the_duplicate_is_transient() {
        // Regression for 2026-11-13T02:33:43Z: this reposted a board even though the old
        // one was still alive. It must never count as proof of loss.
        assert_eq!(classify_parts(Some(502), -1), ProbeError::Transient);
        assert_eq!(classify_parts(Some(429), -1), ProbeError::Transient);
        // Even a 404 with an undecodable body is suspicious rather than conclusive:
        // Discord states the code, and without it we treat the board as possibly alive.
        assert_eq!(classify_parts(Some(404), -1), ProbeError::Transient);
    }

    #[test]
    fn a_deleted_board_is_a_permanent_loss() {
        // The allowed path: only this may trigger a new board. Both signals agree here.
        assert_eq!(
            classify_parts(Some(404), CODE_UNKNOWN_MESSAGE),
            ProbeError::Permanent
        );
        // And the status alone is enough when Discord sends no specific code: this is
        // the arm a plain 404 with an empty body takes, so it must be exercised too.
        assert_eq!(classify_parts(Some(404), 0), ProbeError::Permanent);
    }

    #[test]
    fn a_foreign_author_is_a_permanent_loss() {
        assert_eq!(
            classify_parts(Some(403), CODE_FOREIGN_AUTHOR),
            ProbeError::Permanent,
            "Discord forbids editing another account's message; reposting is the only cure"
        );
    }

    #[test]
    fn a_server_error_is_transient() {
        assert_eq!(classify_parts(Some(500), 0), ProbeError::Transient);
        assert_eq!(classify_parts(Some(503), 0), ProbeError::Transient);
    }

    #[test]
    fn a_rate_limit_is_transient() {
        assert_eq!(classify_parts(Some(429), 0), ProbeError::Transient);
    }

    #[test]
    fn a_revoked_permission_is_repairable_not_permanent() {
        // 403 alone may be a missing permission an operator can grant back.
        assert_eq!(classify_parts(Some(403), 0), ProbeError::Transient);
    }

    #[test]
    fn a_known_permanent_code_wins_over_the_status() {
        // Discord sometimes reports 404 together with code 10008; both agree here, but
        // the code must win when the status is unhelpful.
        assert_eq!(
            classify_parts(Some(400), CODE_UNKNOWN_MESSAGE),
            ProbeError::Permanent
        );
    }

    #[test]
    fn a_missing_board_id_counts_as_a_permanent_loss() {
        // The sentinel this module raises itself. It prints "not a valid webhook",
        // which used to look like a webhook misconfiguration rather than "no board".
        let err = serenity::Error::Http(serenity::http::HttpError::InvalidWebhook);
        assert_eq!(classify(&err), ProbeError::Permanent);
    }

    #[test]
    fn errors_that_say_nothing_about_the_message_are_transient() {
        // Gateway close frames are by far the most common error in the log; treating
        // them as proof of loss is what orphans boards.
        assert_eq!(classify(&serenity::Error::Other("gateway closed")), ProbeError::Transient);
    }

    #[test]
    fn a_board_from_another_account_is_not_adopted() {
        let msg = board_message(7, BOARD_TITLE);
        assert!(
            !is_status_board(&msg, UserId::new(99)),
            "only this bot's own board may be adopted"
        );
    }

    #[test]
    fn a_plain_message_is_not_adopted() {
        let msg = board_message(99, "Some other announcement");
        assert!(!is_status_board(&msg, UserId::new(99)));
    }

    #[test]
    fn a_message_without_embeds_is_not_adopted() {
        let msg = board_message(99, BOARD_TITLE);
        let mut bare = msg.clone();
        bare.embeds.clear();
        assert!(!is_status_board(&bare, UserId::new(99)));
    }

    #[test]
    fn the_real_board_is_adopted() {
        let msg = board_message(99, BOARD_TITLE);
        assert!(is_status_board(&msg, UserId::new(99)));
    }

    #[test]
    fn one_blip_is_never_enough_to_abandon_a_board() {
        // A duplicate is permanent and manual to remove; a delayed update is not.
        assert!(
            MAX_TOLERATED_FAILURES >= 3,
            "tolerating fewer than three failures would orphan a board on a single 5xx"
        );
        // The retries must also fit inside one monitor tick, or a slow board would make
        // the monitor fall behind its own schedule.
        let total: std::time::Duration = READ_RETRY_DELAYS.iter().sum();
        assert!(
            total < std::time::Duration::from_secs(60),
            "retries must stay inside the 60s monitor interval, got {total:?}"
        );
    }
}