use serenity::all::*;
use super::super::config::Init;
use super::super::utils::Color;

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
        let color = if severity { Color::Red.throw() } else { Color::Yellow.throw() };

        let mut clean_err = self.err.clone();
        if clean_err.starts_with("Custom(\"") && clean_err.ends_with("\")") {
            clean_err = clean_err[8..clean_err.len() - 2].to_string();
            clean_err = clean_err.replace("\\\"", "\"").replace("\\n", "\n");
        } else if clean_err.starts_with("Custom(") && clean_err.ends_with(")") && clean_err.len() > 7 {
            clean_err = clean_err[7..clean_err.len() - 1].to_string();
        }

        let mut embed = CreateEmbed::new()
            .author(CreateEmbedAuthor::new(&self.usr.name).icon_url(self.usr.face()))
            .title("⚠️ System Intervention")
            .description(format!("The command could not be completed.\n\n**🔍 Reason:**\n```\n{}\n```", clean_err))
            .field("⚙️ Command Executed", format!("`/{}`", self.on), false)
            .color(color);

        if !self.advice.trim().is_empty() {
            embed = embed.field("💡 Recommended Action", format!("> {}", self.advice), false);
        }
        embed
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
        EditInteractionResponse::new().embed(self.make_embed(severity))
    }
}