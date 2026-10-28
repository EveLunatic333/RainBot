use crate::AppReg;
use serenity::all::CommandOptionType;
use serenity::builder::{CreateCommand, CreateCommandOption};

pub mod create;
pub mod change_pasword;

pub fn reg(_init: &crate::reusable::config::Init) -> Vec<CreateCommand> {
    let mut out = Vec::new();
    out.push(AppReg::normal_slash("create", "Create an MHFZ account for this server"));
    out.push(AppReg::normal_slash("check", "Check your user data on the server"));
    out.push(AppReg::normal_slash("switch", "Switch your active character for Discord events"));
    out.push(AppReg::normal_slash("bind", "Bind your existing MHFZ account to this server"));

    let pass = AppReg::normal_slash("change_password", "Change your account password")
        .add_option(CreateCommandOption::new(CommandOptionType::String, "password", "Your new password").required(true));

    let psn = AppReg::normal_slash("add_psn", "Update your PSN ID for your registered account")
        .add_option(CreateCommandOption::new(CommandOptionType::String, "psn_id", "Your PSN ID").required(true));

    out.push(pass);
    out.push(psn);
    out
}