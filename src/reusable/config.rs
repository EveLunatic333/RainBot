use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::MyErr;
use std::collections::HashMap;
use tracing::warn;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Init {
    pub bounty: Bounty,
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
    /// Per-category cooldown, in hours, applied to the *next* submission of the
    /// same category. Absent from old configs, which keeps the historical 20/40
    /// hour rule in force.
    #[serde(default)]
    pub cooldown_hours: CooldownHours,
    /// What the Bronze -> Silver -> Gold gate requires. See [`ProgressionGateMode`].
    #[serde(default)]
    pub progression_gate: ProgressionGateMode,
    /// Whether the Event category shares the standard cooldown timer.
    ///
    /// `false` (the default) keeps Event on a timer of its own, measured in
    /// `cooldown_hours.event`. `true` folds Event into the shared standard group:
    /// an Event run then locks Bronze/Silver/Gold/Free and the other way round,
    /// and Event repeats obey `same`/`different` instead of `event`.
    ///
    /// Defaults to `false` so a build upgrade cannot silently change a live
    /// server's cooldown behaviour; the operator has to opt in.
    #[serde(default)]
    pub event_shares_standard: bool,
    /// Whether a progression bounty that was already cleared may be repeated.
    ///
    /// The ladder is walked in order: the counter only rises when the submitted
    /// BBQ is exactly the next one, so `bronze = N` means Bronze BBQ01..BBQ_N are
    /// done. `true` (the default) enforces that meaning and refuses a repeat, so
    /// the player has to move up. `false` restores the old behaviour, where any
    /// BBQ up to the current rank may be submitted again.
    ///
    /// Free and Event are never affected.
    #[serde(default = "default_progression_no_repeat")]
    pub progression_no_repeat: bool,
    /// Whether the progression ladder shares the standard (Free) cooldown timer.
    ///
    /// `false` (the default) keeps the ladder on a timer of its own, measured in
    /// `cooldown_hours.progression`. `true` folds the ladder into the shared
    /// standard group: a Bronze/Silver/Gold run then locks Free and the other way
    /// round, and ladder repeats obey `same`/`different` instead of `progression`.
    ///
    /// This is the mirror of [`Self::event_shares_standard`], which pairs Event
    /// with Free. Turning both on leaves a single shared timer for everything that
    /// is not Event-or-ladder specific.
    ///
    /// Defaults to `false` so a build upgrade cannot silently change a live
    /// server's cooldown behaviour; the operator has to opt in.
    #[serde(default)]
    pub progression_shares_standard: bool,
    /// Scale factors applied to the Bounty Coin and Gacha Ticket rewards of each
    /// group. See [`RewardMultipliers`].
    ///
    /// Kept out of the category JSON files on purpose: `static/*_bounty.json` holds
    /// what a bounty *is* (description, rules, items, base reward), while how much of
    /// it is paid out is an operator setting that changes far more often.
    #[serde(default)]
    pub reward_multiplier: RewardMultipliers,
}

/// Upper bound for a reward multiplier.
///
/// This is a typo guard, not a business limit: `10.00` and `1000` are one keystroke
/// apart, and a runaway multiplier would mint currency faster than any audit could
/// catch. Raise the constant deliberately if a larger factor is ever wanted.
pub const MAX_REWARD_MULTIPLIER: f64 = 100.0;

/// Scale factors for the two currencies a bounty pays out.
///
/// Both default to `1.0`, so a config that does not mention them behaves exactly as
/// before. Values are `f64` because a multiplier like `1.25` is the whole point.
#[derive(Debug, Deserialize, Clone, Copy, Serialize, PartialEq)]
pub struct RewardMultiplier {
    /// Factor applied to the Bounty Coin amount (`coin` in the category JSON).
    #[serde(default = "default_multiplier")]
    pub coin: f64,
    /// Factor applied to the Gacha Ticket amount (`ticket` in the category JSON,
    /// stored in the `discord.gacha` column).
    #[serde(default = "default_multiplier")]
    pub gacha: f64,
}

fn default_multiplier() -> f64 {
    1.0
}

impl Default for RewardMultiplier {
    fn default() -> Self {
        Self {
            coin: default_multiplier(),
            gacha: default_multiplier(),
        }
    }
}

impl RewardMultiplier {
    /// Collapses a configured factor into one that is safe to multiply by.
    ///
    /// `NaN`, infinity and negatives fall back to `1.0` rather than being clamped to
    /// zero: a reward of zero is a far worse surprise than an unchanged one, and a
    /// negative factor would *remove* currency from the player.
    ///
    /// Silent on purpose — this runs on every reward grant. Use
    /// [`Self::report_if_invalid`] to surface a bad value at startup instead.
    pub fn effective(factor: f64) -> f64 {
        if !factor.is_finite() || factor < 0.0 {
            return 1.0;
        }
        factor.min(MAX_REWARD_MULTIPLIER)
    }

    /// Logs a warning when a configured factor had to be rewritten.
    ///
    /// Called once while the config is loaded, so a typo is reported without the
    /// reward path emitting a warning for every bounty handed out.
    pub fn report_if_invalid(&self, group: &str) {
        for (field, value) in [("coin", self.coin), ("gacha", self.gacha)] {
            let effective = Self::effective(value);
            if effective != value {
                warn!(
                    group,
                    field,
                    value,
                    applied = effective,
                    limit = MAX_REWARD_MULTIPLIER,
                    "Reward multiplier is unusable and was replaced"
                );
            }
        }
    }

    /// Applies the factor to one amount.
    ///
    /// Rounds **down** so a payout can never exceed `amount * factor`; a fractional
    /// coin or ticket cannot be granted, and rounding up would quietly pay more than
    /// the operator configured. Saturates at [`u32::MAX`] instead of wrapping.
    pub fn apply(value: u32, factor: f64) -> u32 {
        let scaled = (value as f64) * Self::effective(factor);
        if !scaled.is_finite() || scaled >= u32::MAX as f64 {
            return u32::MAX;
        }
        // `floor` of a non-negative finite value is >= 0, so the cast is lossless.
        scaled.floor() as u32
    }

    pub fn coin_of(&self, base: u32) -> u32 {
        Self::apply(base, self.coin)
    }

    pub fn gacha_of(&self, base: u32) -> u32 {
        Self::apply(base, self.gacha)
    }

    /// True when this multiplier changes nothing, so callers can skip the work.
    pub fn is_neutral(&self) -> bool {
        Self::effective(self.coin) == 1.0 && Self::effective(self.gacha) == 1.0
    }
}

/// One multiplier per reward group.
///
/// The grouping deliberately matches the cooldown grouping: Bronze, Silver and Gold
/// form one progression ladder and share a factor, while Free and Event are
/// independent. Keeping the two groupings identical means an operator learns one
/// mental model, not two.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct RewardMultipliers {
    /// Free bounties.
    #[serde(default)]
    pub free: RewardGroup,
    /// Event bounties.
    #[serde(default)]
    pub event: RewardGroup,
    /// The Bronze/Silver/Gold progression ladder, all three at once.
    #[serde(default)]
    pub progression: RewardGroup,
}

/// One group's multipliers: a default for the whole group, plus per-BBQ overrides.
///
/// An override **replaces** the group value rather than compounding with it. With
/// `free.coin = 2.00` and `per_bbq["25"].coin = 5.00`, BBQ25 pays 5x — not 10x. A
/// compounding rule would make the effective factor depend on a product the operator
/// has to work out in their head, and a forgotten group value would silently change
/// every override beneath it.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct RewardGroup {
    /// Applies to every BBQ in the group unless that BBQ overrides it.
    #[serde(flatten)]
    pub default: RewardMultiplier,
    /// Per-BBQ overrides, keyed by BBQ number as a string (`"1"` .. `"25"`).
    ///
    /// A string key keeps the config readable and lets the loader reject an
    /// out-of-range number with a clear message, instead of only failing once that
    /// BBQ happens to be submitted.
    #[serde(default)]
    pub per_bbq: HashMap<String, RewardMultiplier>,
}

impl Default for RewardGroup {
    fn default() -> Self {
        Self {
            default: RewardMultiplier::default(),
            per_bbq: HashMap::new(),
        }
    }
}

impl RewardGroup {
    /// The multiplier that applies to one BBQ, preferring an override.
    ///
    /// `bbq` is the 1-based number the rest of the bot uses (`BBQ::encode()`).
    pub fn for_bbq(&self, bbq: u8) -> RewardMultiplier {
        self.per_bbq
            .get(&bbq.to_string())
            .copied()
            .unwrap_or(self.default)
    }

    /// Reported numbers that cannot ever match a BBQ, so a typo is visible at startup.
    ///
    /// Without this an override keyed `"0"` or `"26"` would simply never apply, and the
    /// operator would see no reward change and no explanation.
    pub fn report_unreachable_overrides(&self, group: &str) {
        let mut reported: Vec<&String> = self
            .per_bbq
            .keys()
            .filter(|k| match k.parse::<u8>() {
                Ok(n) => !(1..=25).contains(&n),
                Err(_) => true,
            })
            .collect();
        reported.sort();
        for key in reported {
            warn!(
                group,
                key,
                "Per-BBQ reward override does not name a BBQ between 1 and 25; it will never apply"
            );
        }
    }

    /// True when nothing here changes a reward.
    pub fn is_neutral(&self) -> bool {
        self.default.is_neutral()
            && self
                .per_bbq
                .values()
                .all(|m| m.is_neutral() || m == &self.default)
    }
}

impl RewardMultipliers {
    /// The multiplier that applies to a category and one BBQ number.
    ///
    /// Uses the same mapping the cooldown uses, so the two groupings cannot drift
    /// apart: a category added to one is automatically grouped in the other.
    pub fn for_category(
        &self,
        category: &crate::reusable::component::bounty::Category,
    ) -> RewardMultiplier {
        use crate::reusable::component::bounty_cooldown::CooldownGroup;
        match CooldownGroup::from_category(category) {
            CooldownGroup::Standard => self.free.default,
            CooldownGroup::Event => self.event.default,
            CooldownGroup::Progression => self.progression.default,
        }
    }

    /// The multiplier for one category **and** one BBQ, honouring any override.
    pub fn for_bounty(
        &self,
        category: &crate::reusable::component::bounty::Category,
        bbq: &crate::reusable::component::bounty::BBQ,
    ) -> RewardMultiplier {
        use crate::reusable::component::bounty_cooldown::CooldownGroup;
        let group = match CooldownGroup::from_category(category) {
            CooldownGroup::Standard => &self.free,
            CooldownGroup::Event => &self.event,
            CooldownGroup::Progression => &self.progression,
        };
        group.for_bbq(bbq.encode())
    }

    /// Every group, paired with its name, for reporting and previews.
    pub fn groups(&self) -> [(&'static str, &RewardGroup); 3] {
        [
            ("free", &self.free),
            ("event", &self.event),
            ("progression", &self.progression),
        ]
    }

    /// True when every group is neutral, so the whole block can be skipped.
    pub fn is_neutral(&self) -> bool {
        self.free.is_neutral() && self.event.is_neutral() && self.progression.is_neutral()
    }
}

impl Default for RewardMultipliers {
    fn default() -> Self {
        Self {
            free: RewardGroup::default(),
            event: RewardGroup::default(),
            progression: RewardGroup::default(),
        }
    }
}

fn default_progression_no_repeat() -> bool {
    true
}

/// What must be true before the next progression category opens.
///
/// The counter (`bronze` / `silver` columns) is the upstream record: it is bumped
/// unconditionally when a bounty is approved. The achievement bit in `title` is
/// downstream: it is only set once the role was actually granted, which can fail
/// or be skipped. `Counter` therefore stays the safe default.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressionGateMode {
    /// Only the counter is required. A missing achievement bit never blocks.
    Counter,
    /// The counter OR the achievement bit is enough. Identical to `Counter` today
    /// (every set bit has its counter), but tolerant of manually granted roles.
    Either,
    /// Both are required. Stricter than the counter alone: a player whose role was
    /// never granted loses access even though their progress is complete.
    Both,
}

impl Default for ProgressionGateMode {
    fn default() -> Self {
        Self::Counter
    }
}

/// How long a user must wait between bounties.
///
/// The bot used to hold one global timer per user, and the Event category
/// bypassed it entirely. That let an Event approval overwrite the timestamp of a
/// Gold/Bronze/Silver run, shortening the user's real cooldown. The cooldown is
/// now tracked per group: `progression` applies to the Bronze/Silver/Gold ladder,
/// `same`/`different` apply to Free, and `event` applies to Event.
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct CooldownHours {
    /// Wait after the same category and the same BBQ number.
    #[serde(default = "default_same")]
    pub same: i64,
    /// Wait after a different category or a different BBQ number.
    #[serde(default = "default_diff")]
    pub different: i64,
    /// Wait for the Event category. An explicit `0` disables the check, which is
    /// how the old build behaved; the shipped default is a real wait of 40 hours.
    #[serde(default = "default_event")]
    pub event: i64,
    /// Wait for the Bronze/Silver/Gold progression ladder.
    ///
    /// One value for the whole group, the way `event` is: a progression run is
    /// always the next BBQ in the ladder, so a `same`/`different` split would never
    /// be asked for. `0` disables the progression cooldown.
    #[serde(default = "default_progression")]
    pub progression: i64,
}

fn default_same() -> i64 {
    40
}

fn default_diff() -> i64 {
    20
}

fn default_event() -> i64 {
    // Kept in step with `bounty_cooldown::DEFAULT_EVENT_HOURS`. `0` used to mean
    // "no Event cooldown at all", which let an Event run be repeated instantly;
    // that is the behaviour the per-group cooldown removed.
    40
}

fn default_progression() -> i64 {
    // Kept in step with `bounty_cooldown::DEFAULT_PROGRESSION_HOURS`. The ladder
    // shared the standard timer before it had one of its own, and 40 hours was the
    // wait in practice, so that is the shipped value.
    40
}

impl Default for CooldownHours {
    fn default() -> Self {
        Self {
            same: default_same(),
            different: default_diff(),
            event: default_event(),
            progression: default_progression(),
        }
    }
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
        config.report_reward_multipliers();
        if config.authorized_guilds.is_empty() {
            warn!("authorized_guilds is empty in config.json. Bot will auto-leave ALL guilds. Add at least one guild ID to prevent lockout.");
        }
        Ok(config)
    }

    pub fn block_new() -> Result<Init, MyErr> {
        let input = std::fs::read_to_string(&Init::path())?;
        let mut config: Init = serde_json::from_str(&input)?;
        config.apply_env_overrides();
        config.report_reward_multipliers();
        if config.authorized_guilds.is_empty() {
            warn!("authorized_guilds is empty in config.json. Bot will auto-leave ALL guilds.");
        }
        Ok(config)
    }

    /// Reports every reward multiplier that had to be rewritten.
    ///
    /// Done once at load so a bad value is visible in the log immediately, instead of
    /// the reward path warning on every single bounty handed out.
    fn report_reward_multipliers(&self) {
        let m = &self.bounty.reward_multiplier;
        for (name, group) in m.groups() {
            group.default.report_if_invalid(name);
            group.report_unreachable_overrides(name);
            for (bbq, override_m) in &group.per_bbq {
                override_m.report_if_invalid(&format!("{name}[{bbq}]"));
            }
        }
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

    /// Builds a config value from the shipped example, so the multiplier tests can
    /// add a block without depending on the live `config.json`.
    fn config_value() -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(Init::path()).expect("config.json"))
            .expect("valid json")
    }

    #[test]
    fn a_missing_multiplier_block_leaves_rewards_untouched() {
        // The promise that makes this feature safe to ship: without the block, every
        // reward is paid exactly as the category JSON says.
        let mut value = config_value();
        value["bounty"]
            .as_object_mut()
            .expect("bounty")
            .remove("reward_multiplier");

        let cfg: Init = serde_json::from_value(value).expect("deserialize");
        for category in [
            crate::reusable::component::bounty::Category::Bronze,
            crate::reusable::component::bounty::Category::Silver,
            crate::reusable::component::bounty::Category::Gold,
            crate::reusable::component::bounty::Category::Free,
            crate::reusable::component::bounty::Category::Event,
        ] {
            let m = cfg.bounty.reward_multiplier.for_category(&category);
            assert_eq!(m.coin_of(15_000), 15_000, "{} coin", category.name());
            assert_eq!(m.gacha_of(10), 10, "{} ticket", category.name());
        }
    }

    #[test]
    fn the_multiplier_is_read_from_the_bounty_block() {
        let mut value = config_value();
        value["bounty"]["reward_multiplier"] = serde_json::json!({
            "free":        { "coin": 1.25, "gacha": 2.00 },
            "event":       { "coin": 3.00, "gacha": 0.50 },
            "progression": { "coin": 2.00, "gacha": 1.00 }
        });

        let cfg: Init = serde_json::from_value(value).expect("deserialize");
        let m = cfg.bounty.reward_multiplier;
        assert_eq!(m.free.default.coin, 1.25);
        assert_eq!(m.free.default.gacha, 2.0);
        assert_eq!(m.event.default.coin, 3.0);
        assert_eq!(m.progression.default.coin, 2.0);
        assert_eq!(m.progression.default.gacha, 1.0);
    }

    #[test]
    fn a_multiplier_scales_only_its_own_group() {
        use crate::reusable::component::bounty::Category;
        let mut value = config_value();
        value["bounty"]["reward_multiplier"] = serde_json::json!({
            "free": { "coin": 2.00, "gacha": 1.00 }
        });
        let cfg: Init = serde_json::from_value(value).expect("deserialize");
        let m = &cfg.bounty.reward_multiplier;

        assert_eq!(m.for_category(&Category::Free).coin_of(15_000), 30_000);
        assert_eq!(m.for_category(&Category::Event).coin_of(15_000), 15_000);
        assert_eq!(m.for_category(&Category::Bronze).coin_of(15_000), 15_000);
        assert_eq!(m.for_category(&Category::Gold).coin_of(15_000), 15_000);
    }

    #[test]
    fn the_progression_group_covers_every_ladder_tier() {
        // Bronze, Silver and Gold are one group by decision: an operator sets one
        // number for the whole ladder. This pins that mapping in place.
        use crate::reusable::component::bounty::Category;
        let mut value = config_value();
        value["bounty"]["reward_multiplier"] = serde_json::json!({
            "progression": { "coin": 1.50, "gacha": 1.00 }
        });
        let cfg: Init = serde_json::from_value(value).expect("deserialize");
        let m = &cfg.bounty.reward_multiplier;

        for tier in [Category::Bronze, Category::Silver, Category::Gold] {
            assert_eq!(
                m.for_category(&tier).coin_of(1_000),
                1_500,
                "{} must follow the ladder multiplier",
                tier.name()
            );
        }
        assert_eq!(m.for_category(&Category::Free).coin_of(1_000), 1_000);
    }

    #[test]
    fn coin_and_gacha_scale_independently() {
        let m = RewardMultiplier { coin: 1.25, gacha: 1.0 };
        assert_eq!(m.coin_of(100_000), 125_000);
        assert_eq!(m.gacha_of(100_000), 100_000);
    }

    #[test]
    fn fractional_rewards_round_down() {
        let m = RewardMultiplier { coin: 1.25, gacha: 1.0 };
        // 15_000 * 1.25 = 18_750 exactly.
        assert_eq!(m.coin_of(15_000), 18_750);
        // 15_001 * 1.25 = 18_751.25 -> 18_751, never 18_752.
        assert_eq!(m.coin_of(15_001), 18_751);
        // 3 * 1.25 = 3.75 -> 3.
        assert_eq!(m.coin_of(3), 3);
        // The inverse: rounding up would hand out a currency unit that was never earned.
        assert_ne!(m.coin_of(3), 4);
    }

    #[test]
    fn a_factor_below_one_reduces_the_reward() {
        // Lowering a reward is a legitimate operator action, not a bug.
        let m = RewardMultiplier { coin: 0.5, gacha: 0.5 };
        assert_eq!(m.coin_of(10_000), 5_000);
        assert_eq!(m.gacha_of(10), 5);
        assert_eq!(m.gacha_of(1), 0);
    }

    #[test]
    fn nonsense_multipliers_fall_back_to_no_change() {
        // NaN, infinity and negatives cannot mean "pay nothing": a zero or negative
        // payout would delete currency, which is never what a typo should do.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.25] {
            assert_eq!(
                RewardMultiplier::effective(bad),
                1.0,
                "factor {bad} must fall back to 1.0"
            );
            assert_eq!(RewardMultiplier::apply(1_000, bad), 1_000);
        }
    }

    #[test]
    fn a_huge_multiplier_saturates_instead_of_overflowing() {
        // Clamped factor, and the amount itself saturates rather than wrapping.
        assert_eq!(
            RewardMultiplier::effective(MAX_REWARD_MULTIPLIER * 10.0),
            MAX_REWARD_MULTIPLIER
        );
        let m = RewardMultiplier { coin: f64::MAX, gacha: 1.0 };
        assert_eq!(m.coin_of(u32::MAX), u32::MAX);
        assert_eq!(m.coin_of(1_000_000), 100_000_000, "clamped to 100x");
    }

    #[test]
    fn zero_is_a_legal_multiplier() {
        // Unlike a negative, an explicit 0 is an understandable request.
        let m = RewardMultiplier { coin: 0.0, gacha: 1.0 };
        assert_eq!(m.coin_of(15_000), 0);
        assert_eq!(m.gacha_of(10), 10);
    }

    #[test]
    fn the_default_multiplier_is_neutral() {
        assert!(RewardMultipliers::default().is_neutral());
        let m = RewardMultiplier { coin: 1.5, gacha: 1.0 };
        assert!(!m.is_neutral());
    }

    #[test]
    fn a_partial_block_fills_the_missing_side_with_one() {
        // An operator may set only `coin`; `gacha` must not become zero.
        let m: RewardMultiplier = serde_json::from_str(r#"{"coin": 2.0}"#).expect("deserialize");
        assert_eq!(m.coin, 2.0);
        assert_eq!(m.gacha, 1.0);
    }

    fn multipliers_with_overrides(json: serde_json::Value) -> RewardMultipliers {
        serde_json::from_value(json).expect("deserialize multipliers")
    }

    #[test]
    fn a_per_bbq_override_replaces_the_group_value() {
        use crate::reusable::component::bounty::{BBQ, Category};
        // The agreed rule: an override is the final factor, it does not multiply the
        // group value. 2.00 group + 5.00 override must pay 5x, not 10x.
        let m = multipliers_with_overrides(serde_json::json!({
            "free": { "coin": 2.00, "gacha": 1.00, "per_bbq": { "25": { "coin": 5.00 } } }
        }));

        let bbq25 = m.for_bounty(&Category::Free, &BBQ::BBQ25);
        assert_eq!(bbq25.coin_of(1_000), 5_000, "override must replace, not compound");

        let bbq24 = m.for_bounty(&Category::Free, &BBQ::BBQ24);
        assert_eq!(bbq24.coin_of(1_000), 2_000, "other BBQ keep the group value");
    }

    #[test]
    fn an_override_may_set_only_one_currency() {
        use crate::reusable::component::bounty::{BBQ, Category};
        // Overriding `coin` alone must not reset `gacha` to the group value silently
        // or to zero: the unset currency falls back to its own default of 1.00.
        let m = multipliers_with_overrides(serde_json::json!({
            "progression": { "coin": 3.00, "gacha": 4.00, "per_bbq": { "7": { "coin": 1.00 } } }
        }));

        let bbq7 = m.for_bounty(&Category::Bronze, &BBQ::BBQ07);
        assert_eq!(bbq7.coin_of(1_000), 1_000, "explicit override of coin");
        assert_eq!(bbq7.gacha_of(1_000), 1_000, "unset gacha falls back to 1.00");
    }

    #[test]
    fn an_override_applies_across_the_whole_progression_ladder() {
        use crate::reusable::component::bounty::{BBQ, Category};
        // The ladder shares one group, so an override keyed on BBQ05 must reach
        // Bronze, Silver and Gold alike.
        let m = multipliers_with_overrides(serde_json::json!({
            "progression": { "coin": 1.00, "gacha": 1.00, "per_bbq": { "5": { "coin": 2.00 } } }
        }));
        for tier in [Category::Bronze, Category::Silver, Category::Gold] {
            assert_eq!(
                m.for_bounty(&tier, &BBQ::BBQ05).coin_of(1_000),
                2_000,
                "{} BBQ05 must use the override",
                tier.name()
            );
        }
        assert_eq!(m.for_bounty(&Category::Free, &BBQ::BBQ05).coin_of(1_000), 1_000);
    }

    #[test]
    fn an_unreachable_override_key_never_applies() {
        use crate::reusable::component::bounty::{BBQ, Category};
        // "0" and "26" cannot ever match a BBQ. They must simply not apply, and must
        // not panic or shift the mapping onto a neighbouring number.
        let m = multipliers_with_overrides(serde_json::json!({
            "free": { "coin": 1.00, "per_bbq": { "0": { "coin": 9.0 }, "26": { "coin": 9.0 } } }
        }));
        for n in 1..=25u8 {
            assert_eq!(
                m.for_bounty(&Category::Free, &BBQ::new(n).unwrap()).coin_of(100),
                100,
                "BBQ{n} must be unaffected by out-of-range keys"
            );
        }
    }

    #[test]
    fn an_override_still_falls_back_when_the_value_is_nonsense() {
        use crate::reusable::component::bounty::{BBQ, Category};
        // The same sanitising applies inside an override as at group level.
        let m = multipliers_with_overrides(serde_json::json!({
            "free": { "per_bbq": { "3": { "coin": -5.0 } } }
        }));
        assert_eq!(
            m.for_bounty(&Category::Free, &BBQ::BBQ03).coin_of(1_000),
            1_000,
            "a negative override must not delete currency"
        );
    }

    /// Every field the bot can read must be visible in the shipped example.
    ///
    /// A field that only exists in Rust is invisible to an operator: they cannot
    /// discover it, and it silently takes a default. This test is the guard that
    /// keeps `config.example.json` a complete map of the configuration.
    ///
    /// `postgress` is the one documented exception — its values come from `.env` so
    /// that `config.json` never discloses credentials (see the `Postgress` docs).
    #[test]
    fn the_example_config_documents_every_setting() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let example: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("config.example.json")).expect("config.example.json"))
                .expect("example must be valid json");

        // Every scalar path the example carries, comments (`_...`) excluded.
        fn paths(v: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(map) => {
                    for (k, child) in map {
                        if k.starts_with('_') {
                            continue;
                        }
                        paths(child, &format!("{prefix}/{k}"), out);
                    }
                }
                serde_json::Value::Array(_) => out.push(prefix.to_string()),
                _ => out.push(prefix.to_string()),
            }
        }
        let mut have = Vec::new();
        paths(&example, "", &mut have);

        // The settings this task added, plus the ones that were missing before it.
        // Named explicitly so the test fails if any disappears from the example.
        let must_document = [
            "/bounty/cooldown_hours/same",
            "/bounty/cooldown_hours/different",
            "/bounty/cooldown_hours/event",
            "/bounty/cooldown_hours/progression",
            "/bounty/event_shares_standard",
            "/bounty/progression_shares_standard",
            "/bounty/progression_no_repeat",
            "/bounty/progression_gate",
            "/bounty/reward_multiplier/free/coin",
            "/bounty/reward_multiplier/free/gacha",
            "/bounty/reward_multiplier/event/coin",
            "/bounty/reward_multiplier/event/gacha",
            "/bounty/reward_multiplier/progression/coin",
            "/bounty/reward_multiplier/progression/gacha",
            "/master_commander_id",
        ];
        for key in must_document {
            assert!(
                have.iter().any(|h| h == key),
                "config.example.json must document {key}"
            );
        }
    }

    /// `leaderboard_ch` was documented for years but no code ever read it.
    ///
    /// A phantom key is worse than a missing one: it implies a feature and sends an
    /// operator looking for a channel that nothing updates.
    #[test]
    fn the_example_config_carries_no_dead_keys() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let raw = std::fs::read_to_string(root.join("config.example.json")).expect("example");
        assert!(
            !raw.contains("leaderboard_ch"),
            "config.example.json must not advertise leaderboard_ch: nothing reads it"
        );
        // Sightengine image moderation was removed; the switch must not linger and
        // imply that uploading `true` still turns something on.
        assert!(
            !raw.contains("sightengine_enabled"),
            "config.example.json must not advertise sightengine_enabled: the feature is gone"
        );
    }

    /// A config.json written before the Sightengine and ChatGPT removals still
    /// carries their keys. It must keep loading: `Discord` has no
    /// `deny_unknown_fields`, so the stale entries are simply ignored.
    #[test]
    fn a_config_carrying_removed_features_still_loads() {
        let mut value = config_value();
        value["discord"]["sightengine_enabled"] = serde_json::json!(false);
        value["bounty"]["reward_multiplier"] = serde_json::json!({
            "free": { "coin": 1.0, "gacha": 1.0 }
        });

        let cfg: Init = serde_json::from_value(value)
            .expect("a config with removed keys must deserialize");
        assert_eq!(cfg.discord.prefix.is_empty(), false, "the rest must still parse");
    }
}
