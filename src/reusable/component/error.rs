use serenity::all::*;
use super::super::config::Init;
use super::super::utils::Color;

/// Discord's limit for an embed description, in characters.
const DESCRIPTION_LIMIT: usize = 4096;
/// Discord's limit for a single embed field value, in characters.
const FIELD_VALUE_LIMIT: usize = 1024;
/// Discord's limit for an embed footer, in characters.
const FOOTER_LIMIT: usize = 2048;

#[derive(Clone)]
pub struct ErrorLog<'a> {
    pub(crate) err: String,
    pub(crate) on: String,
    pub(crate) advice: String,
    pub(crate) ctx: &'a Context,
    pub(crate) init: &'a Init,
    pub(crate) usr: &'a User,
}

impl<'a> ErrorLog<'a> {
    pub async fn new(ctx: &'a Context, init: &'a Init, usr: &'a User) -> ErrorLog<'a> {
        ErrorLog {
            err: String::new(),
            on: String::new(),
            advice: String::new(),
            ctx,
            init,
            usr,
        }
    }

    pub fn change_error(&mut self, error: String, on: String, advice: String) {
        self.err = error;
        self.on = on;
        self.advice = advice;
    }

    pub fn make_embed(&self, severity: bool) -> CreateEmbed {
        let colour = if severity { Color::Red.throw() } else { Color::Orange.throw() };

        // No claim is made about whether state changed: some failures happen after
        // the work committed, so "nothing was changed" would be false there.
        let mut embed = CreateEmbed::new()
            .author(CreateEmbedAuthor::new(&self.usr.name).icon_url(self.usr.face()))
            .title(if severity {
                "🛑 Command Failed"
            } else {
                "⚠️ Request Rejected"
            })
            .field(
                "🔍 Reason",
                clip(&quote_block(&plain_error(&self.err)), FIELD_VALUE_LIMIT),
                false,
            )
            .color(colour);

        if !self.advice.trim().is_empty() {
            embed = embed.field(
                "💡 Recommended Action",
                clip(&quote_block(self.advice.trim()), FIELD_VALUE_LIMIT),
                false,
            );
        }

        embed.footer(CreateEmbedFooter::new(clip(&command_footer(&self.on), FOOTER_LIMIT)))
    }

    pub async fn log_error_channel(&self, severity: bool) {
        let ch_id = ChannelId::new(self.init.log_channel.err_channel);
        let msg = CreateMessage::new()
            .embed(self.make_embed(severity))
            .content(format!("Report for <@{}>", self.usr.id));
        let _ = ch_id.send_message(&self.ctx.http, msg).await;
    }

    pub fn interaction_response(&self, ephemeral: bool, severity: bool) -> CreateInteractionResponse {
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(self.make_embed(severity))
                .ephemeral(ephemeral),
        )
    }

    pub fn defer_response(&self, severity: bool) -> EditInteractionResponse {
        // Clearing components is deliberate: an error embed replaces the previous
        // screen, and leaving its buttons/selects attached produces a message whose
        // widgets look live but no longer have a collector behind them. Pressing one
        // then fails to acknowledge within Discord's 3-second window.
        EditInteractionResponse::new()
            .embed(self.make_embed(severity))
            .components(Vec::new())
    }
}

/// The failure text with its `Variant(...)` wrapper removed.
///
/// `MyErr::get` wraps custom messages as `Custom("...")` so the variant stays
/// visible in a log line. A player should never see that wrapper, so it is
/// stripped here — and anything that does not look like a wrapper is returned
/// untouched rather than risk reporting a mangled reason.
fn plain_error(raw: &str) -> String {
    let text = raw.trim();
    let Some(open) = text.find('(') else {
        return text.to_string();
    };
    if !text.ends_with(')') || open == 0 {
        return text.to_string();
    }

    let variant = &text[..open];
    if !variant.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return text.to_string();
    }

    let mut inner = &text[open + 1..text.len() - 1];
    if inner.len() >= 2 && inner.starts_with('"') && inner.ends_with('"') {
        inner = &inner[1..inner.len() - 1];
    }
    inner.replace("\\\"", "\"").replace("\\n", "\n")
}

/// The footer naming what failed.
///
/// The macro wrapper passes the handler's own function name (`slash`, `button`),
/// which is not a command name. Other callers pass an internal step label such as
/// `Auth Check`. Only a lowercase, space-free name can be a Discord command, so
/// everything else is reported as the step it was, never as `/Auth Check`.
fn command_footer(on: &str) -> String {
    let name = on.trim();
    if name.is_empty()
        || matches!(name, "slash" | "userr" | "button" | "modal" | "auto" | "all" | "msg")
    {
        return "Command: not recorded".to_string();
    }
    let looks_like_command = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if looks_like_command {
        format!("Command: /{name}")
    } else {
        format!("Step: {name}")
    }
}

/// Prefixes every line with `> ` so the text renders as a quote block.
///
/// A code fence would suppress mentions and `<t:...>` timestamps, and several
/// rejection messages carry both — a fenced timestamp is shown to the player as
/// raw markup instead of a live clock.
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

/// Shortens `text` to at most `max` characters.
///
/// Counts characters, not bytes: Discord's limits are in characters, and slicing
/// a multi-byte glyph by byte would panic. Without this, one long database error
/// makes the whole embed exceed its limit and Discord rejects the message.
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
    fn the_variant_wrapper_is_stripped_from_the_reason() {
        assert_eq!(plain_error("Custom(\"plain text\")"), "plain text");
        assert_eq!(
            plain_error("ByteWise(\"connection refused\")"),
            "connection refused"
        );
    }

    #[test]
    fn a_message_without_a_wrapper_is_left_alone() {
        assert_eq!(plain_error("already plain"), "already plain");
    }

    #[test]
    fn an_unbalanced_message_is_left_alone() {
        // Truncated or unexpected text must not be mangled into a wrong claim.
        assert_eq!(plain_error("Custom(\"cut"), "Custom(\"cut");
    }

    #[test]
    fn escaped_quotes_and_newlines_are_restored() {
        assert_eq!(plain_error("Custom(\"a \\\"b\\\"\\nc\")"), "a \"b\"\nc");
    }

    #[test]
    fn an_internal_handler_name_is_never_shown_as_a_command() {
        // The macro wrapper passes the handler's own function name, which is not
        // a command a player can type.
        for internal in ["slash", "button", "modal", "userr", "auto", "all", ""] {
            assert_eq!(
                command_footer(internal),
                "Command: not recorded",
                "leaked {internal:?}"
            );
        }
    }

    #[test]
    fn a_real_command_name_is_shown() {
        assert_eq!(command_footer("submit"), "Command: /submit");
        assert_eq!(command_footer("huntercard"), "Command: /huntercard");
    }

    #[test]
    fn an_internal_step_label_is_reported_as_a_step_not_a_command() {
        // `Reg::check` reports "Auth Check"; it is not a command a player can type.
        assert_eq!(command_footer("Auth Check"), "Step: Auth Check");
    }

    #[test]
    fn a_multi_line_reason_is_quoted_line_by_line() {
        assert_eq!(quote_block("one\ntwo"), "> one\n> two");
        assert_eq!(quote_block("one\n\ntwo"), "> one\n>\n> two");
    }

    #[test]
    fn a_reason_is_never_fenced() {
        // A fence suppresses mentions and timestamps, which several rejection
        // messages carry.
        assert!(!quote_block("until <t:1:R> <@2>").contains("```"));
    }

    #[test]
    fn a_very_long_reason_is_clipped_to_the_field_limit() {
        let long = format!("Custom(\"{}\")", "x".repeat(FIELD_VALUE_LIMIT * 3));
        let clipped = clip(&quote_block(&plain_error(&long)), FIELD_VALUE_LIMIT);
        assert_eq!(clipped.chars().count(), FIELD_VALUE_LIMIT);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        let clipped = clip("╔╦╗╚╩╝", 4);
        assert_eq!(clipped.chars().count(), 4);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn a_reason_under_the_limit_is_untouched() {
        assert_eq!(clip("short", 10), "short");
    }
}
