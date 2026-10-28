use super::{MyErr, Mytrait};
use super::error::ErrorLog;
use serenity::all::*;

impl MyErr {
    pub(super) fn get(&self) -> String {
        match self {
            MyErr::Custom(x) => format!("Custom(\"{}\")", x),
            MyErr::ByteWise(x) => {
                let msg = x.to_string();
                if msg.contains("duplicate key") || msg.contains("23505") {
                    if msg.contains("users_username_key") {
                        return "Database Error: Username already exists!".to_string();
                    }
                    return "Database Error: Duplicate data detected!".to_string();
                }
                format!("ByteWise(\"{}\")", msg)
            }
            MyErr::Utf8(x) => format!("Utf8(\"{}\")", x),
            MyErr::Tokio(x) => format!("Tokio(\"{}\")", x),
            MyErr::Serde(x) => format!("Serde(\"{}\")", x),
            MyErr::Serenity(x) => format!("Serenity(\"{}\")", x),
            MyErr::Image(x) => format!("Image(\"{}\")", x),
            MyErr::Sqlx(x) => format!("Sqlx(\"{}\")", x),
        }
    }

    pub(super) fn severity(&self) -> bool {
        match self {
            MyErr::Custom(_) => false,
            MyErr::ByteWise(_) => true,
            MyErr::Utf8(_) => false,
            MyErr::Tokio(_) => true,
            MyErr::Serde(_) => false,
            MyErr::Serenity(_) => false,
            MyErr::Image(_) => true,
            MyErr::Sqlx(_) => true,
        }
    }

    pub(super) fn advice(&self) -> String {
        match self {
            MyErr::Custom(msg) => {
                let lower = msg.to_lowercase();
                if lower.contains("not registered")
                    || lower.contains("bind")
                    || lower.contains("no account")
                {
                    "You do not have a registered account yet. Please run the /bind command to register your account first!".to_string()
                } else {
                    "An operation failed. Please read the error message carefully. If this is a bug, report it to the admin.".to_string()
                }
            }
            MyErr::ByteWise(_) => {
                "Database connection error or duplicate data format. Please check your inputs.".to_string()
            }
            MyErr::Utf8(_) => {
                "Parsing error while analyzing text. Are you sure the input is valid?".to_string()
            }
            MyErr::Tokio(_) => {
                "File system or thread error. Please report this to the admin!".to_string()
            }
            MyErr::Serde(_) => {
                "Failed to operate with JSON data. The file might be broken or incorrectly formatted.".to_string()
            }
            MyErr::Serenity(_) => {
                "Discord API error. Discord might be lagging or the bot lacked permissions to complete the action.".to_string()
            }
            MyErr::Image(_) => {
                "Error while loading or processing the image. Make sure the image is valid.".to_owned()
            }
            MyErr::Sqlx(_) => {
                "Database query error. Please check your SQL queries and database connection.".to_string()
            }
        }
    }

    pub async fn log_channel(&self, err: &ErrorLog<'_>) {
        err.log_error_channel(self.severity()).await
    }

    pub async fn log_channel_ch(&self, err: &mut ErrorLog<'_>, on: &str) {
        err.change_error(self.get(), on.to_owned(), self.advice());
        err.log_error_channel(self.severity()).await
    }

    pub async fn log_msg(&self, msg: &Message, on: &str, err: &mut ErrorLog<'_>) {
        err.change_error(self.get(), on.to_owned(), self.advice());
        if let Err(why) = msg
            .channel_id
            .send_message(
                &err.ctx.http,
                CreateMessage::new().embed(err.make_embed(self.severity())),
            )
            .await
        {
            MyErr::from(why).log_channel(err).await
        }
    }

    pub async fn log_defer<T: Mytrait>(&self, cmd: &T, on: &str, err: &mut ErrorLog<'_>) {
        if is_benign_interaction_error(self) {
            tracing::warn!(command = on, "Expired interaction (10062/40060) ignored; no error shown to the user.");
            return;
        }
        err.change_error(self.get(), on.to_owned(), self.advice());
        cmd.err_defer(err).await;
    }

    pub async fn log<T: Mytrait>(&self, cmd: &T, on: &str, ephemeral: bool, err: &mut ErrorLog<'_>) {
        if is_benign_interaction_error(self) {
            tracing::warn!(command = on, "Expired interaction (10062/40060) ignored; no error shown to the user.");
            return;
        }
        err.change_error(self.get(), on.to_owned(), self.advice());
        cmd.err_response(err, ephemeral).await;
    }
}

/// Returns true for Discord interaction-lifecycle errors that are expected
/// noise rather than real failures: 10062 "Unknown interaction" (the
/// interaction expired or was answered too late) and 40060 "Interaction has
/// already been acknowledged" (a duplicate acknowledgement). These happen
/// routinely after bot restarts or when a user cancels, and must never
/// surface a scary error embed or flood the log.
pub fn is_benign_interaction_error(err: &MyErr) -> bool {
    match err {
        MyErr::Serenity(serenity::Error::Http(http_err)) => matches!(
            *http_err,
            serenity::http::HttpError::UnsuccessfulRequest(ref resp)
                if resp.error.code == 10062 || resp.error.code == 40060
        ),
        _ => false,
    }
}