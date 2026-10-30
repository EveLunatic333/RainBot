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
    #[serde(default)]
    pub postgress: Postgress,
    pub discord: Discord,
    pub lfg_roles: LfgRoles,
    #[serde(default)]
    pub commands_toggle: HashMap<String, bool>,
    #[serde(default)]
    pub authorized_guilds: Vec<u64>,
    /// Discord user ID that bypasses every admin role check in `execute_guard`.
    ///
    /// Optional on purpose. This was a hardcoded constant, so the bypass could
    /// not be rotated or revoked without recompiling the bot, and its value was
    /// published in the source tree. As an `Option`, omitting the field (or
    /// setting it to `null`) means **no bypass exists at all** and every admin
    /// action must come from one of the configured roles.
    #[serde(default)]
    pub master_commander_id: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Discord {
    pub token: String,
    pub prefix: String,
    pub author_id: u64,
    #[serde(default)]
    pub sightengine_enabled: bool,
}

/// PostgreSQL connection details.
///
/// Every field is `#[serde(default)]` because the values live in `.env`
/// (`POSTGRES_HOST`, `POSTGRES_PORT`, `POSTGRES_DB`, `POSTGRES_USER`,
/// `POSTGRES_PASSWORD`) and are injected by `apply_env_overrides`. Keeping the
/// fields optional lets an older `config.json` that still carries them load fine,
/// while a clean one omits them entirely — so `config.json` no longer discloses
/// the database host, port, name, or user.
#[derive(Debug, Default, Deserialize, Clone, Serialize)]
pub struct Postgress {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
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

    /// Names of the settings that are still empty, so startup can fail with one
    /// actionable message instead of an opaque connection error.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.host.trim().is_empty() {
            out.push("POSTGRES_HOST");
        }
        if self.port == 0 {
            out.push("POSTGRES_PORT");
        }
        if self.database.trim().is_empty() {
            out.push("POSTGRES_DB");
        }
        if self.user.trim().is_empty() {
            out.push("POSTGRES_USER");
        }
        if self.password.trim().is_empty() {
            out.push("POSTGRES_PASSWORD");
        }
        out
    }

    /// Parses `postgres://user:password@host:port/database` into its parts.
    ///
    /// Hand-rolled rather than pulling in a URL crate: the format is fixed and this
    /// stays unit-testable. Returns `None` on anything unexpected so the caller can
    /// fall back to the individual variables instead of crashing.
    pub fn parse_database_url(url: &str) -> Option<Postgress> {
        let rest = url
            .strip_prefix("postgres://")
            .or_else(|| url.strip_prefix("postgresql://"))?;

        let (authority, database) = rest.rsplit_once('/')?;
        let (creds, hostport) = authority.rsplit_once('@')?;

        let (user, password) = match creds.split_once(':') {
            Some((u, p)) => (u.to_string(), urlencoding::decode(p).ok()?.into_owned()),
            None => (creds.to_string(), String::new()),
        };

        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()?),
            None => (hostport.to_string(), 5432),
        };

        // Query parameters (e.g. `?sslmode=require`) are unused here but must not
        // end up inside the database name.
        let database = database.split('?').next()?.to_string();

        if host.is_empty() || database.is_empty() {
            return None;
        }

        Some(Postgress {
            host,
            password,
            port,
            database,
            user,
        })
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

    /// Writes `config.json` **without** the credentials injected from the environment.
    ///
    /// `apply_env_overrides` copies tokens and the DB password into the in-memory
    /// struct. Serializing it directly would move those real secrets into a file whose
    /// purpose is to hold placeholders only — they belong in `.env`.
    pub async fn save(&self) -> Result<(), MyErr> {
        let mut redacted = self.clone();
        if std::env::var("DISCORD_TOKEN").is_ok() {
            redacted.discord.token = String::new();
        }
        if std::env::var("POSTGRES_PASSWORD").is_ok() {
            redacted.postgress.password = String::new();
        }
        if std::env::var("CHATGPT_TOKEN").is_ok() {
            redacted.chat_gpt.token = String::new();
        }

        let content = serde_json::to_string_pretty(&redacted)?;
        Ok(tokio::fs::write(Init::path(), content).await?)
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

        // Connection details are read from the environment so that config.json does
        // not disclose the database host, port, name, or user.
        if let Ok(v) = std::env::var("POSTGRES_HOST") {
            if !v.trim().is_empty() {
                self.postgress.host = v;
            }
        }
        if let Ok(v) = std::env::var("POSTGRES_DB") {
            if !v.trim().is_empty() {
                self.postgress.database = v;
            }
        }
        if let Ok(v) = std::env::var("POSTGRES_USER") {
            if !v.trim().is_empty() {
                self.postgress.user = v;
            }
        }
        if let Ok(v) = std::env::var("POSTGRES_PORT") {
            match v.trim().parse::<u16>() {
                Ok(p) if p > 0 => self.postgress.port = p,
                Ok(_) => warn!("POSTGRES_PORT is 0; ignoring"),
                Err(_) => warn!("POSTGRES_PORT is not a valid port number; ignoring"),
            }
        }

        // Convenience: a single DATABASE_URL wins over the individual pieces.
        if let Ok(url) = std::env::var("DATABASE_URL") {
            if let Some(parsed) = Postgress::parse_database_url(&url) {
                self.postgress = parsed;
            } else {
                warn!("DATABASE_URL could not be parsed; falling back to POSTGRES_* variables");
            }
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

    /// Regression: removing the `postgress` block from `config.json` made startup fail
    /// with "missing field `postgress`", because the field itself was not `#[serde(default)]`
    /// even though all of its members were. The bot must load a config that carries no
    /// database details at all.
    #[test]
    fn config_without_postgress_block_still_loads() {
        let raw = std::fs::read_to_string(Init::path()).expect("config.json");
        let mut value: serde_json::Value = serde_json::from_str(&raw).expect("valid json");

        value
            .as_object_mut()
            .expect("object")
            .remove("postgress");

        let cfg: Init = serde_json::from_value(value)
            .expect("a config without the postgress block must deserialize");
        assert_eq!(cfg.postgress.port, 0, "fields fill from .env at runtime");
    }

    /// Guards a real incident: `persist_board_id` once wrote the in-memory `Init`
    /// back to `config.json` after `apply_env_overrides` had copied the real Discord
    /// token and DB password into it. The live token ended up on disk.
    #[test]
    fn env_credentials_must_not_be_serialized() {
        std::env::set_var("POSTGRES_PASSWORD", "SENTINEL-should-not-reach-disk");
        let mut cfg = Init::block_new().expect("Failed to load config.json");
        cfg.apply_env_overrides();
        assert_eq!(cfg.postgress.password, "SENTINEL-should-not-reach-disk");

        let mut redacted = cfg.clone();
        if std::env::var("POSTGRES_PASSWORD").is_ok() {
            redacted.postgress.password = String::new();
        }
        let json = serde_json::to_string(&redacted).expect("serialize");
        assert!(
            !json.contains("SENTINEL-should-not-reach-disk"),
            "environment credentials must not be serialized into config.json"
        );
        std::env::remove_var("POSTGRES_PASSWORD");
    }

    /// The connection details must survive a round trip through `DATABASE_URL`, so an
    /// operator can supply one variable instead of five.
    #[test]
    fn database_url_is_parsed_into_parts() {
        let p = Postgress::parse_database_url(
            "postgres://rainbot_dev:s3cr3t@127.0.0.1:5444/erupe_devbot",
        )
        .expect("valid url");

        assert_eq!(p.user, "rainbot_dev");
        assert_eq!(p.password, "s3cr3t");
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 5444);
        assert_eq!(p.database, "erupe_devbot");
    }

    #[test]
    fn database_url_tolerates_missing_port_and_query() {
        let p = Postgress::parse_database_url("postgresql://u:p@db.local/erupe?sslmode=require")
            .expect("valid url without port");

        assert_eq!(p.port, 5432, "default PostgreSQL port");
        assert_eq!(p.database, "erupe", "query string must not leak into the name");
    }

    #[test]
    fn percent_encoded_password_is_decoded() {
        let p = Postgress::parse_database_url("postgres://u:p%40ss%2Fword@h:5432/d")
            .expect("valid url");
        assert_eq!(p.password, "p@ss/word");
    }

    #[test]
    fn malformed_database_url_is_rejected() {
        for bad in [
            "",
            "mysql://u:p@h:3306/d",
            "postgres://u:p@h:5432",
            "postgres://nohostpart",
        ] {
            assert!(
                Postgress::parse_database_url(bad).is_none(),
                "should reject: {bad}"
            );
        }
    }

    /// `config.json` no longer carries these values, so a config without them must
    /// still deserialize — the fields are `#[serde(default)]`.
    #[test]
    fn postgress_fields_are_optional_in_json() {
        let p: Postgress = serde_json::from_str("{}").expect("empty object must deserialize");
        assert_eq!(p.port, 0);
        assert!(p.host.is_empty());
        assert_eq!(
            p.missing(),
            vec!["POSTGRES_HOST", "POSTGRES_PORT", "POSTGRES_DB", "POSTGRES_USER", "POSTGRES_PASSWORD"]
        );
    }

    /// The admin bypass used to be a hardcoded constant, which meant it could not be
    /// revoked without a rebuild and its value sat in the published source tree.
    /// It now comes from config, and omitting it must mean "no bypass at all"
    /// rather than "bypass user 0" — a zero default would silently disable the
    /// role checks for nobody, but a careless `unwrap_or_default()` on a real ID
    /// could do the opposite.
    #[test]
    fn master_commander_is_optional_and_defaults_to_no_bypass() {
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(Init::path()).expect("config.json"))
                .expect("valid json");

        value
            .as_object_mut()
            .expect("object")
            .remove("master_commander_id");

        let cfg: Init = serde_json::from_value(value)
            .expect("a config without master_commander_id must deserialize");
        assert_eq!(
            cfg.master_commander_id, None,
            "omitting the field must remove the bypass, never invent one"
        );
    }

    /// An operator must be able to pin the bypass to a specific user from config.
    #[test]
    fn master_commander_is_read_from_config() {
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(Init::path()).expect("config.json"))
                .expect("valid json");

        value
            .as_object_mut()
            .expect("object")
            .insert("master_commander_id".into(), serde_json::json!(123456789012345678_u64));

        let cfg: Init = serde_json::from_value(value).expect("deserialize");
        assert_eq!(cfg.master_commander_id, Some(123456789012345678));
    }
}
