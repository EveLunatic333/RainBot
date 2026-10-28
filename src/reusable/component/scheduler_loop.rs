use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn, error};
use serenity::prelude::Context;
use serenity::all::{ChannelId, CreateMessage};
use crate::reusable::component::scheduler::Scheduler;

pub async fn reminder_checker_loop(scheduler: Arc<Scheduler>) {
    info!("Starting reminder checker loop...");
    
    loop {
        match check_and_send_reminders(&scheduler).await {
            Ok(count) => {
                if count > 0 {
                    info!("Sent {} scheduled reminders", count);
                }
            }
            Err(e) => {
                error!("Error in reminder checker: {:?}", e);
            }
        }
        sleep(Duration::from_secs(300)).await;
    }
}

async fn check_and_send_reminders(scheduler: &Scheduler) -> Result<usize, crate::MyErr> {
    let due = scheduler.get_due_reminders().await;
    let mut sent_count = 0;

    for reminder in due {
        let channel = ChannelId::new(reminder.channel_id);
        
        let content = if let Some(role_id) = reminder.role_id {
            format!("<@&{}> {}", role_id, reminder.message)
        } else {
            reminder.message.clone()
        };

        match channel.say(&serenity::all::Http::default(), &content).await {
            Ok(_) => {
                scheduler.mark_sent(reminder.id).await?;
                sent_count += 1;
            }
            Err(e) => {
                warn!("Failed to send reminder {} to channel {}: {:?}", 
                    reminder.id, reminder.channel_id, e);
            }
        }
    }

    Ok(sent_count)
}