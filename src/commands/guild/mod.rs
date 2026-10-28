pub mod guild;
pub mod extend_food;

use serenity::all::CommandOptionType;
use serenity::builder::{CreateCommand, CreateCommandOption};

pub fn reg() -> Vec<CreateCommand> {
    let guild_option = CreateCommandOption::new(CommandOptionType::Integer, "guild", "Guild ID to search or join")
        .set_autocomplete(true)
        .required(true);

    let user_option = CreateCommandOption::new(CommandOptionType::User, "user", "Target user to perform operation on")
        .required(true);

    let extend_food_option = CreateCommandOption::new(
        CommandOptionType::SubCommand,
        "extend_food",
        "Extend duration of existing guild food (long-term commitment)"
    )
    .add_sub_option(
        CreateCommandOption::new(CommandOptionType::Integer, "target_guild", "Select the Guild first")
            .set_autocomplete(true)
            .required(true)
    )
    .add_sub_option(
        CreateCommandOption::new(CommandOptionType::Integer, "food", "The active food ID to extend")
            .set_autocomplete(true)
            .required(true)
    )
    .add_sub_option(
        CreateCommandOption::new(CommandOptionType::Integer, "days", "Extension duration (30 or 60 days)")
            .add_int_choice("30 Days (300,000 BC)", 30)
            .add_int_choice("60 Days (600,000 BC)", 60)
            .required(true)
    );

    vec![
        CreateCommand::new("guild")
            .description("Manage guild membership, view info, and manage guild food")
            .add_option(
                CreateCommandOption::new(CommandOptionType::SubCommand, "info", "View detailed information about a guild")
                    .add_sub_option(guild_option.clone()),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::SubCommand, "join", "Join a guild if you are not currently in one")
                    .add_sub_option(guild_option.clone()),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::SubCommand, "leave", "Leave your current guild"),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::SubCommand, "clear_app", "[ADMIN ONLY] Clear stuck in-game guild application for a user")
                    .add_sub_option(user_option),
            )
            .add_option(extend_food_option),
    ]
}