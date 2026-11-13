use serenity::gateway::ActivityData;
use serenity::model::prelude::GuildId;
use serenity::model::prelude::UserId;
use serenity::model::prelude::Ready;
use serenity::prelude::*;
use crate::commands;
use crate::Init;
use crate::reusable::component::bounty::BountyTitle;
use crate::reusable::component::shutdown::Shutdown;
use tracing::{info, warn, error};
use std::sync::atomic::{AtomicBool, Ordering};

/// Guards against two shutdown paths racing (e.g. SIGTERM and then Ctrl-C).
static SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

/// Saves the CACHE once, then exits. Safe to call from more than one place.
async fn shutdown_now() {
    if SHUTDOWN_STARTED.swap(true, Ordering::SeqCst) {
        warn!("Shutdown already in progress; ignoring the second signal");
        return;
    }
    info!("Trying to Shutdown Gracefully");
    if let Err(why) = Shutdown::save().await {
        error!("Cannot save cache object: {}", why);
    }
    std::process::exit(0);
}

/// Waits for whichever stop signal arrives first, then saves and exits.
///
/// On unix both SIGINT (Ctrl-C) and SIGTERM are handled: `systemctl stop|restart`
/// and `pkill` deliver SIGTERM, and the old Ctrl-C-only handler skipped the save
/// for those paths. Other platforms only have Ctrl-C, which is all the CI and
/// development host needs.
async fn wait_for_stop_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => info!("Ctrl-C received"),
                    _ = sigterm.recv() => info!("SIGTERM received"),
                }
            }
            Err(e) => {
                // Saving on Ctrl-C only is better than never saving at all.
                error!("Cannot install the SIGTERM handler: {}", e);
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub async fn ready(ctx: &Context, ready: Ready, init: &Init) {
    let user = UserId::new(init.discord.author_id).to_user(&ctx.http).await.unwrap_or_default();
    if let Err(why) = Shutdown::load(ctx, init).await {
        error!("Cannot load cached file: {}", why);
    }

    // Save the CACHE on every ordinary way a service stops, not only on Ctrl-C.
    //
    // The old handler listened for `ctrl_c` alone. `systemctl stop|restart` and
    // `pkill` deliver SIGTERM, which skipped the handler entirely, so pending
    // bounty submissions were lost unless the caller happened to send Ctrl-C.
    tokio::spawn(async move {
        wait_for_stop_signal().await;
        shutdown_now().await;
    });

    info!("----------------------------------------------------------------");
    info!("-------------------------- START -------------------------------");
    info!("----------------------------------------------------------------");
    info!("🤖 Bot is running as {}", ready.user.tag());
    info!("🛠 {} is acknowledged as author", user.tag());

    let mut command = Vec::new();
    command.append(&mut commands::register::reg(init));
    command.append(&mut commands::binded::reg());
    command.append(&mut commands::misc::reg());
    command.append(&mut commands::guild::reg());
    command.append(&mut commands::admin::reg(init));

    if init.bot_config.gacha {
        command.append(&mut commands::gacha::reg());
    }
    if init.bot_config.server_market {
        command.append(&mut commands::market::reg());
    }
    if init.bot_config.bounty {
        command.append(&mut commands::bounty::reg());
    }

    for guild in &ready.guilds {
        if let Ok(x) = guild.id.to_partial_guild(&ctx.http).await {
            info!("🏛 {} is on guild **{}**", &ready.user.tag(), &x.name);
            if let Err(why) = guild.id.set_commands(&ctx.http, command.clone()).await {
                error!("❌ Error setting commands for {}: {:?}", x.name, why);
            }

            // A missing role only surfaces when a judge presses Approve, which is
            // far too late: the submission is already accepted at that point.
            // Report it at startup instead.
            if init.bot_config.bounty {
                check_bounty_roles(guild.id, ctx).await;
            }
        }
    }

    ctx.set_activity(Some(ActivityData::competing("I want to die")));
}

/// Logs every role referenced by `bounty_title.json` that the guild does not have.
async fn check_bounty_roles(guild: GuildId, ctx: &Context) {
    let titles = match BountyTitle::new().await {
        Ok(t) => t,
        Err(e) => {
            error!("Cannot read static/bounty_title.json: {:?}", e);
            return;
        }
    };
    let missing = titles.missing_roles(guild, &ctx.http).await;
    if missing.is_empty() {
        info!("✅ bounty_title.json: every role exists in this guild");
        return;
    }
    error!(
        missing = missing.len(),
        "❌ bounty_title.json references roles this guild does not have. \
         Approving those bounties WILL fail until this is fixed."
    );
    for (trigger, role_id) in missing {
        error!(trigger = %trigger, role_id = role_id, "Missing bounty role");
    }
}
