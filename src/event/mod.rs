pub mod interaction;
pub mod message;
mod ready;
mod paralel;
mod join;

use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use serenity::all::{GuildId, Member, User, Guild};
use serenity::async_trait;
use serenity::model::prelude::{Interaction, Message, Ready};
use serenity::prelude::*;
use tracing::{warn, info, error};
use crate::material::ItemPedia;
use crate::reusable::config::Init;
use crate::Images;
use self::interaction::handled;

use crate::reusable::component::autoresponse;

const MONITOR_INTERVAL_SECS: u64 = 60;
const VERBOSE_LOG_INTERVAL: u64 = 5;
static IS_MONITOR_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub struct Handler {
    pub config: Init,
    pub pedia: ItemPedia,
    pub image: Images,
}

#[async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        if !self.config.authorized_guilds.contains(&guild.id.get()) {
            warn!(unauthorized_guild_name = %guild.name, unauthorized_guild_id = %guild.id.get(), "Security breach: Bot detected in unauthorized guild. Initiating auto-leave protocol.");
            if let Err(e) = ctx.http.leave_guild(guild.id).await {
                error!(error = %e, guild_id = %guild.id.get(), "Failed to execute auto-leave protocol");
            } else {
                info!(guild_id = %guild.id.get(), "Auto-leave protocol executed: Bot extracted from unauthorized guild");
            }
        } else {
            info!(guild_name = %guild.name, guild_id = %guild.id.get(), "Bot connected to authorized base guild");
        }
    }

    async fn interaction_create(&self, ctx: Context, inter: Interaction) {
        handled(&ctx, &inter, &self.pedia, &self.config, &self.image).await;
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        ready::ready(&ctx, ready, &self.config).await;
        let init = self.config.clone();
        let ctx_clone = ctx.clone();
        if !IS_MONITOR_RUNNING.load(Ordering::SeqCst) {
            IS_MONITOR_RUNNING.store(true, Ordering::SeqCst);
            info!("Initializing background monitor loop...");
            tokio::spawn(async move {
                let mut state = 0;
                let mut iteration_count: u64 = 0;
                loop {
                    let verbose_log = iteration_count % VERBOSE_LOG_INTERVAL == 0;
                    state = paralel::handle(&ctx_clone, &init, state, verbose_log).await;
                    iteration_count += 1;
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(MONITOR_INTERVAL_SECS)) => {},
                        _ = tokio::signal::ctrl_c() => {
                            info!("Received shutdown signal, terminating background monitor loop");
                            IS_MONITOR_RUNNING.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            });
        } else {
            info!("Bot reconnected to Discord. Monitor loop is already running.");
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if let Err(e) = autoresponse::handle_auto_response(&ctx, &msg).await {
            warn!("Auto-response error: {:?}", e);
        }

        message::msg_handler(&ctx, &msg, &self.config, &self.pedia).await;
    }

    async fn guild_member_addition(&self, ctx: Context, member: Member) {
        if self.config.bot_config.member_join {
            if let Err(why) = join::join(&ctx, &member, &self.config).await {
                error!(error = ?why, user_id = %member.user.id, guild_id = %member.guild_id.get(), "Member join handling failed");
            }
        }
    }

    async fn guild_member_removal(&self, ctx: Context, guild: GuildId, user: User, _member: Option<Member>) {
        if self.config.bot_config.member_leave {
            if let Err(why) = join::leave(&ctx, &user, &guild, &self.config).await {
                error!(error = ?why, user_id = %user.id, guild_id = %guild.get(), "Member leave handling failed");
            }
        }
    }
}