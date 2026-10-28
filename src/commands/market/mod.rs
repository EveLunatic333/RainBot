use crate::AppReg;
use serenity::all::CommandOptionType;
use serenity::builder::{CreateCommand, CreateCommandOption};

pub mod bar;
pub mod market;
pub mod meal;
pub mod trading;

pub fn reg() -> Vec<CreateCommand> {
    let market_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "market", "Server market trading center");
    let bar_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "bar", "The trading bar to request custom items from the bartender");
    let restaurant_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "restourant", "Restaurant to order guild food")
        .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "food", "The food you want to order").set_autocomplete(true).required(true))
        .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "duration", "The food duration in hours (Max: 1440 hours)").required(true).max_int_value(1440).min_int_value(1));
    let casino_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "casino", "Casino to exchange for gacha tickets")
        .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "ticket", "The amount of tickets you want to buy").required(true));
    let guild_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "guild", "Guild trade to buy Guild RP")
        .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "rp", "The amount of RP you want to buy").required(true));
    let jewelry_opt = CreateCommandOption::new(CommandOptionType::SubCommand, "jewelry", "Jewelry store to exchange for premium gacha points")
        .add_sub_option(CreateCommandOption::new(CommandOptionType::Integer, "amount", "The amount of points you want to buy").required(true));

    let stall = AppReg::normal_slash("trading", "Server trading center to get the items you want")
        .add_option(market_opt)
        .add_option(bar_opt)
        .add_option(restaurant_opt)
        .add_option(casino_opt)
        .add_option(jewelry_opt)
        .add_option(guild_opt);

    vec![stall]
}