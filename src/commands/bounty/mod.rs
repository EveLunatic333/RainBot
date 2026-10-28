use crate::AppReg;
use serenity::all::CommandOptionType;
use serenity::builder::{CreateCommand, CreateCommandOption};

pub mod submit;
pub mod pedia;
pub mod cooldown;

pub fn reg() -> Vec<CreateCommand> {
    let bounty_type_opt = CreateCommandOption::new(CommandOptionType::String, "bounty", "Select the bounty type to manage")
        .required(true)
        .add_string_choice("BBQ01", "1")
        .add_string_choice("BBQ02", "2")
        .add_string_choice("BBQ03", "3")
        .add_string_choice("BBQ04", "4")
        .add_string_choice("BBQ05", "5")
        .add_string_choice("BBQ06", "6")
        .add_string_choice("BBQ07", "7")
        .add_string_choice("BBQ08", "8")
        .add_string_choice("BBQ09", "9")
        .add_string_choice("BBQ10", "10")
        .add_string_choice("BBQ11", "11")
        .add_string_choice("BBQ12", "12")
        .add_string_choice("BBQ13", "13")
        .add_string_choice("BBQ14", "14")
        .add_string_choice("BBQ15", "15")
        .add_string_choice("BBQ16", "16")
        .add_string_choice("BBQ17", "17")
        .add_string_choice("BBQ18", "18")
        .add_string_choice("BBQ19", "19")
        .add_string_choice("BBQ20", "20")
        .add_string_choice("BBQ21", "21")
        .add_string_choice("BBQ22", "22")
        .add_string_choice("BBQ23", "23")
        .add_string_choice("BBQ24", "24")
        .add_string_choice("BBQ25", "25");

    let value_opt = CreateCommandOption::new(CommandOptionType::Integer, "value", "Set the new cooldown value (in seconds)")
        .required(true);

    let sub_bounty = AppReg::subcommand("bounty", "Manage specific bounty cooldown")
        .add_sub_option(bounty_type_opt)
        .add_sub_option(value_opt);

    vec![
        AppReg::normal_slash("bounty", "Bounty Management System")
            .add_option(
                AppReg::subcommand("submit", "Submit a new bounty for review")
                    .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "image", "Upload your quest completion screenshot").required(true))
                    .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "image2", "Additional image (Optional)").required(false))
                    .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "image3", "Additional image (Optional)").required(false))
                    .add_sub_option(CreateCommandOption::new(CommandOptionType::Attachment, "image4", "Additional image (Optional)").required(false))
                    .add_sub_option(CreateCommandOption::new(CommandOptionType::String, "mentions", "Mention participants (e.g. @user1 @user2)").required(false))
            )
            .add_option(AppReg::subcommand("pedia", "View bounty list and requirements")),
            
        AppReg::normal_slash("cooldown", "Check or manage bounty cooldowns")
            .add_option(AppReg::subcommand("user", "Check user cooldown").add_sub_option(CreateCommandOption::new(CommandOptionType::User, "user", "User to check").required(true)))
            .add_option(AppReg::subcommand("refresh", "Refresh cooldown message"))
            .add_option(AppReg::subcommand("interface", "Spawn cooldown interface"))
            .add_option(sub_bounty), 

        AppReg::normal_slash("distribution", "Distribute rewards manually")
            .add_option(CreateCommandOption::new(CommandOptionType::String, "mentions", "Mention targets").required(true)),
    ]
}