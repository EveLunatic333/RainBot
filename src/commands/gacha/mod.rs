pub mod pull;

use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::all::CommandOptionType;
use crate::AppReg;
use std::fs;

pub fn reg() -> Vec<CreateCommand> {
    let mut single_cost: i64 = 10;
    let mut multi_cost: i64 = 110;

    if let Ok(file_content) = fs::read_to_string("./static/gacha.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&file_content) {
            if let Some(config) = json.get("config") {
                let parse_cost = |val: Option<&serde_json::Value>| -> Option<i64> {
                    val.and_then(|v| {
                        if let Some(i) = v.as_i64() {
                            Some(i)
                        } else if let Some(f) = v.as_f64() {
                            Some(f as i64)
                        } else {
                            None
                        }
                    })
                };

                if let Some(c) = parse_cost(config.get("cost_single")) {
                    single_cost = c;
                }
                if let Some(c) = parse_cost(config.get("cost_multi")) {
                    multi_cost = c;
                }
            }
        }
    }

    let single_desc = format!("Single pull with {} ticket(s)", single_cost);
    let multi_desc = format!("12 pulls with {} ticket(s)", multi_cost);

    let pull = AppReg::normal_slash("pull", "Pull gacha with gacha ticket")
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "single", &single_desc)
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "multi", &multi_desc)
        );
        
    vec![pull]
}