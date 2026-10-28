use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResponseLog {
    pub timestamp: DateTime<Utc>,
    pub user_id: u64,
    pub username: String,
    pub channel_id: u64,
    pub channel_name: String,
    pub trigger: String,
    pub response_type: String,
    pub matched_regex: bool,
    pub cooldown_applied: bool,
}

#[derive(Debug)]
pub struct AutoResponseLogger {
    logs: Arc<Mutex<Vec<AutoResponseLog>>>,
    log_file_path: String,
    max_logs_in_memory: usize,
}

impl AutoResponseLogger {
    pub fn new(log_file_path: &str, max_logs: usize) -> Self {
        AutoResponseLogger {
            logs: Arc::new(Mutex::new(Vec::new())),
            log_file_path: log_file_path.to_string(),
            max_logs_in_memory: max_logs,
        }
    }

    pub async fn log_response(&self, log: AutoResponseLog) {
        let mut logs = self.logs.lock().await;
        
        info!(
            "Auto-response triggered: {} by {} in #{}",
            log.trigger, log.username, log.channel_name
        );

        logs.push(log.clone());

        if logs.len() > self.max_logs_in_memory {
            logs.remove(0);
        }

        drop(logs);

        self.write_to_file(&log).await;
    }

    async fn write_to_file(&self, log: &AutoResponseLog) {
        let log_entry = serde_json::to_string(&log).unwrap_or_default();
        
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
        {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "{}", log_entry) {
                    warn!("Failed to write auto-response log to file: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to open log file: {}", e);
            }
        }
    }

    pub async fn get_recent_logs(&self, limit: usize) -> Vec<AutoResponseLog> {
        let logs = self.logs.lock().await;
        logs.iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn get_logs_by_user(&self, user_id: u64) -> Vec<AutoResponseLog> {
        let logs = self.logs.lock().await;
        logs.iter()
            .filter(|log| log.user_id == user_id)
            .cloned()
            .collect()
    }

    pub async fn get_logs_by_trigger(&self, trigger: &str) -> Vec<AutoResponseLog> {
        let logs = self.logs.lock().await;
        logs.iter()
            .filter(|log| log.trigger == trigger)
            .cloned()
            .collect()
    }

    pub async fn get_statistics(&self) -> AutoResponseStats {
        let logs = self.logs.lock().await;
        
        let total_triggers = logs.len();
        let unique_users = logs.iter()
            .map(|log| log.user_id)
            .collect::<std::collections::HashSet<_>>()
            .len();
        
        let mut trigger_counts = std::collections::HashMap::new();
        for log in logs.iter() {
            *trigger_counts.entry(log.trigger.clone()).or_insert(0) += 1;
        }

        let regex_matches = logs.iter()
            .filter(|log| log.matched_regex)
            .count();

        AutoResponseStats {
            total_triggers,
            unique_users,
            trigger_counts,
            regex_matches,
            text_responses: logs.iter()
                .filter(|log| log.response_type == "text")
                .count(),
            embed_responses: logs.iter()
                .filter(|log| log.response_type == "embed")
                .count(),
        }
    }

    pub async fn clear_old_logs(&self, older_than_days: i64) -> usize {
        let mut logs = self.logs.lock().await;
        let initial_len = logs.len();
        
        let cutoff = Utc::now() - chrono::Duration::days(older_than_days);
        logs.retain(|log| log.timestamp > cutoff);
        
        let removed = initial_len - logs.len();
        
        if removed > 0 {
            info!("Cleared {} auto-response logs older than {} days", removed, older_than_days);
        }
        
        removed
    }
}

#[derive(Debug, Clone)]
pub struct AutoResponseStats {
    pub total_triggers: usize,
    pub unique_users: usize,
    pub trigger_counts: std::collections::HashMap<String, usize>,
    pub regex_matches: usize,
    pub text_responses: usize,
    pub embed_responses: usize,
}

impl AutoResponseStats {
    pub fn to_embed(&self) -> String {
        let mut output = String::from("```css\n");
        output.push_str(&format!("Total Triggers: {}\n", self.total_triggers));
        output.push_str(&format!("Unique Users: {}\n", self.unique_users));
        output.push_str(&format!("Regex Matches: {}\n", self.regex_matches));
        output.push_str(&format!("Text Responses: {}\n", self.text_responses));
        output.push_str(&format!("Embed Responses: {}\n\n", self.embed_responses));
        
        output.push_str("Top Triggers:\n");
        let mut triggers: Vec<_> = self.trigger_counts.iter().collect();
        triggers.sort_by(|a, b| b.1.cmp(a.1));
        
        for (trigger, count) in triggers.iter().take(10) {
            output.push_str(&format!("  {}: {}\n", trigger, count));
        }
        
        output.push_str("```");
        output
    }
}

pub fn create_log_entry(
    user_id: u64,
    username: &str,
    channel_id: u64,
    channel_name: &str,
    trigger: &str,
    response_type: &str,
    matched_regex: bool,
    cooldown_applied: bool,
) -> AutoResponseLog {
    AutoResponseLog {
        timestamp: Utc::now(),
        user_id,
        username: username.to_string(),
        channel_id,
        channel_name: channel_name.to_string(),
        trigger: trigger.to_string(),
        response_type: response_type.to_string(),
        matched_regex,
        cooldown_applied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_logger_creation() {
        let logger = AutoResponseLogger::new("test_logs.json", 100);
        assert_eq!(logger.max_logs_in_memory, 100);
    }

    #[tokio::test]
    async fn test_log_creation() {
        let log = create_log_entry(
            123456789,
            "TestUser",
            987654321,
            "general",
            "!bounty",
            "embed",
            false,
            true,
        );
        
        assert_eq!(log.user_id, 123456789);
        assert_eq!(log.trigger, "!bounty");
        assert_eq!(log.response_type, "embed");
    }
}