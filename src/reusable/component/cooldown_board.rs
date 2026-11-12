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

/// Copies the adopted board id into `config.json`.
///
/// The override in `static/cooldown_board.json` wins at runtime, but it is a
/// runtime file: it is not in the repository and can be lost when the folder is
/// replaced or the bot moves host. `config.json` is the durable record, so when
/// the two disagree the adopted value is written back to it.
///
/// Only the `bounty.cooldown_msg` key is touched. The file is read, that one
/// field is replaced, and the result is written through a temp file and a rename
/// so an interrupted write cannot truncate the configuration.
///
/// Returns the ids that were synchronised, or `None` when they already agree.
pub async fn sync_into_config(channel: u64, configured: u64) -> Result<Option<Board>, crate::MyErr> {
    let Some(board) = resolve_with(channel, configured, BoardState::load().await) else {
        return Ok(None);
    };
    if board.message == configured {
        return Ok(None);
    }

    let path = std::path::Path::new(".").join("config.json");
    let raw = tokio::fs::read_to_string(&path).await?;
    let mut doc: serde_json::Value = serde_json::from_str(&raw)?;

    let bounty = doc
        .get_mut("bounty")
        .and_then(|b| b.as_object_mut())
        .ok_or_else(|| crate::MyErr::Custom("config.json has no bounty block".to_owned()))?;
    bounty.insert(
        "cooldown_msg".to_owned(),
        serde_json::Value::from(board.message),
    );

    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(&doc)?;
    tokio::fs::write(&tmp, body.as_bytes()).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(Some(board))
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

    /// The decision `sync_into_config` makes, separated from the file writes so
    /// the "do we need to write at all" rule can be tested offline.
    fn needs_sync(channel: u64, configured: u64, state: Option<BoardState>) -> Option<u64> {
        let board = resolve_with(channel, configured, state)?;
        if board.message == configured {
            None
        } else {
            Some(board.message)
        }
    }

    #[test]
    fn an_adopted_board_that_differs_from_config_needs_syncing() {
        // The live case: the operator adopted a fresh board, config still holds
        // the old id, so the durable record has to be corrected.
        assert_eq!(needs_sync(10, 20, state(10, 99)), Some(99));
    }

    #[test]
    fn an_adopted_board_that_already_matches_config_needs_no_sync() {
        // Rewriting config.json on every start would be pointless churn.
        assert_eq!(needs_sync(10, 20, state(10, 20)), None);
    }

    #[test]
    fn no_override_leaves_config_alone() {
        assert_eq!(needs_sync(10, 20, None), None);
    }

    #[test]
    fn nothing_configured_needs_no_sync() {
        assert_eq!(needs_sync(10, 0, None), None);
    }

    #[test]
    fn a_zeroed_override_does_not_overwrite_a_good_config() {
        // A corrupt override must not blank the durable record.
        assert_eq!(needs_sync(10, 20, state(10, 0)), None);
    }

    #[test]
    fn the_override_round_trips_through_json() {
        let s = BoardState { channel: 1, message: 2 };
        let raw = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<BoardState>(&raw).unwrap(), s);
    }
}
