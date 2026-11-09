//! Rich, user-facing notices for gate failures.
//!
//! A blocked action used to be a single line of prose — "must finish Bronze
//! BBQ21 first (progress 0/21)" — which named neither the next step nor how far
//! along the player was. Worse, three unrelated failures shared one headline,
//! so a progression lock was announced as a cooldown.
//!
//! This module turns the same facts into a structured embed: a headline naming
//! the actual obstacle, progress towards the requirement as a bar, the concrete
//! next step, and — when time is the obstacle — when the wait ends.
//!
//! Rendering lives here rather than in the evaluators so the rule stays pure and
//! unit-testable (`bounty_cooldown`), while presentation stays in one place.
//! `Notice::view()` is the seam tests assert on: it exposes the finished title,
//! description and fields without depending on serenity's builder internals.

use serenity::builder::{
    CreateActionRow, CreateEmbed, CreateEmbedFooter, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};
use serenity::all::Colour;

use super::super::utils::Color;

/// Number of cells in a rendered progress bar.
pub const BAR_WIDTH: usize = 12;

/// Discord's own limits, enforced before sending so an embed is never rejected.
const TITLE_LIMIT: usize = 256;
const DESCRIPTION_LIMIT: usize = 4096;
const FIELD_NAME_LIMIT: usize = 256;
const FIELD_VALUE_LIMIT: usize = 1024;
const FOOTER_LIMIT: usize = 2048;

// Box-drawing glyphs, written as escapes so no edit can turn them into a
// look-alike character that silently breaks alignment.
const CELL_FILLED: char = '\u{2593}'; // dark shade
const CELL_EMPTY: char = '\u{2591}'; // light shade

/// How loudly a notice speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Nothing is wrong; this is context for something that did happen.
    Info,
    /// The action was refused, but the player can clear it themselves.
    Warning,
    /// The action failed for a reason the player cannot fix.
    Error,
}

impl Severity {
    fn colour(self) -> Colour {
        match self {
            Self::Info => Color::Blue.throw(),
            Self::Warning => Color::Orange.throw(),
            Self::Error => Color::Red.throw(),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Info => "ℹ️",
            Self::Warning => "⚠️",
            Self::Error => "🛑",
        }
    }
}

/// One requirement and how far towards it the player is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressRow {
    /// Who this row is about, plus the requirement, e.g. `<@1> Bronze`.
    pub label: String,
    pub have: i64,
    pub need: i64,
}

impl ProgressRow {
    /// `i64` throughout so the same row can carry a bounty count or a coin
    /// balance; callers pass whatever integer they already hold.
    pub fn new(label: impl Into<String>, have: impl Into<i64>, need: impl Into<i64>) -> Self {
        Self {
            label: label.into(),
            have: have.into(),
            need: need.into(),
        }
    }

    /// Renders one line: a bar followed by the exact numbers.
    ///
    /// The bar is clamped to `need`, so a value above the goal shows a full bar
    /// rather than overflowing the row; the printed numbers stay honest.
    pub fn rendered(&self) -> String {
        format!("**{}**: `{}`", self.label, bar(self.have, self.need))
    }
}

/// Renders a progress bar body without the label.
///
/// `have` is clamped to `0..=need` so the cell count can never go out of range.
/// A `need` of `0` means the requirement is already met, which renders full.
pub fn bar(have: i64, need: i64) -> String {
    if need <= 0 {
        return format!("{} {}/{}", CELL_FILLED.to_string().repeat(BAR_WIDTH), 0, 0);
    }

    let reached = have.clamp(0, need);
    // A met requirement is a full bar, decided before any arithmetic: with a
    // balance near `i64::MAX`, computing the ratio would saturate and could
    // report a fraction of a bar for a requirement that is plainly satisfied.
    if reached >= need {
        return format!("{} {}/{}", CELL_FILLED.to_string().repeat(BAR_WIDTH), reached, need);
    }

    // Round half up so a bar never claims less progress than the numbers show.
    let numerator = reached.saturating_mul(BAR_WIDTH as i64).saturating_add(need / 2);
    let filled = ((numerator / need) as usize).min(BAR_WIDTH);

    format!(
        "{}{} {}/{}",
        CELL_FILLED.to_string().repeat(filled),
        CELL_EMPTY.to_string().repeat(BAR_WIDTH - filled),
        reached,
        need
    )
}

/// A finished, sendable notice.
///
/// Every part except the headline is optional: a one-line refusal stays a
/// one-line notice instead of being padded with empty headings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub headline: String,
    pub summary: String,
    pub cause: String,
    pub progress: Vec<ProgressRow>,
    pub details: Vec<String>,
    pub action: String,
    pub opens_at: Option<i64>,
    pub footer_text: Option<String>,
    pub severity: Severity,
}

impl Notice {
    pub fn new(headline: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            headline: headline.into(),
            summary: summary.into(),
            cause: String::new(),
            progress: Vec::new(),
            details: Vec::new(),
            action: String::new(),
            opens_at: None,
            footer_text: None,
            severity: Severity::Warning,
        }
    }

    /// The common shape: a refusal the reader can usually clear themselves.
    pub fn refusal(headline: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(headline, summary).severity(Severity::Warning)
    }

    /// A refusal that no user action can clear, e.g. a failed render.
    pub fn failure(headline: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(headline, summary).severity(Severity::Error)
    }

    /// Context for something that worked, or a harmless no-op.
    pub fn info(headline: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(headline, summary).severity(Severity::Info)
    }

    pub fn cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = cause.into();
        self
    }

    pub fn progress(mut self, row: ProgressRow) -> Self {
        self.progress.push(row);
        self
    }

    pub fn detail(mut self, line: impl Into<String>) -> Self {
        self.details.push(line.into());
        self
    }

    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = action.into();
        self
    }

    pub fn opens_at(mut self, when: i64) -> Self {
        self.opens_at = Some(when);
        self
    }

    /// Names the slash command in the footer.
    pub fn command(mut self, command: impl Into<String>) -> Self {
        self.footer_text = Some(format!("Command: {}", command.into()));
        self
    }

    /// Uses `text` verbatim as the footer, for actions driven by a button.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.footer_text = Some(text.into());
        self
    }

    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Builds the finished embed parts. Tests assert on this, not on serenity.
    pub fn view(&self) -> NoticeView {
        let mut fields: Vec<(String, String, bool)> = Vec::new();

        if !self.cause.trim().is_empty() {
            fields.push((
                "🔍 Cause".to_string(),
                quote_block(&clip(self.cause.trim(), FIELD_VALUE_LIMIT)),
                false,
            ));
        }

        if !self.progress.is_empty() {
            let body = self
                .progress
                .iter()
                .map(ProgressRow::rendered)
                .collect::<Vec<_>>()
                .join("\n");
            fields.push((
                "📊 Progress".to_string(),
                clip(&body, FIELD_VALUE_LIMIT),
                false,
            ));
        }

        if !self.details.is_empty() {
            let body = self
                .details
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            fields.push((
                "📋 Details".to_string(),
                clip(&body, FIELD_VALUE_LIMIT),
                false,
            ));
        }

        if !self.action.trim().is_empty() {
            fields.push((
                "✅ What to do".to_string(),
                quote_block(&clip(self.action.trim(), FIELD_VALUE_LIMIT)),
                false,
            ));
        }

        if let Some(when) = self.opens_at {
            fields.push((
                "⏳ Opens".to_string(),
                format!("<t:{when}:R>  ·  <t:{when}:f>"),
                false,
            ));
        }

        for (name, value, _) in &mut fields {
            *name = clip(name, FIELD_NAME_LIMIT);
            // Re-clip after adding quoting/prefixes, which grow the value.
            *value = clip(value, FIELD_VALUE_LIMIT);
        }

        NoticeView {
            title: clip(&format!("{} {}", self.severity.icon(), self.headline), TITLE_LIMIT),
            description: clip(self.summary.trim(), DESCRIPTION_LIMIT),
            fields,
            colour: self.severity.colour(),
            footer: self
                .footer_text
                .as_ref()
                .map(|text| clip(text, FOOTER_LIMIT)),
        }
    }

    pub fn to_embed(&self) -> CreateEmbed {
        let view = self.view();
        let mut embed = CreateEmbed::new()
            .title(view.title)
            .description(view.description)
            .color(view.colour);
        for (name, value, inline) in view.fields {
            embed = embed.field(name, value, inline);
        }
        if let Some(footer) = view.footer {
            embed = embed.footer(CreateEmbedFooter::new(footer));
        }
        embed
    }

    /// A fresh reply, public or ephemeral.
    pub fn to_interaction_response(&self, ephemeral: bool) -> CreateInteractionResponse {
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(self.to_embed())
                .ephemeral(ephemeral),
        )
    }

    /// Replacement for a message already on screen.
    ///
    /// Components are cleared deliberately: a notice replaces the previous
    /// screen, and leaving its buttons attached produces widgets that look live
    /// but no longer have a collector behind them.
    pub fn to_edit_response(&self) -> EditInteractionResponse {
        EditInteractionResponse::new()
            .embed(self.to_embed())
            .components(Vec::new())
    }

    /// Replacement that keeps working buttons on the message.
    ///
    /// Used by flows where the notice is only a status line and the judge is
    /// expected to act on the original buttons afterwards — Cancel / timeout in
    /// the bounty review, where the Approve/Reject/Check row must survive or the
    /// pending submission becomes unreviewable.
    pub fn to_edit_response_with(&self, components: Vec<CreateActionRow>) -> EditInteractionResponse {
        EditInteractionResponse::new()
            .embed(self.to_embed())
            .components(components)
    }

    /// A one-paragraph version for paths that can only return an error string.
    ///
    /// `Hunter::new` reports its refusals through `MyErr::Custom`, which the
    /// generic error screen prints as a single reason. Rendering the headline,
    /// the bars and the next step into that string keeps those refusals as
    /// informative as the embed, without teaching the error type about layout.
    pub fn to_plain_text(&self) -> String {
        let mut out = self.headline.clone();
        if !self.summary.trim().is_empty() {
            out.push('\n');
            out.push_str(self.summary.trim());
        }
        for row in &self.progress {
            out.push('\n');
            out.push_str(&row.rendered());
        }
        if let Some(when) = self.opens_at {
            out.push_str(&format!("\nOpens <t:{when}:R>"));
        }
        if !self.action.trim().is_empty() {
            out.push('\n');
            out.push_str(self.action.trim());
        }
        out
    }
}

/// The finished embed parts, independent of the Discord builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeView {
    pub title: String,
    pub description: String,
    pub fields: Vec<(String, String, bool)>,
    pub colour: Colour,
    pub footer: Option<String>,
}

impl NoticeView {
    /// Value of the field with this heading, if it was rendered.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, v, _)| v.as_str())
    }

    /// Every rendered text, for assertions that must not care about placement.
    pub fn all_text(&self) -> String {
        let mut out = format!("{}\n{}\n", self.title, self.description);
        for (name, value, _) in &self.fields {
            out.push_str(name);
            out.push('\n');
            out.push_str(value);
            out.push('\n');
        }
        if let Some(footer) = &self.footer {
            out.push_str(footer);
        }
        out
    }
}

/// Prefixes every line with `> ` so the text renders as a quote block.
///
/// A code fence would suppress mentions and `<t:...>` timestamps, which are
/// exactly what these notices need to show; a quote keeps both live.
fn quote_block(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if !out.is_empty() {
            out.push('\n');
        }
        if line.trim().is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }
    out
}

/// Shortens `text` to at most `max` characters, marking the cut with an ellipsis.
///
/// Counts characters, not bytes: Discord's limits are in characters and slicing
/// a multi-byte glyph by byte would panic.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_progress_bar_is_empty() {
        let rendered = bar(0, 21);
        assert_eq!(rendered.matches(CELL_FILLED).count(), 0);
        assert_eq!(rendered.matches(CELL_EMPTY).count(), BAR_WIDTH);
        assert!(rendered.ends_with("0/21"), "got {rendered}");
    }

    #[test]
    fn a_finished_requirement_fills_the_bar() {
        let rendered = bar(21, 21);
        assert_eq!(rendered.matches(CELL_FILLED).count(), BAR_WIDTH);
        assert_eq!(rendered.matches(CELL_EMPTY).count(), 0);
        assert!(rendered.ends_with("21/21"), "got {rendered}");
    }

    #[test]
    fn a_partial_bar_rounds_to_the_nearest_cell() {
        // 10/21 is 47.6% of 12 cells, which rounds to 6.
        assert_eq!(bar(10, 21).matches(CELL_FILLED).count(), 6);
        // 5/21 is 23.8%, which rounds to 3.
        assert_eq!(bar(5, 21).matches(CELL_FILLED).count(), 3);
    }

    #[test]
    fn a_bar_never_overflows_when_the_counter_passes_the_goal() {
        let rendered = bar(99, 21);
        assert_eq!(rendered.matches(CELL_FILLED).count(), BAR_WIDTH);
        assert_eq!(rendered.matches(CELL_EMPTY).count(), 0);
        // The printed numbers are clamped too, so the row stays readable.
        assert!(rendered.ends_with("21/21"), "got {rendered}");
    }

    #[test]
    fn a_negative_counter_renders_empty_rather_than_underflowing() {
        let rendered = bar(-5, 21);
        assert_eq!(rendered.matches(CELL_FILLED).count(), 0);
        assert!(rendered.ends_with("0/21"), "got {rendered}");
    }

    #[test]
    fn a_zero_goal_renders_full_without_dividing_by_zero() {
        let rendered = bar(0, 0);
        assert_eq!(rendered.matches(CELL_FILLED).count(), BAR_WIDTH);
        assert!(rendered.ends_with("0/0"), "got {rendered}");
    }

    #[test]
    fn a_huge_balance_bar_does_not_overflow_arithmetic() {
        // i64::MAX * BAR_WIDTH would wrap; the implementation must saturate.
        let rendered = bar(i64::MAX, i64::MAX);
        assert_eq!(rendered.matches(CELL_FILLED).count(), BAR_WIDTH);
    }

    #[test]
    fn a_money_row_shows_the_exact_balance_and_price() {
        let row = ProgressRow::new("Your balance", 1_000_i64, 2_500_i64);
        let rendered = row.rendered();
        assert!(rendered.contains("1,000/2,500") || rendered.contains("1000/2500"), "got {rendered}");
        assert_eq!(rendered.matches(CELL_FILLED).count(), 5);
    }

    #[test]
    fn mentions_and_timestamps_survive_rendering() {
        // A code fence would suppress both, which is the defect this replaces.
        let notice = Notice::new("Blocked", "Nothing to do yet.")
            .cause("<@42> must finish Bronze first")
            .opens_at(1_760_000_000);
        let view = notice.view();
        let text = view.all_text();
        assert!(text.contains("<@42>"), "mention was altered: {text}");
        assert!(text.contains("<t:1760000000:R>"), "timestamp missing: {text}");
        assert!(!text.contains("```"), "a fence would suppress both");
    }

    #[test]
    fn the_opens_field_appears_only_when_a_time_is_known() {
        let without = Notice::new("Blocked", "Now.").view();
        assert!(without.field("⏳ Opens").is_none());

        let with = Notice::new("Blocked", "Later.").opens_at(10).view();
        assert_eq!(with.field("⏳ Opens"), Some("<t:10:R>  ·  <t:10:f>"));
    }

    #[test]
    fn empty_optional_sections_are_omitted_entirely() {
        let view = Notice::new("Blocked", "Summary.").view();
        assert!(view.fields.is_empty(), "got {:?}", view.fields);
        assert!(view.footer.is_none());
        assert_eq!(view.title, "⚠️ Blocked");
    }

    #[test]
    fn severity_picks_the_icon_and_colour() {
        assert!(Notice::new("A", "x").severity(Severity::Error).view().title.starts_with("🛑"));
        assert!(Notice::new("A", "x").severity(Severity::Info).view().title.starts_with("ℹ️"));
        assert_eq!(
            Notice::new("A", "x").severity(Severity::Error).view().colour,
            Color::Red.throw()
        );
    }

    #[test]
    fn the_three_constructors_pick_the_right_severity() {
        assert_eq!(Notice::refusal("A", "x").severity, Severity::Warning);
        assert_eq!(Notice::failure("A", "x").severity, Severity::Error);
        assert_eq!(Notice::info("A", "x").severity, Severity::Info);
    }

    #[test]
    fn a_long_cause_is_clipped_to_the_field_limit() {
        let long = "x".repeat(FIELD_VALUE_LIMIT * 2);
        let view = Notice::new("Blocked", "Summary.").cause(long).view();
        let cause = view.field("🔍 Cause").expect("cause field");
        assert!(
            cause.chars().count() <= FIELD_VALUE_LIMIT,
            "cause was {} chars",
            cause.chars().count()
        );
        assert!(cause.ends_with('…'), "a cut must be visible");
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        // Would panic on a byte slice: every glyph here is multi-byte.
        let clipped = clip("╔╦╗╚╩╝", 4);
        assert_eq!(clipped.chars().count(), 4);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn progress_rows_keep_the_label_beside_the_bar() {
        let row = ProgressRow::new("<@7> Bronze", 3, 21);
        let rendered = row.rendered();
        assert!(rendered.starts_with("**<@7> Bronze**:"), "got {rendered}");
        assert!(rendered.contains("3/21"), "got {rendered}");
    }

    #[test]
    fn a_multi_line_cause_is_quoted_line_by_line() {
        let view = Notice::new("Blocked", "Summary.")
            .cause("first line\n\nthird line")
            .view();
        let cause = view.field("🔍 Cause").expect("cause field");
        assert_eq!(cause, "> first line\n>\n> third line");
    }

    #[test]
    fn plain_text_keeps_the_bar_the_wait_and_the_next_step() {
        let notice = Notice::new("Bounty Locked", "Finish the tier.")
            .progress(ProgressRow::new("<@1> Bronze", 3, 21))
            .opens_at(1_760_000_000)
            .action("Wait, then submit again.");
        let text = notice.to_plain_text();
        assert!(text.starts_with("Bounty Locked"), "got {text}");
        assert!(text.contains("3/21"), "the bar must survive: {text}");
        assert!(text.contains("<t:1760000000:R>"), "the wait must survive: {text}");
        assert!(text.contains("Wait, then submit again."), "got {text}");
    }

    #[test]
    fn plain_text_omits_the_sections_it_does_not_have() {
        let text = Notice::new("Blocked", "Summary.").to_plain_text();
        assert!(!text.contains("Opens"), "got {text}");
        assert_eq!(text, "Blocked\nSummary.");
    }

    #[test]
    fn the_command_helper_and_the_footer_helper_set_different_text() {
        let as_command = Notice::new("A", "x").command("/submit").view();
        assert_eq!(as_command.footer.as_deref(), Some("Command: /submit"));

        let as_footer = Notice::new("A", "x").footer("Party button").view();
        assert_eq!(as_footer.footer.as_deref(), Some("Party button"));
    }

    #[test]
    fn the_with_variant_accepts_rows_without_losing_the_embed() {
        // The Cancel path in the bounty review depends on this contract: the
        // notice travels as the embed while the Approve/Reject/Check row stays
        // attached. Both builders must accept the same notice unchanged.
        let notice = Notice::new("Review Cancelled", "Nothing was decided.");
        let plain = notice.to_edit_response();
        let with_rows = notice.to_edit_response_with(Vec::new());
        let _ = (plain, with_rows);
    }
}
