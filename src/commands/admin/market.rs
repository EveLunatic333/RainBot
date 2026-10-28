use serenity::all::*;
use crate::reusable::bitwise::ItemCode;
use crate::{Components, ItemPedia, MyErr, Reg, SlashBundle};

pub enum SubCommand {
    Item(String),
    Head(String),
    Arms(String),
    Waist(String),
    Chest(String),
    Leg(String),
    Melee(String),
    Ranged(String),
}

struct MarketHandle {
    data: SubCommand,
    user: UserId,
    count: u16,
    price: Option<i32>,
}

impl MarketHandle {
    fn new(cmd: &CommandInteraction) -> Result<MarketHandle, MyErr> {
        let err = MyErr::Custom("invalid input, please check your input especially on unit".to_string());
        for sub in &cmd.data.options {
            if let CommandDataOptionValue::SubCommand(cmx) = &sub.value {
                let mut user = UserId::default();
                let mut count = 1;
                let mut price = None;
                let mut data = SubCommand::Item("0000".to_owned());
                for opt in cmx {
                    match &opt.value {
                        CommandDataOptionValue::User(x) => user = *x,
                        CommandDataOptionValue::Integer(x) => {
                            if opt.name == "price" {
                                price = Some(i32::try_from(*x).map_err(|_| MyErr::Custom("invalid input".to_string()))?);
                            } else {
                                count = u16::try_from(*x).map_err(|_| MyErr::Custom("invalid input".to_string()))?;
                                if count == 0 { return Err(err); }
                            }
                        }
                        CommandDataOptionValue::String(value) => {
                            data = match sub.name.as_str() {
                                "item" => SubCommand::Item(value.clone()),
                                "head" => SubCommand::Head(value.clone()),
                                "arms" => SubCommand::Arms(value.clone()),
                                "waist" => SubCommand::Waist(value.clone()),
                                "chest" => SubCommand::Chest(value.clone()),
                                "leg" => SubCommand::Leg(value.clone()),
                                "melee" => SubCommand::Melee(value.clone()),
                                "ranged" => SubCommand::Ranged(value.clone()),
                                _ => continue,
                            }
                        }
                        _ => continue,
                    }
                }
                return Ok(MarketHandle { data, user, count, price });
            }
        }
        Err(err)
    }

    fn code(&self) -> ItemCode {
        ItemCode {
            name: None,
            key: self.data.value().to_owned(),
            count: self.count,
            types: self.data.code(),
        }
    }
}

impl SubCommand {
    fn new(cmd: &CommandInteraction) -> Result<SubCommand, MyErr> {
        for sub in &cmd.data.options {
            if let CommandDataOptionValue::SubCommand(cmx) = &sub.value {
                for opt in cmx {
                    if let CommandDataOptionValue::Autocomplete { kind: _, value } = &opt.value {
                        let value = value.to_owned();
                        let data = match sub.name.as_str() {
                            "item" => SubCommand::Item(value),
                            "head" => SubCommand::Head(value),
                            "arms" => SubCommand::Arms(value),
                            "waist" => SubCommand::Waist(value),
                            "chest" => SubCommand::Chest(value),
                            "leg" => SubCommand::Leg(value),
                            "melee" => SubCommand::Melee(value),
                            "ranged" => SubCommand::Ranged(value),
                            _ => continue,
                        };
                        return Ok(data);
                    }
                }
            }
        }
        Err(MyErr::Custom("cant get the data option".to_string()))
    }

    fn code(&self) -> u8 {
        match self {
            SubCommand::Leg(_) => 0,
            SubCommand::Head(_) => 1,
            SubCommand::Chest(_) => 2,
            SubCommand::Arms(_) => 3,
            SubCommand::Waist(_) => 4,
            SubCommand::Melee(_) => 5,
            SubCommand::Ranged(_) => 6,
            SubCommand::Item(_) => 7,
        }
    }

    fn value(&self) -> &String {
        match self {
            SubCommand::Leg(val) => val,
            SubCommand::Head(val) => val,
            SubCommand::Chest(val) => val,
            SubCommand::Arms(val) => val,
            SubCommand::Waist(val) => val,
            SubCommand::Melee(val) => val,
            SubCommand::Ranged(val) => val,
            SubCommand::Item(val) => val,
        }
    }

    pub fn predict(&self, pedia: &ItemPedia) -> Result<Vec<AutocompleteChoice>, MyErr> {
        let val = self.value();
        let item = pedia.types.get(&self.code()).ok_or_else(|| MyErr::Custom("category not found in pedia".to_string()))?;
        let out = item
            .iter()
            .filter(|(k, f)| {
                let flat_val = val.to_lowercase();
                let flat_tar = f.to_lowercase();
                flat_tar.starts_with(&flat_val)
                    || flat_tar.contains(&flat_val)
                    || k.starts_with(&val.to_uppercase())
                    || k.contains(&val.to_uppercase())
            })
            .map(|(&k, &v)| {
                let mut name = format!("{} - {}", k, v);
                if v.len() > 100 {
                    name = name[..90].to_owned();
                }
                serenity::builder::AutocompleteChoice::new(name.clone(), k)
            })
            .collect::<Vec<_>>();

        if out.is_empty() {
            return Err(MyErr::Custom("no result match for your input".to_string()));
        } else if out.len() > 15 {
            return Ok(out[..15].to_vec());
        }
        Ok(out)
    }
}

#[hertz::hertz_auto]
async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let option = SubCommand::new(bnd.cmd)?;
    let choice = option.predict(bnd.pedia)?;
    let content = CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(choice));
    Components::response_adv(bnd, content).await?;
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let mut is_distributor = false;
    if let Some(member) = &bnd.cmd.member {
        if member.roles.contains(&RoleId::new(bnd.init.server_role.weekly_distributor)) {
            is_distributor = true;
        }
    }
    let option = MarketHandle::new(bnd.cmd)?;

    if is_distributor && option.user == bnd.cmd.user.id {
        return Err(MyErr::Custom(
            "❌ ANTI-CHEAT PROTOCOL ACTIVATED\nIdentity Conflict: You are attempting to execute a distribution command to your own account.\nTo prevent abuse of authority and ensure administrative integrity, self-distribution is strictly prohibited.\nRules of Engagement:\nWeekly Distributors are authorized to provide logistics to other hunters only.\nSelf-rewarding operations are flagged as a violation of server policy.\nPlease designate a different recipient for this operation.".to_string()
        ));
    }

    if !is_distributor && !crate::commands::admin::execute_guard(bnd).await {
        return Ok(());
    }

    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(msg).ephemeral(true)),
        ).await;
        return Ok(());
    }

    let user = option.user.to_user(&bnd.ctx.http).await.map_err(|_| MyErr::Custom("Failed to fetch user info".to_string()))?;
    let mut reg = Reg::check(bnd, &user).await?;
    reg.pg.market(&option.code(), reg.cid, option.price, bnd.pedia).await?;
    Components::response(bnd, "sent distribution data", true).await?;
    reg.pg.close().await;
    Ok(())
}