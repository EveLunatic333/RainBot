//! Pure cooldown evaluation and progression gating for bounty submissions.
//!
//! The rule used to live twice: once as a pre-check in `commands::bounty::submit`
//! and once more inside `Hunter::new`. Two copies drift apart, and neither could be
//! tested without Discord and a database. This module holds the rule with no I/O at
//! all — no clock, no HTTP, no SQL — so the behaviour can be pinned by unit tests
//! and every caller consults the same function.
//!
//! Cooldown is tracked **per group**, not per category:
//!
//! | Group    | Categories                    | Wait                             |
//! |----------|-------------------------------|----------------------------------|
//! | Standard | Bronze, Silver, Gold, Free    | `same` / `different` from config |
//! | Event    | Event                         | `event` from config              |
//!
//! One row per group means a Gold run locks Bronze, Silver and Free too. Tracking
//! each of the five categories separately used to let a player chain
//! Gold -> Silver -> Bronze with no wait at all. Free used to have a timer of its
//! own, which let one player take a Bronze/Silver/Gold bounty and a Free bounty
//! back to back; folding it into the same timer caps a player at one standard
//! bounty plus one Event per cooldown window.

use super::bounty::{BBQ, Category};

/// Default wait, in hours, when the same category and BBQ are repeated.
pub const DEFAULT_SAME_HOURS: i64 = 40;

/// Default wait, in hours, when the category or the BBQ differs.
pub const DEFAULT_DIFF_HOURS: i64 = 20;

/// Default wait, in hours, for the Event category.
///
/// This used to be `0`, which disabled the check and reproduced the historical
/// "Event bypasses cooldown" behaviour. That is the bug this module exists to
/// close: a `0` here means an Event approval can be repeated with no wait at all.
/// The default is therefore a real wait, and an operator who genuinely wants no
/// Event cooldown has to configure `event: 0` explicitly.
pub const DEFAULT_EVENT_HOURS: i64 = 20;

/// The cooldown ladder a category belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownGroup {
    /// Bronze, Silver, Gold and Free share one timer.
    Standard,
    /// Event has a timer of its own, because it is not part of the ladder.
    Event,
}

impl CooldownGroup {
    pub fn from_category(category: &Category) -> Self {
        match category {
            Category::Bronze | Category::Silver | Category::Gold | Category::Free => Self::Standard,
            Category::Event => Self::Event,
        }
    }

    /// Stable number used as the database key. Never renumber: existing rows use it.
    ///
    /// `1` used to mean "Free" and is retired, but it is still *decoded* as
    /// `Standard` so a row written by the previous build keeps its meaning even if
    /// the merge migration did not get to run.
    pub fn encode(self) -> i16 {
        match self {
            Self::Standard => 0,
            Self::Event => 2,
        }
    }

    /// Every group, so a migration can enumerate them without hard-coding numbers.
    ///
    /// The backfill builds its SQL from `encode()` values, which keeps the
    /// database keys and this enum from drifting apart.
    pub const ALL: [CooldownGroup; 2] = [Self::Standard, Self::Event];

    pub fn decode(value: i16) -> Option<Self> {
        match value {
            0 => Some(Self::Standard),
            // Retired key for the old Free group, folded into Standard at startup.
            1 => Some(Self::Standard),
            2 => Some(Self::Event),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Event => "Event",
        }
    }
}

/// How long each group must wait, in hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooldownPolicy {
    pub same: i64,
    pub different: i64,
    pub event: i64,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self {
            same: DEFAULT_SAME_HOURS,
            different: DEFAULT_DIFF_HOURS,
            event: DEFAULT_EVENT_HOURS,
        }
    }
}

impl CooldownPolicy {
    /// Wait in hours that applies when `new_category` is submitted after the
    /// recorded run described by `previous_category` / `previous_bbq`.
    pub fn hours_for(
        &self,
        group: CooldownGroup,
        previous_category: &Category,
        previous_bbq: &BBQ,
        new_category: &Category,
        new_bbq: &BBQ,
    ) -> i64 {
        // A negative wait is meaningless; treat it as "no cooldown" rather than
        // letting `history + negative` unlock submissions in the past.
        let hours = match group {
            CooldownGroup::Event => self.event,
            // A repeat means the same category AND the same BBQ. Moving from Gold
            // to Silver counts as a change even when the BBQ number matches.
            _ if previous_category == new_category && previous_bbq == new_bbq => self.same,
            _ => self.different,
        };
        hours.max(0)
    }
}

/// One recorded completion, keyed by the group it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupHistory {
    pub group: CooldownGroup,
    pub category: Category,
    pub bbq: BBQ,
    pub completed_at: i64,
}

/// Outcome of evaluating a user's bounty cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownVerdict {
    /// No cooldown is pending; the submission may proceed.
    Allowed,
    /// A cooldown is still running and expires at the given unix timestamp.
    Blocked { ends_at: i64 },
}

impl CooldownVerdict {
    /// The timestamp a blocked user must wait for, if any.
    pub fn ends_at(self) -> Option<i64> {
        match self {
            Self::Blocked { ends_at } => Some(ends_at),
            _ => None,
        }
    }

    /// Whether the submission may go ahead.
    pub fn is_allowed(self) -> bool {
        !matches!(self, Self::Blocked { .. })
    }
}

/// The most recent recorded completion of `group`.
///
/// Only rows of that group are considered, which is what keeps the two ladders
/// independent: an Event run cannot shorten a Standard wait because they are
/// different rows.
pub fn last_of_group(history: &[GroupHistory], group: CooldownGroup) -> Option<GroupHistory> {
    history
        .iter()
        .filter(|h| h.group == group)
        .max_by_key(|h| h.completed_at)
        .cloned()
}

/// How far into the future a recorded completion may sit before it is treated as
/// corrupt rather than as real data.
///
/// A correct record is never ahead of the clock: it is written with the submission
/// time. Live data nonetheless contains a row stamped in 2035, which would have
/// locked that player out of Event bounties for nine years. Thirty days of slack
/// covers clock drift between the database and the bot while still rejecting
/// values that cannot be genuine.
pub const MAX_FUTURE_SKEW_SECS: i64 = 30 * 24 * 60 * 60;

/// Whether a recorded completion looks like real data.
///
/// A timestamp far in the future cannot come from a real submission, so it is
/// ignored instead of being allowed to create a multi-year cooldown.
pub fn is_plausible(completed_at: i64, now: i64) -> bool {
    completed_at <= now + MAX_FUTURE_SKEW_SECS
}

/// Decide whether a submission may proceed.
///
/// `now` is passed in rather than read from the clock so the caller stays in
/// charge of time and the tests stay deterministic.
pub fn evaluate(
    history: &[GroupHistory],
    new_category: &Category,
    new_bbq: &BBQ,
    policy: &CooldownPolicy,
    now: i64,
) -> CooldownVerdict {
    let group = CooldownGroup::from_category(new_category);
    let Some(previous) = last_of_group(history, group) else {
        // Never completed a bounty of this group, so there is nothing to wait for.
        return CooldownVerdict::Allowed;
    };

    if !is_plausible(previous.completed_at, now) {
        // A completion stamped in the far future is corrupt data. Ignoring it is
        // safer than honouring it: honouring it would block the player for years.
        return CooldownVerdict::Allowed;
    }

    let hours = policy.hours_for(
        group,
        &previous.category,
        &previous.bbq,
        new_category,
        new_bbq,
    );
    if hours == 0 {
        // Configured to have no wait (the default for Event).
        return CooldownVerdict::Allowed;
    }

    let ends_at = previous.completed_at + hours * 60 * 60;

    // Strictly greater: the moment the cooldown ends the user is free to submit
    // again, so a submission landing exactly on the boundary is allowed.
    if ends_at > now {
        CooldownVerdict::Blocked { ends_at }
    } else {
        CooldownVerdict::Allowed
    }
}

/// Counters a player must reach before the next progression category opens.
///
/// The goals come from `bounty_title.json` (the BBQ number in each trigger), so the
/// gate opens exactly when the achievement role is granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressionTier {
    pub bronze_goal: u8,
    pub silver_goal: u8,
}

impl ProgressionTier {
    /// Fallback when `bounty_title.json` cannot be read: the shipped triggers are
    /// `0_21` for Bronze and `1_20` for Silver.
    pub const fn fallback() -> Self {
        Self {
            bronze_goal: 21,
            silver_goal: 20,
        }
    }
}

impl Default for ProgressionTier {
    fn default() -> Self {
        Self::fallback()
    }
}

/// Result of the Bronze -> Silver -> Gold ladder check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressionGate {
    Allowed,
    /// The previous tier is not finished yet.
    Blocked {
        required: Category,
        need: u8,
        have: i32,
    },
}

impl ProgressionGate {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Enforces that Bronze is finished before Silver and Silver before Gold.
///
/// Free and Event are open to everyone: they are not part of the ladder.
///
/// `mode` decides whether the counter, the achievement bit, or both are required.
/// The bit is downstream of the counter (it is only set once the role was granted),
/// so `Counter` is the default; `Both` is the strict variant an operator can opt
/// into from `config.json`.
#[allow(clippy::too_many_arguments)]
pub fn progression_gate(
    bronze: i32,
    silver: i32,
    bronze_achievement: bool,
    silver_achievement: bool,
    new_category: &Category,
    tier: &ProgressionTier,
    mode: super::super::config::ProgressionGateMode,
) -> ProgressionGate {
    use super::super::config::ProgressionGateMode;

    let satisfied = |counter: i32, goal: u8, achievement: bool| -> bool {
        let counter_ok = counter >= goal as i32;
        match mode {
            ProgressionGateMode::Counter => counter_ok,
            // Every set bit also has its counter today, so this matches `Counter`
            // in practice while tolerating manually granted roles.
            ProgressionGateMode::Either => counter_ok || achievement,
            ProgressionGateMode::Both => counter_ok && achievement,
        }
    };

    match new_category {
        Category::Silver => {
            if satisfied(bronze, tier.bronze_goal, bronze_achievement) {
                ProgressionGate::Allowed
            } else {
                ProgressionGate::Blocked {
                    required: Category::Bronze,
                    need: tier.bronze_goal,
                    have: bronze,
                }
            }
        }
        Category::Gold => {
            if satisfied(silver, tier.silver_goal, silver_achievement) {
                ProgressionGate::Allowed
            } else {
                ProgressionGate::Blocked {
                    required: Category::Silver,
                    need: tier.silver_goal,
                    have: silver,
                }
            }
        }
        // Bronze has no prerequisite, and Free/Event are outside the ladder.
        _ => ProgressionGate::Allowed,
    }
}

/// The earliest moment any submission of `category`'s group becomes possible.
///
/// The card shows this instead of a repeat-specific countdown: a player asking
/// "when can I do a bounty again?" is served by the shortest wait, because
/// picking a different BBQ is always allowed sooner than repeating one.
pub fn earliest_reopen(
    history: &[GroupHistory],
    category: &Category,
    policy: &CooldownPolicy,
    now: i64,
) -> CooldownVerdict {
    let group = CooldownGroup::from_category(category);
    let Some(previous) = last_of_group(history, group) else {
        return CooldownVerdict::Allowed;
    };

    // Same guard as `evaluate`: a corrupt future timestamp must not paint a
    // multi-year countdown on the card either.
    if !is_plausible(previous.completed_at, now) {
        return CooldownVerdict::Allowed;
    }

    let hours = match group {
        CooldownGroup::Event => policy.event,
        // Treat the next submission as a change, which is the shortest wait.
        _ => policy.different,
    }
    .max(0);

    if hours == 0 {
        return CooldownVerdict::Allowed;
    }

    let ends_at = previous.completed_at + hours * 60 * 60;
    if ends_at > now {
        CooldownVerdict::Blocked { ends_at }
    } else {
        CooldownVerdict::Allowed
    }
}

/// Builds the policy from `config.json`, so the waits are operator settings.
impl From<&super::super::config::CooldownHours> for CooldownPolicy {
    fn from(c: &super::super::config::CooldownHours) -> Self {
        Self {
            same: c.same,
            different: c.different,
            event: c.event,
        }
    }
}

/// Turns raw database rows into the history the evaluator understands.
///
/// Rows that do not decode (an unknown group id, category or BBQ) are skipped
/// rather than failing the whole check: a cooldown is a guard, not a reason to
/// block every submission.
pub fn history_from_rows(rows: &[(i16, i16, i16, i64)]) -> Vec<GroupHistory> {
    let mut out = Vec::with_capacity(rows.len());
    for (group, category, bbq, completed_at) in rows {
        let Some(group) = CooldownGroup::decode(*group) else {
            continue;
        };
        let Some(cat) = u8::try_from(*category).ok().and_then(|v| Category::new(v).ok()) else {
            continue;
        };
        let Some(b) = u8::try_from(*bbq).ok().and_then(|v| BBQ::new(v).ok()) else {
            continue;
        };
        out.push(GroupHistory {
            group,
            category: cat,
            bbq: b,
            completed_at: *completed_at,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000;
    const HOUR: i64 = 60 * 60;

    fn history_of(category: Category, bbq: BBQ, at: i64) -> Vec<GroupHistory> {
        vec![GroupHistory {
            group: CooldownGroup::from_category(&category),
            category,
            bbq,
            completed_at: at,
        }]
    }

    fn entry(category: Category, bbq: BBQ, at: i64) -> GroupHistory {
        GroupHistory {
            group: CooldownGroup::from_category(&category),
            category,
            bbq,
            completed_at: at,
        }
    }

    fn policy() -> CooldownPolicy {
        CooldownPolicy::default()
    }

    // ------------------------------------------------------------- path 1: allowed

    #[test]
    fn allows_when_that_group_has_no_history() {
        let verdict = evaluate(&[], &Category::Gold, &BBQ::BBQ07, &policy(), T0);
        assert_eq!(verdict, CooldownVerdict::Allowed);
    }

    // ------------------------------------------------- corrupt future timestamps

    #[test]
    fn a_completion_stamped_far_in_the_future_is_ignored() {
        // Live data contains a row dated 2035 — nine years ahead. Honouring it
        // would block that player's Event group until then.
        let year_2035 = 2_069_445_092;
        let hist = history_of(Category::Event, BBQ::BBQ06, year_2035);
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ06, &configured, T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn a_completion_slightly_ahead_of_the_clock_is_still_trusted() {
        // Up to 30 days of skew is tolerated, so ordinary clock drift does not
        // silently disable every cooldown.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0 + 1);
        let verdict = evaluate(&hist, &Category::Gold, &BBQ::BBQ07, &policy(), T0);
        assert!(matches!(verdict, CooldownVerdict::Blocked { .. }));
    }

    #[test]
    fn the_future_skew_allowance_is_thirty_days() {
        assert_eq!(MAX_FUTURE_SKEW_SECS, 30 * 24 * 60 * 60);
        assert!(is_plausible(T0, T0));
        assert!(is_plausible(T0 + MAX_FUTURE_SKEW_SECS, T0));
        assert!(!is_plausible(T0 + MAX_FUTURE_SKEW_SECS + 1, T0));
    }

    #[test]
    fn a_corrupt_timestamp_does_not_paint_a_countdown_on_the_card() {
        // `event` must be non-zero here, otherwise the policy alone returns
        // Allowed and the plausibility guard is not actually exercised.
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        let year_2035 = 2_069_445_092;
        let hist = history_of(Category::Event, BBQ::BBQ06, year_2035);
        assert_eq!(
            earliest_reopen(&hist, &Category::Event, &configured, T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn a_plausible_timestamp_still_produces_a_countdown() {
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            earliest_reopen(&hist, &Category::Event, &configured, T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 6 * HOUR
            }
        );
    }

    #[test]
    fn allows_when_the_cooldown_has_expired() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        // A change waits 20 hours; one second past that is free.
        let verdict = evaluate(
            &hist,
            &Category::Silver,
            &BBQ::BBQ09,
            &policy(),
            T0 + 20 * HOUR + 1,
        );
        assert_eq!(verdict, CooldownVerdict::Allowed);
        assert!(verdict.is_allowed());
        assert_eq!(verdict.ends_at(), None);
    }

    // ------------------------------------------------------------- path 2: denied

    #[test]
    fn blocks_while_the_cooldown_is_running() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(
            &hist,
            &Category::Silver,
            &BBQ::BBQ09,
            &policy(),
            T0 + 20 * HOUR - 1,
        );
        assert_eq!(
            verdict,
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
        assert!(!verdict.is_allowed());
    }

    // ------------------------------------------------------------- path 3: boundary

    #[test]
    fn exactly_on_the_boundary_is_allowed() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(
            &hist,
            &Category::Silver,
            &BBQ::BBQ09,
            &policy(),
            T0 + 20 * HOUR,
        );
        assert_eq!(verdict, CooldownVerdict::Allowed);
    }

    #[test]
    fn one_second_before_the_boundary_is_blocked() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(
            &hist,
            &Category::Silver,
            &BBQ::BBQ09,
            &policy(),
            T0 + 20 * HOUR - 1,
        );
        assert!(matches!(verdict, CooldownVerdict::Blocked { .. }));
    }

    // ------------------------------------------------- progression shares one timer

    #[test]
    fn bronze_after_gold_is_blocked() {
        // The bug this fixes: chaining Gold -> Silver -> Bronze with no wait.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &policy(), T0 + 1);
        assert_eq!(
            verdict,
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
    }

    #[test]
    fn silver_after_gold_is_blocked() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(&hist, &Category::Silver, &BBQ::BBQ01, &policy(), T0 + 1);
        assert!(matches!(verdict, CooldownVerdict::Blocked { .. }));
    }

    #[test]
    fn silver_after_gold_with_the_same_bbq_counts_as_a_change() {
        // Gold BBQ07 -> Silver BBQ07 is NOT a repeat: the category differs.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(&hist, &Category::Silver, &BBQ::BBQ07, &policy(), T0 + 20 * HOUR - 1);
        assert_eq!(
            verdict,
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
    }

    #[test]
    fn bronze_after_silver_is_blocked() {
        let hist = history_of(Category::Silver, BBQ::BBQ03, T0);
        let verdict = evaluate(&hist, &Category::Bronze, &BBQ::BBQ09, &policy(), T0 + 1);
        assert!(matches!(verdict, CooldownVerdict::Blocked { .. }));
    }

    // ----------------------------------------------------- groups are independent

    #[test]
    fn free_and_progression_share_one_timer() {
        // The merge: a Free run and a Bronze/Silver/Gold run are the same wait, so
        // one player cannot take both back to back.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        assert_eq!(
            evaluate(&hist, &Category::Free, &BBQ::BBQ03, &policy(), T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );

        // ...and the other way around.
        let hist = history_of(Category::Free, BBQ::BBQ03, T0);
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &policy(), T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
    }

    #[test]
    fn free_repeated_waits_forty_hours() {
        // Free is a normal member of the standard group, so a repeat follows the
        // `same` branch and not a separate Free setting.
        let hist = history_of(Category::Free, BBQ::BBQ03, T0);
        assert_eq!(
            evaluate(&hist, &Category::Free, &BBQ::BBQ03, &policy(), T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
    }

    #[test]
    fn event_and_standard_have_separate_timers() {
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        // A fresh Event run must not block the ladder.
        assert_eq!(
            evaluate(&hist, &Category::Gold, &BBQ::BBQ07, &policy(), T0),
            CooldownVerdict::Allowed
        );

        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        // A standard run must not block Event either: Event only answers to its
        // own row, which is why an Event repeat can be gated without gating Gold.
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn event_and_free_have_separate_timers() {
        let hist = history_of(Category::Free, BBQ::BBQ03, T0);
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &configured, T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn the_latest_run_of_one_group_wins() {
        let hist = vec![
            entry(Category::Gold, BBQ::BBQ07, T0),
            entry(Category::Silver, BBQ::BBQ09, T0 + 50 * HOUR),
        ];
        // If the older row won, Bronze after BBQ07 would be a 20 hour wait that
        // ended at T0+20h — already past, so Allowed. Because the newer row wins,
        // Bronze counts as a change from Silver: 20 hours from T0+50h.
        let verdict = evaluate(&hist, &Category::Bronze, &BBQ::BBQ07, &policy(), T0 + 51 * HOUR);
        assert_eq!(
            verdict,
            CooldownVerdict::Blocked {
                ends_at: T0 + 70 * HOUR
            }
        );
    }

    // ------------------------------------------------------------- duration branches

    #[test]
    fn repeating_the_same_category_and_bbq_waits_forty_hours() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(
            &hist,
            &Category::Gold,
            &BBQ::BBQ07,
            &policy(),
            T0 + 20 * HOUR,
        );
        assert_eq!(
            verdict,
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
    }

    #[test]
    fn same_category_with_a_different_bbq_waits_twenty_hours() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let verdict = evaluate(
            &hist,
            &Category::Gold,
            &BBQ::BBQ09,
            &policy(),
            T0 + 20 * HOUR,
        );
        assert_eq!(verdict, CooldownVerdict::Allowed);
    }

    // ------------------------------------------------------------- event configured

    #[test]
    fn event_is_free_only_when_zero_is_configured_explicitly() {
        // `0` is no longer the default, but an operator may still ask for it. The
        // evaluator must honour that literally rather than falling back to 20.
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        let no_wait = CooldownPolicy {
            event: 0,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &no_wait, T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn every_group_has_a_distinct_database_key() {
        // The backfill builds its SQL from these numbers. A duplicated key would
        // silently collapse two timers into one row, so pin them here.
        let keys: Vec<i16> = CooldownGroup::ALL.iter().map(|g| g.encode()).collect();
        assert_eq!(keys, vec![0, 2]);
        assert_eq!(CooldownGroup::ALL.len(), 2);

        for group in CooldownGroup::ALL {
            assert_eq!(CooldownGroup::decode(group.encode()), Some(group));
        }
    }

    #[test]
    fn the_retired_free_key_still_decodes_as_standard() {
        // Rows written by the previous build carry group 1 for Free. If the merge
        // migration has not run yet they must still gate the standard timer rather
        // than being dropped, which would hand those players a free submission.
        assert_eq!(CooldownGroup::decode(1), Some(CooldownGroup::Standard));
        assert_eq!(CooldownGroup::Standard.encode(), 0);
        assert_eq!(CooldownGroup::Event.encode(), 2);

        let rows = vec![(1_i16, 3_i16, 5_i16, T0)];
        let hist = history_from_rows(&rows);
        assert_eq!(hist, vec![entry(Category::Free, BBQ::BBQ05, T0)]);
    }

    #[test]
    fn an_unknown_group_key_is_skipped() {
        let rows = vec![(7_i16, 0_i16, 1_i16, T0)];
        assert!(history_from_rows(&rows).is_empty());
    }

    #[test]
    fn the_shipped_default_gives_event_a_real_wait() {
        // The bug being fixed: an Event approval must not be repeatable instantly
        // by default. This pins the shipped default, not just the mechanics.
        assert_eq!(DEFAULT_EVENT_HOURS, 20);

        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0 + 19 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0 + 20 * HOUR),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn event_obeys_its_own_configured_wait() {
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };

        let blocked = evaluate(&hist, &Category::Event, &BBQ::BBQ01, &configured, T0 + 5 * HOUR);
        assert_eq!(
            blocked,
            CooldownVerdict::Blocked {
                ends_at: T0 + 6 * HOUR
            }
        );

        let allowed = evaluate(&hist, &Category::Event, &BBQ::BBQ01, &configured, T0 + 6 * HOUR);
        assert_eq!(allowed, CooldownVerdict::Allowed);
    }

    #[test]
    fn a_negative_configured_wait_is_treated_as_no_cooldown() {
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        let configured = CooldownPolicy {
            event: -5,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &configured, T0),
            CooldownVerdict::Allowed
        );
    }

    // ------------------------------------------------------- progression ladder gate

    use crate::reusable::config::ProgressionGateMode;

    fn tier() -> ProgressionTier {
        ProgressionTier::fallback()
    }

    /// The default mode, spelled out so a change to the default shows up here.
    const M: ProgressionGateMode = ProgressionGateMode::Counter;

    #[test]
    fn silver_requires_completed_bronze() {
        assert_eq!(
            progression_gate(0, 0, false, false, &Category::Silver, &tier(), M),
            ProgressionGate::Blocked {
                required: Category::Bronze,
                need: 21,
                have: 0
            }
        );
    }

    #[test]
    fn gold_requires_completed_silver() {
        assert_eq!(
            progression_gate(21, 0, true, false, &Category::Gold, &tier(), M),
            ProgressionGate::Blocked {
                required: Category::Silver,
                need: 20,
                have: 0
            }
        );
    }

    #[test]
    fn bronze_free_and_event_have_no_tier_requirement() {
        for category in [Category::Bronze, Category::Free, Category::Event] {
            assert_eq!(
                progression_gate(0, 0, false, false, &category, &tier(), M),
                ProgressionGate::Allowed,
                "{} should not need a prerequisite",
                category.name()
            );
        }
    }

    #[test]
    fn silver_allowed_exactly_at_the_goal() {
        assert_eq!(
            progression_gate(21, 0, true, false, &Category::Silver, &tier(), M),
            ProgressionGate::Allowed
        );
    }

    #[test]
    fn silver_blocked_one_below_the_goal() {
        assert!(matches!(
            progression_gate(20, 0, false, false, &Category::Silver, &tier(), M),
            ProgressionGate::Blocked { .. }
        ));
    }

    #[test]
    fn gold_allowed_exactly_at_the_goal() {
        assert_eq!(
            progression_gate(21, 20, true, true, &Category::Gold, &tier(), M),
            ProgressionGate::Allowed
        );
    }

    #[test]
    fn gold_blocked_one_below_the_goal() {
        assert!(matches!(
            progression_gate(21, 19, true, false, &Category::Gold, &tier(), M),
            ProgressionGate::Blocked { .. }
        ));
    }

    #[test]
    fn tier_goals_are_configurable() {
        let custom = ProgressionTier {
            bronze_goal: 5,
            silver_goal: 3,
        };
        assert_eq!(
            progression_gate(5, 0, false, false, &Category::Silver, &custom, M),
            ProgressionGate::Allowed
        );
        assert_eq!(
            progression_gate(5, 3, false, false, &Category::Gold, &custom, M),
            ProgressionGate::Allowed
        );
    }

    // ------------------------------------------------------------ gate modes

    #[test]
    fn counter_mode_ignores_a_missing_achievement_bit() {
        // The situation on live data: 225 players meet the counter but never got
        // the role. They must keep their access.
        assert_eq!(
            progression_gate(
                21,
                0,
                false,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Counter
            ),
            ProgressionGate::Allowed
        );
    }

    #[test]
    fn either_mode_accepts_the_achievement_bit_alone() {
        assert_eq!(
            progression_gate(
                0,
                0,
                true,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Either
            ),
            ProgressionGate::Allowed
        );
    }

    #[test]
    fn either_mode_still_blocks_when_neither_is_present() {
        assert!(matches!(
            progression_gate(
                0,
                0,
                false,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Either
            ),
            ProgressionGate::Blocked { .. }
        ));
    }

    #[test]
    fn both_mode_blocks_a_counter_without_the_bit() {
        // This is why `Counter` is the default: requiring both would lock out the
        // 225 players whose role was never granted.
        assert!(matches!(
            progression_gate(
                21,
                0,
                false,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Both
            ),
            ProgressionGate::Blocked { .. }
        ));
    }

    #[test]
    fn both_mode_blocks_a_bit_without_the_counter() {
        assert!(matches!(
            progression_gate(
                0,
                0,
                true,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Both
            ),
            ProgressionGate::Blocked { .. }
        ));
    }

    #[test]
    fn both_mode_allows_when_counter_and_bit_agree() {
        assert_eq!(
            progression_gate(
                21,
                0,
                true,
                false,
                &Category::Silver,
                &tier(),
                ProgressionGateMode::Both
            ),
            ProgressionGate::Allowed
        );
    }

    #[test]
    fn gold_obeys_each_mode_the_same_way() {
        for (mode, expected) in [
            (ProgressionGateMode::Counter, ProgressionGate::Allowed),
            (ProgressionGateMode::Either, ProgressionGate::Allowed),
            (ProgressionGateMode::Both, ProgressionGate::Allowed),
        ] {
            assert_eq!(
                progression_gate(21, 20, true, true, &Category::Gold, &tier(), mode),
                expected,
                "mode {:?}",
                mode
            );
        }
    }
}
