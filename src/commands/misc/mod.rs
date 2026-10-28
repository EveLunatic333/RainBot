use crate::AppReg;
use serenity::all::CommandOptionType;
use serenity::builder::{CreateCommand, CreateCommandOption};

pub mod ferias;
pub mod gpt;
pub mod party;
pub mod role_btn;
pub mod tag;
pub mod active_feature;

pub fn reg() -> Vec<CreateCommand> {
    let ferias = AppReg::normal_slash("ferias", "link you a ferias item")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "item", "the item you wanna search")
                .set_autocomplete(true)
                .required(true),
        );

    let gpt = AppReg::normal_slash("gpt", "chat-gpt command")
        .add_option(
            AppReg::subcommand("chat", "chat with chat-gpt (using free token so expected rate limit)")
                .add_sub_option(AppReg::str_option("ask", "what you ask to chat-gpt").required(true)),
        )
        .add_option(
            AppReg::subcommand("image", "generate image with bot")
                .add_sub_option(
                    AppReg::int_option("quantity", "the number of image generated 1-10")
                        .min_int_value(1)
                        .max_int_value(10)
                        .required(true),
                )
                .add_sub_option(
                    AppReg::str_option("size", "the image size generated")
                        .required(true)
                        .add_string_choice("256x256", "256x256")
                        .add_string_choice("512x512", "512x512")
                        .add_string_choice("1024x1024", "1024x1024"),
                )
                .add_sub_option(
                    AppReg::str_option("description", "description of image you want to generate")
                        .required(true),
                ),
        );

    let party = AppReg::normal_slash("party", "Recruit players for a party/quest")
        .add_option(
            AppReg::str_option("quest", "Title of the quest you want to run").required(true),
        )
        .add_option(
            AppReg::str_option("location", "World/Land meeting point").required(true),
        )
        .add_option(
            AppReg::int_option("size", "Number of total party members")
                .required(true)
                .add_int_choice("2 Players", 2)
                .add_int_choice("3 Players", 3)
                .add_int_choice("4 Players", 4),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "role_mention", "Tag a specific role for this recruitment")
                .required(false)
                .add_string_choice("Raviente Raid", "raviente_raid")
                .add_string_choice("Party Helper", "party_helper")
                .add_string_choice("Bounty Hunter", "bounty_hunter")
                .add_string_choice("Road Runner", "road_runner")
                .add_string_choice("Gacha Enjoyer", "gacha_enjoyer")
                .add_string_choice("Musou Slayer", "musou_slayer"),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "comment", "Add optional notes (e.g., specific weapon required, chill run)")
                .required(false),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "join_mode", "How players join this party")
                .required(false)
                .add_string_choice("Open — anyone can join directly", "open")
                .add_string_choice("Approval — leader approves each request", "approval"),
        );

    let active_feature = AppReg::normal_slash("active_feature", "Check the currently active weapon feature for today");
    vec![ferias, gpt, party, active_feature]
}