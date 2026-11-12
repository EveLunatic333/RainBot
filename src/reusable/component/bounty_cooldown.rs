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
//!
//! The operator switch for the standard timer is still `discord.latest_bounty_time`,
//! the column the old build used. [`history_with_legacy`] folds that column over the
//! stored rows so a plain `UPDATE discord SET latest_bounty_time = ...` keeps
//! working. Event is never driven by that column: it was exempt under the old rule,
//! and it now has a timer of its own.

use super::bounty::{BBQ, BountyTitle, Category};

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
///
/// It ships at 40 hours to match `same`, which is the wait Event had in practice
/// before this module existed: the old single timestamp charged 40 hours when the
/// category and the BBQ repeated, and an Event queue is normally the same bounty
/// over and over. `same` and `different` do not apply to Event — this constant is
/// the whole rule for that group.
pub const DEFAULT_EVENT_HOURS: i64 = 40;

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
    /// Whether Event shares the standard timer instead of keeping its own.
    ///
    /// Set from `bounty.event_shares_standard`. When `true`, Event is mapped onto
    /// [`CooldownGroup::Standard`] for evaluation, writing and display, which makes
    /// an Event run lock the progression ladder and the ladder lock Event. When
    /// `false`, the historical split applies and `event` is the whole Event rule.
    pub event_shares_standard: bool,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self {
            same: DEFAULT_SAME_HOURS,
            different: DEFAULT_DIFF_HOURS,
            event: DEFAULT_EVENT_HOURS,
            event_shares_standard: false,
        }
    }
}

impl CooldownPolicy {
    /// Whether Event and the progression ladder share one timer.
    pub fn event_shares_standard(&self) -> bool {
        self.event_shares_standard
    }

    /// Enables or disables Event sharing, for callers that build the policy from
    /// `cooldown_hours` and then apply the operator switch.
    pub fn with_event_sharing(mut self, on: bool) -> Self {
        self.event_shares_standard = on;
        self
    }

    /// The group a category is evaluated against, taking the option into account.
    ///
    /// This is the option-aware counterpart of [`CooldownGroup::from_category`].
    /// Every read, write and display path must go through this rather than the
    /// static mapping, or Event silently keeps its old behaviour.
    pub fn group_of(&self, category: &Category) -> CooldownGroup {
        if self.event_shares_standard && *category == Category::Event {
            // Event joins the ladder, so it uses the shared row and the
            // `same`/`different` rule. `event` is deliberately not consulted.
            CooldownGroup::Standard
        } else {
            CooldownGroup::from_category(category)
        }
    }

    /// Rewrites stored rows onto their effective groups.
    ///
    /// Rows are written with the group that was in force at the time, so an Event
    /// completion sits in `group_id = 2`. Turning the option on must make that row
    /// count for the standard timer, and turning it off must let Event recover its
    /// own timer from the same row. Remapping on read does both without a
    /// migration: the stored key is never destroyed, only reinterpreted.
    ///
    /// A remapped row keeps its category, so the `same`/`different` decision still
    /// knows which bounty the previous run was.
    pub fn normalize(&self, history: &[GroupHistory]) -> Vec<GroupHistory> {
        if !self.event_shares_standard {
            return history.to_vec();
        }
        history
            .iter()
            .map(|h| {
                if h.group == CooldownGroup::Event {
                    GroupHistory {
                        group: CooldownGroup::Standard,
                        ..h.clone()
                    }
                } else {
                    h.clone()
                }
            })
            .collect()
    }

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
    let group = policy.group_of(new_category);
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
    let group = policy.group_of(category);
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

/// When a player may repeat their last bounty unchanged, and when they may take
/// a different one.
///
/// The card used to print only "Ready" or a single countdown, which left players
/// asking when a specific bounty opened again. The old build printed both waits
/// ("Different Bounty CD" and "Same Bounty CD"), and that is the pair this
/// restores.
///
/// A *change* is a different category in the same group, or the same category
/// with a different BBQ number; it waits `different` (`event` for the Event
/// group). A *repeat* is the same category **and** the same BBQ; it waits `same`,
/// except for Event, where `event` is the whole rule and a repeat costs the same
/// as a change.
///
/// `last_category` / `last_bbq` name the bounty both waits are measured against.
/// Without them "different" and "same" mean nothing to a reader: the card has to
/// say *which* bounty the repeat refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenWindows {
    /// Category of the run the waits are measured from.
    pub last_category: Category,
    /// BBQ number of the run the waits are measured from.
    pub last_bbq: BBQ,
    /// Shortest wait: pick a different category or BBQ number.
    pub change: CooldownVerdict,
    /// Wait before the exact same category and BBQ can be submitted again.
    pub repeat: CooldownVerdict,
}

impl ReopenWindows {
    /// Whether the same category and BBQ may be repeated right now.
    pub fn repeat_is_open(&self) -> bool {
        self.repeat.is_allowed()
    }

    /// Whether some other bounty in the group may be taken right now.
    pub fn change_is_open(&self) -> bool {
        self.change.is_allowed()
    }
}

/// Both reopen windows for `category`'s group.
///
/// Returns `None` when the group has no history, the recorded run is corrupt, or
/// both waits are configured to zero: there is nothing for the card to count down.
pub fn reopen_windows(
    history: &[GroupHistory],
    category: &Category,
    policy: &CooldownPolicy,
    now: i64,
) -> Option<ReopenWindows> {
    let group = policy.group_of(category);
    let previous = last_of_group(history, group)?;

    // Same guard as `evaluate`: a corrupt future timestamp must not paint a
    // multi-year countdown on the card.
    if !is_plausible(previous.completed_at, now) {
        return None;
    }

    // Ask the evaluator itself rather than re-deriving the hours, so the card
    // cannot drift from what the submit flow enforces. `previous.category` and
    // `previous.bbq` are exactly what an unchanged repeat would submit.
    let repeat = evaluate(
        history,
        &previous.category,
        &previous.bbq,
        policy,
        now,
    );
    let change = match group {
        // Event ignores `same`/`different`, so a change waits the same amount.
        CooldownGroup::Event => repeat,
        // A change from the last run, with a BBQ that is guaranteed to differ.
        _ => {
            let other = different_bbq(&previous.bbq);
            evaluate(history, &previous.category, &other, policy, now)
        }
    };

    if repeat.is_allowed() && change.is_allowed() {
        return None;
    }
    Some(ReopenWindows {
        last_category: previous.category,
        last_bbq: previous.bbq,
        change,
        repeat,
    })
}

/// A BBQ number that is never equal to `bbq`, so a probe submission counts as a
/// change rather than a repeat.
fn different_bbq(bbq: &BBQ) -> BBQ {
    let code = bbq.encode();
    BBQ::new(if code == 1 { 2 } else { 1 }).unwrap_or(BBQ::BBQ01)
}

/// Builds the policy from `config.json`, so the waits are operator settings.
impl From<&super::super::config::CooldownHours> for CooldownPolicy {
    fn from(c: &super::super::config::CooldownHours) -> Self {
        Self {
            same: c.same,
            different: c.different,
            event: c.event,
            // The hours block says nothing about sharing; that lives in
            // `bounty.event_shares_standard`. Call sites apply it with
            // `with_event_sharing`, so this conversion stays default-off.
            event_shares_standard: false,
        }
    }
}

/// The policy in force for one `bounty` config block.
///
/// This is the constructor every caller should use: it reads both the hour values
/// and the sharing switch, so a caller cannot accidentally apply the hours while
/// ignoring the option.
impl From<&super::super::config::Bounty> for CooldownPolicy {
    fn from(b: &super::super::config::Bounty) -> Self {
        CooldownPolicy::from(&b.cooldown_hours).with_event_sharing(b.event_shares_standard)
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

/// What the legacy `discord.latest_bounty_time` column says about the standard
/// timer.
///
/// The old build kept one timestamp per player and gated every non-Event
/// submission on it, so an operator could move a cooldown with a single
/// `UPDATE discord SET latest_bounty_time = ...`. That habit is kept: the column
/// stays authoritative for [`CooldownGroup::Standard`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyStandard {
    /// The column is zero (or negative): the operator cleared the timer.
    Cleared,
    /// The column holds a readable, non-Event completion.
    Entry(GroupHistory),
    /// The column cannot drive the standard timer: it does not decrypt, or the
    /// last bounty was an Event.
    Unusable,
}

/// Reads the legacy column for the standard timer.
///
/// A zero means "no cooldown", which is exactly how the old build read it, so an
/// operator clearing the column still clears the wait. An unreadable
/// `latest_bounty` is ignored rather than treated as a permanent block: the old
/// build could not decrypt those rows either and fell through to no history.
pub fn legacy_entry(latest_bounty: &str, latest_bounty_time: i64) -> LegacyStandard {
    if latest_bounty_time <= 0 {
        return LegacyStandard::Cleared;
    }
    let Some((category, bbq)) = BountyTitle::decrypt(latest_bounty) else {
        return LegacyStandard::Unusable;
    };
    // Event never drove the standard timer: it was exempt under the old rule and
    // it has its own row now.
    if category == Category::Event {
        return LegacyStandard::Unusable;
    }
    LegacyStandard::Entry(GroupHistory {
        group: CooldownGroup::Standard,
        category,
        bbq,
        completed_at: latest_bounty_time,
    })
}

/// Folds the legacy column over the stored rows.
///
/// Only the standard group is affected. `Cleared` drops the stored standard row
/// so a zeroed column really means "no wait"; `Entry` replaces it, which lets an
/// operator both shorten and extend a cooldown with SQL; `Unusable` leaves the
/// history untouched, so the stored row still speaks for a player whose last
/// bounty was an Event.
pub fn apply_legacy(history: &[GroupHistory], legacy: LegacyStandard) -> Vec<GroupHistory> {
    match legacy {
        LegacyStandard::Unusable => history.to_vec(),
        LegacyStandard::Cleared => history
            .iter()
            .filter(|h| h.group != CooldownGroup::Standard)
            .cloned()
            .collect(),
        LegacyStandard::Entry(entry) => {
            let mut out: Vec<GroupHistory> = history
                .iter()
                .filter(|h| h.group != CooldownGroup::Standard)
                .cloned()
                .collect();
            out.push(entry);
            out
        }
    }
}

/// History the evaluator should see: stored rows with the legacy column applied.
///
/// Every caller goes through this so the submit pre-check, `Hunter::new` and the
/// player card cannot disagree about a cooldown.
pub fn history_with_legacy(
    history: &[GroupHistory],
    latest_bounty: &str,
    latest_bounty_time: i64,
) -> Vec<GroupHistory> {
    apply_legacy(history, legacy_entry(latest_bounty, latest_bounty_time))
}

/// History for a policy that may fold Event into the standard group.
///
/// Same as [`history_with_legacy`], but the stored rows are first remapped onto
/// their effective groups via [`CooldownPolicy::normalize`]. Without that step an
/// Event row stays in its own group, so turning `event_shares_standard` on would
/// not make a completed Event block the progression ladder.
///
/// The legacy column is applied afterwards on purpose: `legacy_entry` already
/// refuses to drive Event (an Event timestamp reports `Unusable`), which stays
/// correct in both modes because that column never gated Event.
pub fn history_with_legacy_and_policy(
    history: &[GroupHistory],
    latest_bounty: &str,
    latest_bounty_time: i64,
    policy: &CooldownPolicy,
) -> Vec<GroupHistory> {
    let normalized = policy.normalize(history);
    apply_legacy(&normalized, legacy_entry(latest_bounty, latest_bounty_time))
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
        // The card must not print a nine-year countdown either.
        assert_eq!(
            reopen_windows(&hist, &Category::Event, &configured, T0),
            None
        );
    }

    // ------------------------------------------------------------- reopen windows

    #[test]
    fn no_history_means_no_countdown_to_show() {
        assert_eq!(reopen_windows(&[], &Category::Gold, &policy(), T0), None);
    }

    #[test]
    fn both_windows_expired_means_no_countdown_to_show() {
        // 41 hours after a run the 20 h and 40 h waits have both ended, so the
        // card must say Ready rather than print two stale times.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Gold, &policy(), T0 + 41 * HOUR),
            None
        );
    }

    #[test]
    fn a_fresh_standard_run_reports_both_waits() {
        // 20 hours for a different BBQ, 40 hours for the exact same one. This is
        // the pair the old card printed and the new one had lost.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Bronze, &policy(), T0 + HOUR),
            Some(ReopenWindows {
                last_category: Category::Gold,
                last_bbq: BBQ::BBQ07,
                change: CooldownVerdict::Blocked {
                    ends_at: T0 + 20 * HOUR
                },
                repeat: CooldownVerdict::Blocked {
                    ends_at: T0 + 40 * HOUR
                },
            })
        );
    }

    #[test]
    fn the_group_is_what_selects_the_window_not_the_asked_category() {
        // Asking about Bronze after a Gold run reads the standard row, so the
        // countdown is the same one Gold would report.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let asked_bronze = reopen_windows(&hist, &Category::Bronze, &policy(), T0 + HOUR);
        let asked_gold = reopen_windows(&hist, &Category::Gold, &policy(), T0 + HOUR);
        assert_eq!(asked_bronze, asked_gold);
    }

    #[test]
    fn the_change_window_closes_before_the_repeat_window() {
        // 25 hours in: any different bounty is open again, but the exact repeat
        // still has 15 hours to go. The card must show one line, not zero or two.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Gold, &policy(), T0 + 25 * HOUR),
            Some(ReopenWindows {
                last_category: Category::Gold,
                last_bbq: BBQ::BBQ07,
                change: CooldownVerdict::Allowed,
                repeat: CooldownVerdict::Blocked {
                    ends_at: T0 + 40 * HOUR
                },
            })
        );
    }

    #[test]
    fn event_reports_the_same_wait_for_change_and_repeat() {
        // `event` is the whole rule for that group: a different BBQ costs exactly
        // the same as the exact repeat, so both windows share one time.
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Event, &policy(), T0 + HOUR),
            Some(ReopenWindows {
                last_category: Category::Event,
                last_bbq: BBQ::BBQ01,
                change: CooldownVerdict::Blocked {
                    ends_at: T0 + 40 * HOUR
                },
                repeat: CooldownVerdict::Blocked {
                    ends_at: T0 + 40 * HOUR
                },
            })
        );
    }

    #[test]
    fn the_event_window_follows_the_configured_value() {
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Event, &configured, T0 + HOUR),
            Some(ReopenWindows {
                last_category: Category::Event,
                last_bbq: BBQ::BBQ01,
                change: CooldownVerdict::Blocked {
                    ends_at: T0 + 6 * HOUR
                },
                repeat: CooldownVerdict::Blocked {
                    ends_at: T0 + 6 * HOUR
                },
            })
        );
    }

    #[test]
    fn a_zero_wait_leaves_nothing_to_count_down() {
        // An operator who configures `event: 0` must get a plain Ready card.
        let no_wait = CooldownPolicy {
            event: 0,
            ..CooldownPolicy::default()
        };
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(reopen_windows(&hist, &Category::Event, &no_wait, T0), None);
    }

    #[test]
    fn the_probe_bbq_always_differs_for_a_repeat_check() {
        // The change window is measured with a BBQ that is guaranteed different,
        // otherwise BBQ01 would collapse into the repeat branch.
        assert_eq!(different_bbq(&BBQ::BBQ01), BBQ::BBQ02);
        assert_eq!(different_bbq(&BBQ::BBQ02), BBQ::BBQ01);
        assert_eq!(different_bbq(&BBQ::BBQ25), BBQ::BBQ01);
        for code in 1..=25u8 {
            let bbq = BBQ::new(code).expect("valid bbq");
            assert_ne!(different_bbq(&bbq), bbq, "bbq {code} must change");
        }
    }

    #[test]
    fn the_windows_still_work_when_the_run_came_from_the_legacy_column() {
        // The card folds the legacy column in before calling this, so the windows
        // must honour an SQL-planted cooldown too.
        let hist = history_with_legacy(&[], "2_7", T0);
        assert_eq!(
            reopen_windows(&hist, &Category::Bronze, &policy(), T0 + HOUR),
            Some(ReopenWindows {
                last_category: Category::Gold,
                last_bbq: BBQ::BBQ07,
                change: CooldownVerdict::Blocked {
                    ends_at: T0 + 20 * HOUR
                },
                repeat: CooldownVerdict::Blocked {
                    ends_at: T0 + 40 * HOUR
                },
            })
        );
    }

    #[test]
    fn the_windows_name_the_bounty_they_are_measured_from() {
        // The card prints this, so a wrong reference would tell the player to
        // wait for a bounty they never did.
        let hist = history_of(Category::Free, BBQ::BBQ12, T0);
        let windows = reopen_windows(&hist, &Category::Bronze, &policy(), T0 + HOUR)
            .expect("fresh run should have windows");
        assert_eq!(windows.last_category, Category::Free);
        assert_eq!(windows.last_bbq, BBQ::BBQ12);
    }

    #[test]
    fn the_open_helpers_agree_with_the_verdicts() {
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        // 25 hours in: a change is open, the exact repeat is not.
        let windows = reopen_windows(&hist, &Category::Gold, &policy(), T0 + 25 * HOUR)
            .expect("repeat wait still running");
        assert!(windows.change_is_open());
        assert!(!windows.repeat_is_open());
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
        // evaluator must honour that literally rather than falling back to 40.
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
        assert_eq!(DEFAULT_EVENT_HOURS, 40);

        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0 + 39 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0 + 40 * HOUR),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn event_ignores_the_same_and_different_settings() {
        // `event` is the whole rule for that group. A repeat and a change both
        // wait 40 hours on the shipped default, so an operator changing `same` or
        // `different` cannot silently move the Event wait.
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);

        // Same category and BBQ: would be 40 under `same`, and it is.
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy(), T0 + 39 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
        // Different BBQ: `different` is 20, but Event must still wait 40.
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ09, &policy(), T0 + 25 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );

        // Changing `same` / `different` must not move the Event wait at all.
        let tweaked = CooldownPolicy {
            same: 1,
            different: 2,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &tweaked, T0 + 39 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
    }

    #[test]
    fn the_event_default_survives_a_config_without_the_key() {
        // `serde(default)` means an existing config.json that predates the key
        // picks up the new default rather than silently keeping the old wait.
        let parsed: crate::reusable::config::CooldownHours =
            serde_json::from_str(r#"{"same": 40, "different": 20}"#).expect("config should parse");
        assert_eq!(parsed.event, DEFAULT_EVENT_HOURS);
        assert_eq!(parsed.event, 40);

        let policy = CooldownPolicy::from(&parsed);
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy, T0 + 39 * HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 40 * HOUR
            }
        );
    }

    #[test]
    fn an_explicit_config_value_overrides_the_event_default() {
        // The other half of the rule: a config.json that names `event` wins over
        // the compiled default, in both directions. This is why an operator can
        // shorten Event without rebuilding.
        for (configured, expected) in [(6_i64, 6_i64), (40, 40), (1, 1), (0, 0)] {
            let json = format!(r#"{{"same": 40, "different": 20, "event": {configured}}}"#);
            let parsed: crate::reusable::config::CooldownHours =
                serde_json::from_str(&json).expect("config should parse");
            assert_eq!(
                parsed.event, configured,
                "config value {configured} must not be replaced by the default"
            );

            let policy = CooldownPolicy::from(&parsed);
            let hist = history_of(Category::Event, BBQ::BBQ01, T0);
            let verdict = evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy, T0);
            if expected == 0 {
                assert_eq!(verdict, CooldownVerdict::Allowed, "event 0 means no wait");
            } else {
                assert_eq!(
                    verdict,
                    CooldownVerdict::Blocked {
                        ends_at: T0 + expected * HOUR
                    },
                    "event {configured} must gate for exactly that long"
                );
            }
        }
    }

    #[test]
    fn the_whole_cooldown_block_can_be_omitted() {
        let parsed: crate::reusable::config::CooldownHours =
            serde_json::from_str("{}").expect("an empty block should parse");
        assert_eq!(parsed.same, DEFAULT_SAME_HOURS);
        assert_eq!(parsed.different, DEFAULT_DIFF_HOURS);
        assert_eq!(parsed.event, DEFAULT_EVENT_HOURS);
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

    // ------------------------------------------------- legacy column compatibility

    /// The column the old build used, spelled the way `BountyTitle::encrypt`
    /// writes it: `"<category>_<bbq>"`.
    fn code(category: &Category, bbq: &BBQ) -> String {
        format!("{}_{}", category.encode(), bbq.encode())
    }

    #[test]
    fn a_zeroed_legacy_column_clears_the_standard_timer() {
        // The old reset: `UPDATE discord SET latest_bounty_time = 0`.
        let hist = history_of(Category::Gold, BBQ::BBQ07, T0);
        let legacy = legacy_entry("0_1", 0);
        assert_eq!(legacy, LegacyStandard::Cleared);
        let cleared = apply_legacy(&hist, legacy);
        assert_eq!(cleared, vec![]);
        assert_eq!(
            evaluate(&cleared, &Category::Bronze, &BBQ::BBQ01, &policy(), T0 + HOUR),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn a_negative_legacy_timestamp_also_clears() {
        assert_eq!(legacy_entry("2_7", -1), LegacyStandard::Cleared);
        assert_eq!(legacy_entry("2_7", i64::MIN), LegacyStandard::Cleared);
    }

    #[test]
    fn a_legacy_standard_entry_drives_the_standard_timer() {
        // `UPDATE discord SET latest_bounty='2_7', latest_bounty_time = <now>`.
        let legacy = legacy_entry(&code(&Category::Gold, &BBQ::BBQ07), T0);
        let hist = apply_legacy(&[], legacy);
        assert_eq!(hist, vec![entry(Category::Gold, BBQ::BBQ07, T0)]);
        assert_eq!(
            evaluate(&hist, &Category::Free, &BBQ::BBQ03, &policy(), T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
    }

    #[test]
    fn the_legacy_entry_overrides_a_stored_standard_row() {
        // An operator shortening a wait: the stored row says 20 hours from now, the
        // column says 30 hours ago. The column wins, so the wait is already over.
        let stored = history_of(Category::Gold, BBQ::BBQ07, T0 + 20 * HOUR);
        let legacy = legacy_entry(&code(&Category::Free, &BBQ::BBQ03), T0 - 30 * HOUR);
        let hist = apply_legacy(&stored, legacy);
        assert_eq!(hist.len(), 1);
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &policy(), T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn the_legacy_entry_also_extends_a_stored_row() {
        // The other direction: the stored row is old, the column is fresh.
        let stored = history_of(Category::Gold, BBQ::BBQ07, T0 - 100 * HOUR);
        let legacy = legacy_entry(&code(&Category::Silver, &BBQ::BBQ09), T0);
        let hist = apply_legacy(&stored, legacy);
        assert_eq!(hist.len(), 1);
        assert_eq!(
            evaluate(&hist, &Category::Gold, &BBQ::BBQ07, &policy(), T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 20 * HOUR
            }
        );
    }

    #[test]
    fn the_legacy_entry_never_touches_the_event_row() {
        // A stored Event row must survive a legacy standard entry, otherwise an
        // operator editing the standard timer would hand out a free Event repeat.
        let stored = history_of(Category::Event, BBQ::BBQ01, T0);
        let legacy = legacy_entry(&code(&Category::Gold, &BBQ::BBQ07), T0);
        let hist = apply_legacy(&stored, legacy);
        assert!(hist.iter().any(|h| h.group == CooldownGroup::Event));

        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &configured, T0 + HOUR),
            CooldownVerdict::Blocked {
                ends_at: T0 + 6 * HOUR
            }
        );
    }

    #[test]
    fn a_zeroed_column_also_leaves_the_event_row_alone() {
        let stored = vec![
            entry(Category::Gold, BBQ::BBQ07, T0),
            entry(Category::Event, BBQ::BBQ01, T0),
        ];
        let hist = apply_legacy(&stored, LegacyStandard::Cleared);
        assert_eq!(hist, vec![entry(Category::Event, BBQ::BBQ01, T0)]);
    }

    #[test]
    fn a_legacy_event_last_bounty_does_not_gate_the_standard_timer() {
        // The old build exempted Event, so the column cannot mean "wait" here. If
        // it did, a player whose last bounty was an Event would lose 20 hours of
        // ladder access that the old build never took away.
        let legacy = legacy_entry(&code(&Category::Event, &BBQ::BBQ01), T0);
        assert_eq!(legacy, LegacyStandard::Unusable);

        let stored = history_of(Category::Gold, BBQ::BBQ07, T0 - 100 * HOUR);
        let hist = apply_legacy(&stored, legacy);
        assert_eq!(hist, stored);
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &policy(), T0),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn an_unreadable_legacy_column_falls_back_to_the_stored_row() {
        // Live data still holds malformed values. They must not block forever and
        // must not erase a real cooldown either.
        for broken in ["", "gold_7", "2", "2_", "_", "2_99", "9_1", "0_0"] {
            assert_eq!(
                legacy_entry(broken, T0),
                LegacyStandard::Unusable,
                "input {broken:?}"
            );
        }

        let stored = history_of(Category::Gold, BBQ::BBQ07, T0);
        let hist = apply_legacy(&stored, legacy_entry("gold_7", T0));
        assert_eq!(hist, stored);
    }

    #[test]
    fn history_with_legacy_is_what_the_callers_use() {
        let rows = vec![
            (2_i16, 4_i16, 1_i16, T0),
            (0_i16, 2_i16, 7_i16, T0 - 100 * HOUR),
        ];
        let stored = history_from_rows(&rows);

        // The column clears the standard wait but keeps the Event wait.
        let hist = history_with_legacy(&stored, "0_1", 0);
        assert_eq!(hist, vec![entry(Category::Event, BBQ::BBQ01, T0)]);

        // The column sets a fresh standard wait on top of the stored Event row.
        let hist = history_with_legacy(&stored, &code(&Category::Gold, &BBQ::BBQ07), T0);
        assert_eq!(
            hist,
            vec![
                entry(Category::Event, BBQ::BBQ01, T0),
                entry(Category::Gold, BBQ::BBQ07, T0),
            ]
        );
    }

    #[test]
    fn a_corrupt_legacy_future_timestamp_is_still_ignored() {
        // The plausibility guard has to work on the legacy path too, otherwise a
        // hand-edited 2035 row would paint a nine-year cooldown.
        let year_2035 = 2_069_445_092;
        let hist = history_with_legacy(&[], &code(&Category::Gold, &BBQ::BBQ07), year_2035);
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &policy(), T0),
            CooldownVerdict::Allowed
        );
    }

    // ---------------------------------------------------------------------
    // Event sharing the standard timer (`bounty.event_shares_standard`)
    // ---------------------------------------------------------------------

    /// A 40/20/40 policy with the sharing switch in the requested state.
    fn shared_policy(on: bool) -> CooldownPolicy {
        CooldownPolicy::default().with_event_sharing(on)
    }

    #[test]
    fn the_shipped_default_keeps_event_separate() {
        // Turning the option on must be a deliberate operator action, so the
        // default has to stay off or an upgrade would change live behaviour.
        assert!(!CooldownPolicy::default().event_shares_standard());
        let parsed: super::super::super::config::CooldownHours =
            serde_json::from_str("{}").expect("an empty hours block must parse");
        assert!(
            !CooldownPolicy::from(&parsed).event_shares_standard(),
            "a config without the key must not enable sharing"
        );
    }

    #[test]
    fn the_option_comes_from_the_bounty_block() {
        // The switch lives beside the hours, not inside them, so the constructor
        // that reads the whole `bounty` block is the one that must see it.
        let cfg: super::super::super::config::Bounty = serde_json::from_value(serde_json::json!({
            "board_ch": 0, "conquered_ch": 0, "receptionist_ch": 0, "promotion_ch": 0,
            "cooldown_ch": 0, "cooldown_msg": 0, "judge_ch": 0,
            "event_shares_standard": true
        }))
        .expect("a bounty block with the switch must parse");
        assert!(CooldownPolicy::from(&cfg).event_shares_standard());

        let legacy: super::super::super::config::Bounty = serde_json::from_value(serde_json::json!({
            "board_ch": 0, "conquered_ch": 0, "receptionist_ch": 0, "promotion_ch": 0,
            "cooldown_ch": 0, "cooldown_msg": 0, "judge_ch": 0
        }))
        .expect("a bounty block without the switch must still parse");
        assert!(!CooldownPolicy::from(&legacy).event_shares_standard());
    }

    #[test]
    fn event_off_does_not_block_the_ladder() {
        // The behaviour that must survive: with sharing off, a completed Event
        // leaves Bronze untouched.
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ01, &shared_policy(false), T0 + HOUR),
            CooldownVerdict::Allowed
        );
    }

    #[test]
    fn event_on_blocks_the_ladder() {
        // An Event run now locks the standard group, so Bronze must wait.
        let policy = shared_policy(true);
        let hist = policy.normalize(&history_of(Category::Event, BBQ::BBQ01, T0));
        assert_eq!(
            evaluate(&hist, &Category::Bronze, &BBQ::BBQ02, &policy, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + 20 * HOUR },
            "a different BBQ waits `different`, measured from the Event run"
        );
    }

    #[test]
    fn event_on_is_blocked_by_the_ladder() {
        // The other direction: a completed Bronze run must now gate Event.
        let policy = shared_policy(true);
        let hist = policy.normalize(&history_of(Category::Bronze, BBQ::BBQ03, T0));
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ01, &policy, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + 20 * HOUR }
        );
    }

    #[test]
    fn event_on_uses_same_and_different_not_the_event_hours() {
        // The confirmed semantic: Event obeys `same`/`different`, so the `event`
        // value must not be consulted while sharing is on.
        let policy = shared_policy(true);

        let repeat = policy.normalize(&history_of(Category::Event, BBQ::BBQ01, T0));
        assert_eq!(
            evaluate(&repeat, &Category::Event, &BBQ::BBQ01, &policy, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + DEFAULT_SAME_HOURS * HOUR },
            "an unchanged Event repeat waits `same`"
        );

        let change = policy.normalize(&history_of(Category::Event, BBQ::BBQ01, T0));
        assert_eq!(
            evaluate(&change, &Category::Event, &BBQ::BBQ09, &policy, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + DEFAULT_DIFF_HOURS * HOUR },
            "a different Event BBQ waits `different`"
        );
    }

    #[test]
    fn event_off_still_uses_the_event_hours() {
        // Regression guard for the other half of the switch.
        let policy = shared_policy(false);
        let hist = history_of(Category::Event, BBQ::BBQ01, T0);
        assert_eq!(
            evaluate(&hist, &Category::Event, &BBQ::BBQ02, &policy, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + DEFAULT_EVENT_HOURS * HOUR }
        );
    }

    #[test]
    fn normalize_maps_only_the_event_group() {
        let standard = GroupHistory {
            group: CooldownGroup::Standard,
            category: Category::Gold,
            bbq: BBQ::BBQ07,
            completed_at: T0,
        };
        let event = GroupHistory {
            group: CooldownGroup::Event,
            category: Category::Event,
            bbq: BBQ::BBQ02,
            completed_at: T0 + 5,
        };
        let rows = vec![standard.clone(), event.clone()];

        // Off: untouched, so a stored Event row keeps driving its own timer.
        assert_eq!(shared_policy(false).normalize(&rows), rows);

        // On: only the Event row moves, and it keeps its category/bbq/time so the
        // `same` versus `different` decision still knows the previous run.
        let mapped = shared_policy(true).normalize(&rows);
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0], standard);
        assert_eq!(mapped[1].group, CooldownGroup::Standard);
        assert_eq!(mapped[1].category, Category::Event);
        assert_eq!(mapped[1].bbq, BBQ::BBQ02);
        assert_eq!(mapped[1].completed_at, T0 + 5);
    }

    #[test]
    fn the_write_key_follows_the_option() {
        // `send_reward` uses `group_of(...).encode()`, so this pins the database
        // key an Event completion lands on in each mode.
        assert_eq!(shared_policy(false).group_of(&Category::Event).encode(), 2);
        assert_eq!(shared_policy(true).group_of(&Category::Event).encode(), 0);
        for on in [false, true] {
            let p = shared_policy(on);
            assert_eq!(p.group_of(&Category::Bronze).encode(), 0);
            assert_eq!(p.group_of(&Category::Free).encode(), 0);
        }
    }

    #[test]
    fn turning_the_option_off_restores_the_events_own_timer() {
        // The stored row was never moved, so switching back must recover the real
        // Event cooldown rather than losing it.
        let stored = history_of(Category::Event, BBQ::BBQ04, T0);

        let on = shared_policy(true);
        assert_eq!(
            evaluate(&on.normalize(&stored), &Category::Event, &BBQ::BBQ01, &on, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + DEFAULT_DIFF_HOURS * HOUR }
        );

        let off = shared_policy(false);
        assert_eq!(
            evaluate(&off.normalize(&stored), &Category::Event, &BBQ::BBQ01, &off, T0 + HOUR),
            CooldownVerdict::Blocked { ends_at: T0 + DEFAULT_EVENT_HOURS * HOUR }
        );
    }

    #[test]
    fn the_latest_row_wins_when_both_groups_map_together() {
        // With sharing on, a standard row and an event row become the same group.
        // The newer one must be the one that gates.
        let policy = shared_policy(true);
        let rows = vec![
            GroupHistory {
                group: CooldownGroup::Standard,
                category: Category::Gold,
                bbq: BBQ::BBQ01,
                completed_at: T0,
            },
            GroupHistory {
                group: CooldownGroup::Event,
                category: Category::Event,
                bbq: BBQ::BBQ02,
                completed_at: T0 + 2 * HOUR,
            },
        ];
        let hist = policy.normalize(&rows);
        assert_eq!(
            last_of_group(&hist, CooldownGroup::Standard)
                .expect("there must be a standard row")
                .completed_at,
            T0 + 2 * HOUR
        );
    }

    #[test]
    fn a_legacy_event_timestamp_still_never_drives_standard() {
        // `latest_bounty_time` was never a gate for Event, and folding Event into
        // the ladder must not change that.
        let hist = history_with_legacy_and_policy(
            &[],
            &code(&Category::Event, &BBQ::BBQ01),
            T0,
            &shared_policy(true),
        );
        assert!(hist.is_empty(), "an Event legacy column must not seed a row");
    }
}
