//! Where the cooldown board lives.
//!
//! The board is a fixed message that gets edited in place, so its id has to be
//! remembered. Two places can hold it:
//!
//! 1. `config.json` under `bounty.cooldown_msg`: the operator-facing setting.
//! 2. `static/cooldown_board.json`: an override written by `/cooldown adopt`.
//!
//! The override exists because `Init` is loaded once at startup and never
//! reloaded. Writing `config.json` alone would not change the board until the
//! bot restarted, so adopting a freshly spawned board has to be recorded
//! somewhere the running process reads on each update. The override wins when it
//! is present, so adopting works immediately; the config value stays as the
//! fallback for a deployment that sets it by hand.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// A resolved board location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board {
    pub channel: u64,
    pub message: u64,
}

/// The on-disk override.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardState {
    pub channel: u64,
    pub message: u64,
}

impl BoardState {
    pub fn path() -> PathBuf {
        std::path::Path::new(".")
            .join("static")
            .join("cooldown_board.json")
    }

    /// Reads the override, if one was ever written.
    ///
    /// A missing file is not an error: it only means nobody has adopted a board
    /// yet, and the config value applies.
    pub async fn load() -> Option<Self> {
        let raw = tokio::fs::read_to_string(Self::path()).await.ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Writes the override atomically.
    ///
    /// The temporary file is written first and then renamed, so an interrupted
    /// write cannot leave a half-written file that would drop the board id.
    pub async fn save(&self) -> Result<(), crate::MyErr> {
        let path = Self::path();
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&tmp, body.as_bytes()).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }
}

/// The board location in force, or `None` when none is configured.
///
/// The override is preferred over the config value so an adopted board takes
/// effect without a restart. Either way the channel is the configured
/// `cooldown_ch`, because that is where the board belongs.
pub async fn resolve(init: &crate::Init) -> Option<Board> {
    resolve_with(init.bounty.cooldown_ch, init.bounty.cooldown_msg, BoardState::load().await)
}

/// The decision itself, with the file read passed in so it can be tested.
pub fn resolve_with(channel: u64, configured: u64, state: Option<BoardState>) -> Option<Board> {
    // An adopted board is used when it was recorded and is non-zero.
    if let Some(state) = state {
        if state.message != 0 {
            let channel = if state.channel != 0 { state.channel } else { channel };
            return Some(Board {
                channel,
                message: state.message,
            });
        }
    }
    // `cooldown_msg: 0` means no board has been spawned by hand.
    if configured == 0 {
        return None;
    }
    Some(Board {
        channel,
        message: configured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(channel: u64, message: u64) -> Option<BoardState> {
        Some(BoardState { channel, message })
    }

    #[test]
    fn the_config_value_is_used_when_nothing_was_adopted() {
        assert_eq!(
            resolve_with(10, 20, None),
            Some(Board { channel: 10, message: 20 })
        );
    }

    #[test]
    fn an_adopted_board_overrides_the_config_value() {
        // This is what makes `/cooldown adopt` take effect without a restart.
        assert_eq!(
            resolve_with(10, 20, state(10, 99)),
            Some(Board { channel: 10, message: 99 })
        );
    }

    #[test]
    fn an_unconfigured_board_resolves_to_nothing() {
        // `cooldown_msg: 0` and no adoption: there is no board to edit, and the
        // caller must not invent a /channels/<g>/<c>/0 link.
        assert_eq!(resolve_with(10, 0, None), None);
    }

    #[test]
    fn a_zero_message_in_the_override_is_ignored() {
        // A corrupt or cleared override must not shadow a good config value.
        assert_eq!(
            resolve_with(10, 20, state(10, 0)),
            Some(Board { channel: 10, message: 20 })
        );
    }

    #[test]
    fn a_zero_channel_in_the_override_falls_back_to_the_configured_channel() {
        assert_eq!(
            resolve_with(10, 0, state(0, 99)),
            Some(Board { channel: 10, message: 99 })
        );
    }

    #[test]
    fn the_override_round_trips_through_json() {
        let s = BoardState { channel: 1, message: 2 };
        let raw = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<BoardState>(&raw).unwrap(), s);
    }
}
