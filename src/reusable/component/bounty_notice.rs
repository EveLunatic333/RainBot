//! Turns bounty gate failures into readable notices.
//!
//! The submit flow used to report every refusal the same way — a bullet list
//! under a "Cooldown Detected!" headline, even when the real obstacle was an
//! unfinished tier that no amount of waiting would clear. This module maps the
//! structured facts each gate already has onto a [`Notice`], so the reader is
//! told what stopped them, how far along they are, and what to do next.
//!
//! Kept separate from `bounty_cooldown` so the rule stays pure and free of
//! presentation, and separate from the command modules so the wording can be
//! unit-tested without a Discord client or a database.

use super::bounty::{BBQ, Category};
use super::bounty_cooldown::CooldownGroup;
use super::notice::{Notice, ProgressRow, Severity};

/// One reason a submission was refused, for one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// The previous tier of the ladder is not finished yet.
    Progression {
        user: String,
        required: Category,
        need: u8,
        have: i32,
    },
    /// A wait from an earlier completed bounty is still running.
    Cooldown {
        user: String,
        ends_at: i64,
        group: CooldownGroup,
        hours: i64,
        previous: Option<(Category, BBQ)>,
    },
    /// The requested BBQ number is above what this player has unlocked.
    Rank {
        user: String,
        category: Category,
        /// Highest BBQ currently allowed, e.g. `BBQ01`.
        allowed: u8,
        /// BBQ the player asked for.
        wanted: u8,
        /// Completions in this category today.
        have: i32,
        /// Completions needed before `wanted` unlocks.
        need: u8,
    },
    /// The administrator closed this bounty slot.
    SlotClosed { category: Category, bbq: BBQ },
}

impl Block {
    /// One line for the Details field.
    fn line(&self) -> String {
        match self {
            Self::Progression { user, required, need, have } => format!(
                "{user} has not finished {} yet ({have}/{need}).",
                required.name()
            ),
            Self::Cooldown { user, ends_at, group, hours, previous } => {
                let from = match previous {
                    Some((category, bbq)) => {
                        format!(" after {} {}", category.name(), bbq.name())
                    }
                    None => String::new(),
                };
                format!(
                    "{user} is on a {}-hour {} cooldown{from}; it ends <t:{ends_at}:R>.",
                    hours,
                    group.name()
                )
            }
            Self::Rank { user, category, allowed, wanted, .. } => format!(
                "{user} asked for {} {} but only {} is unlocked.",
                category.name(),
                BBQ::new(*wanted).map(|b| b.name()).unwrap_or_else(|_| format!("BBQ{wanted}")),
                BBQ::new(*allowed).map(|b| b.name()).unwrap_or_else(|_| format!("BBQ{allowed}"))
            ),
            Self::SlotClosed { category, bbq } => format!(
                "{} {} is closed for submission right now.",
                category.name(),
                bbq.name()
            ),
        }
    }

    fn progress(&self) -> Option<ProgressRow> {
        match self {
            Self::Progression { user, required, need, have } => Some(ProgressRow::new(
                format!("{user} {}", required.name()),
                *have,
                *need,
            )),
            Self::Rank { user, category, wanted, have, need, .. } => Some(ProgressRow::new(
                format!(
                    "{user} {} (unlocks {})",
                    category.name(),
                    BBQ::new(*wanted).map(|b| b.name()).unwrap_or_else(|_| format!("BBQ{wanted}"))
                ),
                *have,
                *need,
            )),
            _ => None,
        }
    }
}

/// Builds the notice for one or more refusals.
///
/// Mixed causes get a neutral headline and keep every cause in Details, because
/// a party of three can trip different gates at once.
pub fn blocked_submission(blocks: &[Block]) -> Notice {
    let has_progression = blocks.iter().any(|b| matches!(b, Block::Progression { .. }));
    let has_cooldown = blocks.iter().any(|b| matches!(b, Block::Cooldown { .. }));
    let has_rank = blocks.iter().any(|b| matches!(b, Block::Rank { .. }));
    let has_closed = blocks.iter().any(|b| matches!(b, Block::SlotClosed { .. }));

    let single_kind = [has_progression, has_cooldown, has_rank, has_closed]
        .iter()
        .filter(|x| **x)
        .count()
        == 1;

    let (headline, summary) = if single_kind && has_progression {
        (
            "Bounty Locked: Finish the Previous Tier",
            "The progression ladder has to be walked in order before this bounty opens.",
        )
    } else if single_kind && has_cooldown {
        (
            "Bounty Cooldown Still Running",
            "A bounty from the same group was completed recently, so a wait is still running.",
        )
    } else if single_kind && has_rank {
        (
            "BBQ Locked: Clear the Earlier Bounties First",
            "Bounty numbers unlock one at a time inside a category.",
        )
    } else if single_kind && has_closed {
        (
            "Bounty Slot Closed",
            "The administrator has closed this bounty for submissions right now.",
        )
    } else {
        (
            "Submission Blocked",
            "One or more requirements are not met yet.",
        )
    };

    let mut notice = Notice::new(headline, summary).severity(Severity::Warning);
    notice = notice.command("/submit");

    for block in blocks {
        notice = notice.detail(block.line());
        if let Some(row) = block.progress() {
            notice = notice.progress(row);
        }
    }

    if let Some(ends_at) = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Cooldown { ends_at, .. } => Some(*ends_at),
            _ => None,
        })
        .min()
    {
        notice = notice.opens_at(ends_at);
    }

    notice.action(action_for(has_progression, has_cooldown, has_rank, has_closed))
}

fn action_for(progression: bool, cooldown: bool, rank: bool, closed: bool) -> String {
    let mut steps: Vec<&str> = Vec::new();
    if progression {
        steps.push(
            "Finish the tier listed under Progress. The next tier opens the moment its role is granted.",
        );
    }
    if rank {
        steps.push(
            "Clear the unlocked BBQ number first; each completion unlocks the next number in order.",
        );
    }
    if cooldown {
        steps.push("Wait until the time shown under Opens, then submit again.");
    }
    if closed {
        steps.push("Ask an administrator to reopen this bounty before submitting it again.");
    }
    steps.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progression() -> Block {
        Block::Progression {
            user: "<@1>".to_string(),
            required: Category::Bronze,
            need: 21,
            have: 3,
        }
    }

    fn cooldown() -> Block {
        Block::Cooldown {
            user: "<@1>".to_string(),
            ends_at: 1_760_000_000,
            group: CooldownGroup::Progression,
            hours: 20,
            previous: Some((Category::Gold, BBQ::BBQ07)),
        }
    }

    fn rank() -> Block {
        Block::Rank {
            user: "<@2>".to_string(),
            category: Category::Bronze,
            allowed: 1,
            wanted: 21,
            have: 0,
            need: 20,
        }
    }

    #[test]
    fn a_progression_lock_is_not_announced_as_a_cooldown() {
        let view = blocked_submission(&[progression()]).view();
        assert!(view.title.contains("Finish the Previous Tier"), "got {}", view.title);
        assert!(
            !view.all_text().to_lowercase().contains("cooldown"),
            "a progression lock must never mention a cooldown: {}",
            view.all_text()
        );
    }

    #[test]
    fn a_progression_lock_shows_a_bar_for_the_missing_tier() {
        let view = blocked_submission(&[progression()]).view();
        let progress = view.field("📊 Progress").expect("progress field");
        assert!(progress.contains("<@1> Bronze"), "got {progress}");
        assert!(progress.contains("3/21"), "got {progress}");
    }

    #[test]
    fn a_cooldown_shows_hours_previous_bounty_and_the_opening_time() {
        let view = blocked_submission(&[cooldown()]).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("20-hour"), "got {details}");
        assert!(details.contains("after Gold BBQ07"), "got {details}");
        assert_eq!(view.field("⏳ Opens"), Some("<t:1760000000:R>  ·  <t:1760000000:f>"));
    }

    #[test]
    fn a_cooldown_offers_no_way_to_skip_but_names_the_wait() {
        let view = blocked_submission(&[cooldown()]).view();
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("Wait until the time shown"), "got {action}");
    }

    #[test]
    fn a_rank_lock_names_both_numbers_and_the_progress_needed() {
        let view = blocked_submission(&[rank()]).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("BBQ21"), "got {details}");
        assert!(details.contains("BBQ01"), "got {details}");
        let progress = view.field("📊 Progress").expect("progress field");
        assert!(progress.contains("0/20"), "got {progress}");
    }

    #[test]
    fn a_closed_slot_points_at_the_administrator_and_names_the_bounty() {
        let view =
            blocked_submission(&[Block::SlotClosed { category: Category::Free, bbq: BBQ::BBQ21 }])
                .view();
        assert!(view.title.contains("Bounty Slot Closed"), "got {}", view.title);
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("Free BBQ21"), "got {details}");
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("administrator"), "got {action}");
    }

    #[test]
    fn several_causes_at_once_use_a_neutral_headline_and_keep_every_line() {
        let view = blocked_submission(&[progression(), cooldown(), rank()]).view();
        assert!(view.title.contains("Submission Blocked"), "got {}", view.title);
        let details = view.field("📋 Details").expect("details field");
        let lines = details.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(lines, 3, "got {details}");
        // The earliest cooldown still drives the Opens field.
        assert_eq!(view.field("⏳ Opens"), Some("<t:1760000000:R>  ·  <t:1760000000:f>"));
    }

    #[test]
    fn mixed_causes_offer_every_applicable_step() {
        let view = blocked_submission(&[progression(), cooldown()]).view();
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("Finish the tier"), "got {action}");
        assert!(action.contains("Wait until the time shown"), "got {action}");
    }

    #[test]
    fn the_earliest_opening_wins_when_two_cooldowns_are_reported() {
        let later = Block::Cooldown {
            user: "<@3>".to_string(),
            ends_at: 1_900_000_000,
            group: CooldownGroup::Free,
            hours: 40,
            previous: None,
        };
        let view = blocked_submission(&[cooldown(), later]).view();
        assert_eq!(view.field("⏳ Opens"), Some("<t:1760000000:R>  ·  <t:1760000000:f>"));
    }

    #[test]
    fn no_cooldown_means_no_opens_field() {
        let view = blocked_submission(&[progression()]).view();
        assert!(view.field("⏳ Opens").is_none(), "got {:?}", view.fields);
    }

    #[test]
    fn every_notice_carries_the_submit_command_in_its_footer() {
        let view = blocked_submission(&[progression()]).view();
        assert_eq!(view.footer.as_deref(), Some("Command: /submit"));
    }

    #[test]
    fn mentions_survive_so_a_party_member_can_be_told_apart() {
        let view = blocked_submission(&[progression(), rank()]).view();
        let text = view.all_text();
        assert!(text.contains("<@1>"), "got {text}");
        assert!(text.contains("<@2>"), "got {text}");
    }
}
