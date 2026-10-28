use serenity::all::*;
use crate::{SlashBundle, MyErr, Components};
use serde::Deserialize;
use std::fs;
use std::sync::OnceLock;
use crate::reusable::utils::color;

#[derive(Deserialize)]
struct InterfaceJson {
    embeds: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    title: String,
    description: String,
    color: u32,
}

static INTERFACE_JSON: OnceLock<String> = OnceLock::new();

#[hertz::hertz_slash_normal(0, false)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
            )
        ).await;
        return Ok(());
    }

    let json_content = INTERFACE_JSON.get_or_init(|| {
        fs::read_to_string("static/interface.json").unwrap_or_else(|_| String::new())
    });

    let mut embeds = Vec::new();
    if let Ok(data) = serde_json::from_str::<InterfaceJson>(json_content) {
        for emb in data.embeds {
            embeds.push(CreateEmbed::new()
                .title(emb.title)
                .description(emb.description)
                .color(Color::new(emb.color)));
        }
    } else {
        embeds.push(CreateEmbed::new()
            .title("MHFZ user interface")
            .color(color("40", "ff", "40"))
            .description("button interface for mhfz player to make use of server's utility"));
    }

    let row1 = CreateActionRow::Buttons(vec![
        Components::normal_button("Register", "register", ButtonStyle::Primary, "📝"),
        Components::normal_button("Bind", "bind", ButtonStyle::Secondary, "🔗"),
        Components::normal_button("Link PSN", "psn_btn", ButtonStyle::Primary, "📱"),
    ]);

    let row2 = CreateActionRow::Buttons(vec![
        Components::normal_button("DM save", "dms", ButtonStyle::Success, "🔐"),
        Components::normal_button("Claim Role", "role_btn", ButtonStyle::Success, "😊"),
    ]);

    let resp = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embeds(embeds)
            .components(vec![row1, row2])
    );

    Components::response_adv(bnd, resp).await?;
    Ok(())
}