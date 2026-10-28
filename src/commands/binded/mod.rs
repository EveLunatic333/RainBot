use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::all::CommandOptionType;
use crate::AppReg;

pub mod card;
pub mod save;
pub mod transfer;
pub mod event;
pub mod huntercard;

pub fn reg() -> Vec<CreateCommand> {
    let file = AppReg::normal_slash("transfer", "transfer your save data to server")
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file", "attach your savefile .bin").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file1", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file2", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file3", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file4", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file5", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file6", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file7", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file8", "attach your savefile .bin"))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "file9", "attach your savefile .bin"));

    let huntercard = AppReg::normal_slash("huntercard", "Manage and display your visual Hunter Card")
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "edit", "Edit your Hunter Card images and note")
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "character", "Your character screenshot").required(false))
                .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "equipment", "Your equipment screenshot").required(false))
                .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "note", "Your hunter note").required(false))
                .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "title", "Your Hunter Title (Max 3 Words)").required(false))
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "show", "Show a Hunter Card")
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::User, "user", "Select a user to view their card (Optional)").required(false)
                )
        );

    vec![
        AppReg::normal_slash("card", "show your hunter card status"),
        AppReg::user_context("👤 Card"),
        AppReg::user_context("🎀 Event"),
        AppReg::normal_slash("event", "show my event data"),
        AppReg::normal_slash("dm_save", "make bot send you your save file"),
        file,
        huntercard,
    ]
}