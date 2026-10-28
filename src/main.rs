#![allow(dead_code)]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod reusable;
pub mod commands;
pub mod material;
mod event;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use event::Handler;
use lazy_static::lazy_static;
use material::ItemPedia;
use commands::misc::party::PartyRecord;
use reusable::component::bounty::BountySubmit;
use reusable::component::error::ErrorLog;
use reusable::config::Init;
use reusable::component::Components;
use reusable::postgress::PgConn;
use reusable::component::registered::Reg;
use reusable::component::discord::AppReg;
use reusable::image_edit::Images;
use reusable::gpt::chat::CompModel;
use event::interaction::{ModalBundle, SlashBundle, ComponentBundle, Mybundle};
use event::message::MsgBundle;
use reusable::component::{MyErr, Mytrait};
use reusable::component::autoresponse::{AutoResponse, AutoResponseConfig};
use reusable::component::autoresponse_logger::AutoResponseLogger;
use reusable::component::scheduler::Scheduler;
use serenity::all::{GatewayIntents, Http};
use serenity::prelude::Client;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
 

lazy_static! {
    pub static ref COOLDOWN: Mutex<HashMap<String, i64>> = Mutex::new(HashMap::new());
    pub static ref CHAT: Mutex<HashMap<String, CompModel>> = Mutex::new(HashMap::new());
    pub static ref MONITOR: Mutex<bool> = Mutex::new(true);
    pub static ref BOUNTY: Mutex<HashMap<String, BountySubmit>> = Mutex::new(HashMap::new());
    // Tracks users that are currently filling out a bounty submission form.
    // Prevents two concurrent /bounty submit flows from the same user, which
    // previously let the second flow overwrite the first one's pending data.
    pub static ref SUBMIT_LOCK: Mutex<std::collections::HashSet<String>> = Mutex::new(std::collections::HashSet::new());
    // Active party recruitment posts, keyed by recruitment message id. Used by
    // the 30-minute inactivity sweep and persisted through the CACHE file.
    pub static ref PARTIES: Mutex<HashMap<String, PartyRecord>> = Mutex::new(HashMap::new());
    // Party recruitment posts currently being processed by a button click.
    // Prevents two simultaneous clicks from overwriting each other's update.
    pub static ref PARTY_LOCK: Mutex<std::collections::HashSet<String>> = Mutex::new(std::collections::HashSet::new());
    pub static ref AUTO_RESPONSE_LOGGER: Arc<AutoResponseLogger> = Arc::new(AutoResponseLogger::new("logs/autoresponse.json", 1000));
    pub static ref AUTO_CACHE: RwLock<HashMap<String, AutoResponse>> = RwLock::new(HashMap::new());
    pub static ref SCHEDULER: std::sync::OnceLock<Arc<Scheduler>> = std::sync::OnceLock::new();
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::new(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".into()),
            ),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn parse_message_content(input: &str) -> String {
    input.replace("\\n", "\n")
         .replace("\\t", "\t")
         .replace("\\r", "\r")
}

#[tokio::main]
async fn main() {
    init_tracing();
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MEMBERS;

    let conf = match Init::new().await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("Configuration initialization failed: {}", e);
            return;
        }
    };

    reusable::postgress::init_pool(&conf)
        .await
        .expect("Critical: Database pool initialization failed");

    let image_handler = Images::new().await.expect("Critical: Image processor initialization failed");

    let auto_config = AutoResponseConfig::load().await.unwrap_or_else(|_| AutoResponseConfig { responses: vec![] });
    {
        let mut cache = AUTO_CACHE.write().await;
        for res in auto_config.responses {
            cache.insert(res.trigger.clone(), res);
        }
    }
    tracing::info!("Auto-response cache initialized with {} entries", AUTO_CACHE.read().await.len());

    let http = Arc::new(Http::new(&conf.discord.token));

    let pool = reusable::postgress::get_pool().clone();
    let scheduler = Arc::new(Scheduler::new(pool));
    scheduler.init().await.expect("Failed to initialize scheduler");
    SCHEDULER.set(scheduler.clone()).expect("Failed to set scheduler");

    let scheduler_clone = scheduler.clone();
    let http_clone = http.clone();
    tokio::spawn(async move {
        reminder_checker_loop(scheduler_clone, http_clone).await;
    });

    let mut client = Client::builder(conf.discord.token.clone(), intents)
        .event_handler(Handler {
            config: conf,
            pedia: ItemPedia::default(),
            image: image_handler,
        })
        .await
        .expect("Critical: Discord client creation failed");

    tracing::info!("Bot is starting...");

    if let Err(e) = client.start().await {
        tracing::error!("Client runtime error: {:?}", e);
    }
}

async fn reminder_checker_loop(scheduler: Arc<Scheduler>, http: Arc<Http>) {
    use std::time::Duration;
    use tokio::time::sleep;
    use tracing::{info, error};
    info!("Starting reminder checker loop...");

    loop {
        match check_and_send_reminders(&scheduler, &http).await {
            Ok(count) => {
                if count > 0 {
                    info!("Sent {} scheduled reminders", count);
                }
            }
            Err(e) => {
                error!("Error in reminder checker: {:?}", e);
            }
        }
        // Close party recruitment posts that have been inactive for 30 minutes.
        commands::misc::party::sweep_inactive_parties(&http).await;
        sleep(Duration::from_secs(60)).await;
    }
}

async fn check_and_send_reminders(scheduler: &Scheduler, http: &Http) -> Result<usize, MyErr> {
    use tracing::warn;
    use serenity::all::ChannelId;
    
    let due = scheduler.get_due_reminders().await;
    let mut sent_count = 0;

    for reminder in due {
        let channel = ChannelId::new(reminder.channel_id as u64);
        
        let parsed_message = parse_message_content(&reminder.message);
        
        let content = if let Some(role_id) = reminder.role_id {
            if role_id > 0 {
                format!("<@&{}> {}", role_id as u64, parsed_message)
            } else {
                parsed_message
            }
        } else {
            parsed_message
        };

        match channel.say(http, &content).await {
            Ok(_) => {
                if let Err(e) = scheduler.mark_sent(reminder.id).await {
                    warn!("Failed to mark reminder {} as sent: {:?}", reminder.id, e);
                }
                sent_count += 1;
            }
            Err(e) => {
                warn!("Failed to send reminder {} to channel {}: {:?}", reminder.id, reminder.channel_id, e);
            }
        }
    }

    Ok(sent_count)
}