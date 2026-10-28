use crate::{MyErr, SlashBundle, ComponentBundle, ModalBundle, Components, Mybundle};
use serenity::all::*;
use crate::reusable::gpt::{Gpt, chat::CompModel};

#[hertz::hertz_slash_normal(0, true)]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, serenity::builder::CreateInteractionResponse::Message(
            serenity::builder::CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(()); 
    }

    let mut name = "";
    for data in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(_) = &data.value {
            name = &data.name;
        }
    }
    match name {
        "chat" => chat(bnd).await?,
        "image" => image(bnd).await?,
        _ => return Err(MyErr::Custom("we dont have that command".to_owned())),
    }
    Ok(())
}

#[hertz::hertz_button_normal(0, false)]
async fn button(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let id = get_id(bnd);
    let modal_resp = modal_response(&id);
    if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, modal_resp).await {
        if e.to_string().contains("40060") {
            return Ok(());
        }
        return Err(MyErr::from(e));
    }

    Ok(())
}

async fn modal(bnd: &ModalBundle<'_>) -> Result<(), MyErr> {
    let _ = bnd.cmd.defer(&bnd.ctx.http).await;
    
    let mut ask = String::new();
    for comp in &bnd.cmd.data.components {
        let arow = comp.components.first().unwrap();
        if let ActionRowComponent::InputText(input) = arow {
            ask = input.value.clone().unwrap_or_default();
        }
    }
    
    if !ask.is_empty() {
        let id = get_id(bnd);
        let gpt_s = Gpt::new(bnd.init)?;
        let data = CompModel::retrieve(&id).await?;
        let resp = gpt_s.reply_comp(&data, &ask).await?;
        resp.modal_send(bnd, &id).await?;
        Ok(())
    } else {
        Err(MyErr::Custom("no question asked".to_owned()))
    }
}

async fn chat(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, serenity::builder::CreateInteractionResponse::Message(
            serenity::builder::CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(()); 
    }

    let gpt_struct = Gpt::new(bnd.init)?;
    let mut name = "";
    let id = bnd.cmd.id.to_string();
    for i in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::String(x) = &i.value {
            name = x;
        }
    }
    if name == "" {
        return Err(MyErr::Custom("Unknown request".to_owned()));
    } else {
        let resp = gpt_struct.completion(name).await?;
        resp.send(bnd, &id).await?;
    }
    Ok(())
}

async fn image(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, serenity::builder::CreateInteractionResponse::Message(
            serenity::builder::CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(()); 
    }

    let mut prompt = "".to_owned();
    let mut size = "".to_owned();
    let mut n = 1;
    for x in Components::sub_options(bnd)? {
        match &x.value {
            CommandDataOptionValue::String(y) => {
                if x.name == "size" {
                    size = y.to_owned()
                } else {
                    prompt = y.to_owned()
                }
            }
            CommandDataOptionValue::Integer(y) => {
                n = *y as u8;
            }
            _ => {
                continue;
            }
        }
    }
    let gpt = Gpt::new(bnd.init)?;
    let img = gpt.get_image(prompt, n, size).await?;
    img.send(bnd).await?;
    Ok(())
}

fn get_id<T: Mybundle>(bnd: &T) -> String {
    let name = bnd.name();
    name.split("-").nth(1).unwrap().to_string()
}

fn modal_register_row(name: &str) -> CreateActionRow {
    let placeholder = "Your Reply".to_owned();
    CreateActionRow::InputText(
        CreateInputText::new(InputTextStyle::Paragraph, name, name)
            .required(true)
            .placeholder(&placeholder),
    )
}

fn modal_response(id: &str) -> CreateInteractionResponse {
    let name = format!("chat-{id}");
    CreateInteractionResponse::Modal(
        CreateModal::new(name, "Your Chat Response").components(vec![modal_register_row("username")]),
    )
}