use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::MyErr;
use std::collections::HashMap;
use tracing::warn;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Init {
    pub bounty: Bounty,
    pub chat_gpt: ChatGpt,
    pub server_role: ServerRole,
    pub log_channel: LogChannels,
    pub bot_config: BotConfig,
    pub mhfz_config: MhfzConfig,
    pub postgress: Postgress,
    pub discord: Discord,
    pub lfg_roles: LfgRoles,
    #[serde(default)]
    pub commands_toggle: HashMap<String, bool>,
    #[serde(default)]
    pub authorized_guilds: Vec<u64>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Discord {
    pub token: String,
    pub prefix: String,
    pub author_id: u64,
    #[serde(default)]
    pub sightengine_enabled: bool,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Postgress {
    pub host: String,
    pub password: String,
    pub port: u16,
    pub database: String,
    pub user: String,
}

impl Postgress {
    pub fn database_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user,
            urlencoding::encode(&self.password),
            self.host,
            self.port,
            self.database
        )
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct MhfzConfig {
    pub account_creation: bool,
    pub sending_log: bool,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct BotConfig {
    pub member_join: bool,
    pub member_leave: bool,
    pub gacha: bool,
    pub bounty: bool,
    pub server_market: bool,
    pub transfer_timeout: u64,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LogChannels {
    pub err_channel: u64,
    pub account_channel: u64,
    pub transfer_channel: u64,
    pub erupe_channel: u64,
    pub info_channel: u64,
    pub info_channel_msg: u64,
    pub market_channel: u64,
    pub join_channel: u64,
    pub leave_channel: u64,
    pub market_channel_msg: u64,
    pub request_channel: u64,
    #[serde(default)]
    pub party_ch: u64,
    pub ticket_channel_id: u64,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct ServerRole {
    pub admin_role: u64,
    pub bartender_role: u64,
    pub member_role: u64,
    pub register_role: u64,
    pub judge_role: u64,
    pub maintainer_role: u64,
    pub weekly_distributor: u64,
    pub speedrun_supervisor_role: u64, 
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Bounty {
    pub board_ch: u64,
    pub conquered_ch: u64,
    pub receptionist_ch: u64,
    pub promotion_ch: u64,
    pub cooldown_ch: u64,
    pub cooldown_msg: u64,
    pub judge_ch: u64,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct ChatGpt {
    pub token: String,
    pub timeout: u32,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LfgRoles {
    pub raviente_raid: u64,
    pub party_helper: u64,
    pub bounty_hunter: u64,
    pub road_runner: u64,
    pub gacha_enjoyer: u64,
    pub musou_slayer: u64,
}

impl Init {
    fn path() -> PathBuf {
        Path::new(".").join("config.json")
    }

    pub async fn save(&self) -> Result<(), MyErr> {
        let content = serde_json::to_string_pretty(self)?;
        Ok(tokio::fs::write(&Init::path(), content).await?)
    }

    fn apply_env_overrides(&mut self) {
        let _ = dotenvy::dotenv();
        if let Ok(token) = std::env::var("DISCORD_TOKEN") {
            self.discord.token = token;
        }
        if let Ok(password) = std::env::var("POSTGRES_PASSWORD") {
            self.postgress.password = password;
        }
        if let Ok(gpt_token) = std::env::var("CHATGPT_TOKEN") {
            self.chat_gpt.token = gpt_token;
        }
    }

    pub async fn new() -> Result<Init, MyErr> {
        let input = tokio::fs::read_to_string(&Init::path()).await?;
        let mut config: Init = serde_json::from_str(&input)?;
        config.apply_env_overrides();
        if config.authorized_guilds.is_empty() {
            warn!("authorized_guilds is empty in config.json. Bot will auto-leave ALL guilds. Add at least one guild ID to prevent lockout.");
        }
        Ok(config)
    }

    pub fn block_new() -> Result<Init, MyErr> {
        let input = std::fs::read_to_string(&Init::path())?;
        let mut config: Init = serde_json::from_str(&input)?;
        config.apply_env_overrides();
        if config.authorized_guilds.is_empty() {
            warn!("authorized_guilds is empty in config.json. Bot will auto-leave ALL guilds.");
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_load_test() {
        let _ = Init::block_new().expect("Failed to load config.json");
    }
}