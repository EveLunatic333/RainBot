use serenity::gateway::ActivityData;
use serenity::model::prelude::UserId;
use serenity::model::prelude::Ready;
use serenity::prelude::*;
use crate::commands;
use crate::Init;
use crate::reusable::component::shutdown::Shutdown;
use tracing::{info, error};

pub async fn ready(ctx: &Context, ready: Ready, init: &Init) {
    let user = UserId::new(init.discord.author_id).to_user(&ctx.http).await.unwrap_or_default();
    if let Err(why) = Shutdown::load(ctx, init).await {
        error!("Cannot load cached file: {}", why);
    }

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        info!("Trying to Shutdown Gracefully");
        if let Err(why) = Shutdown::save().await {
            error!("Cannot save cache object: {}", why);
        }
        std::process::exit(0);
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
        }
    }

    ctx.set_activity(Some(ActivityData::competing("I want to die")));
}