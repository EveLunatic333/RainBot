use crate::{MyErr, SlashBundle, ModalBundle, Components};
use crate::reusable::utils::Color;
use serenity::all::*;

pub(super) async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let modal = CreateInteractionResponse::Modal(
        CreateModal::new("bar", "Bar Request Board")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Paragraph, "Requested item list", "item")
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Range price", "price")
                ),
            ]),
    );
    Components::response_adv(bnd, modal).await?;
    Ok(())
}

#[hertz::hertz_modal_normal(0, false)]
async fn modal(bnd: &ModalBundle<'_>) -> Result<(), MyErr> {
    let mut item = "";
    let mut price = "";
    for data in &bnd.cmd.data.components {
        let arow = data.components.first().ok_or_else(|| MyErr::Custom("Cannot get first component".to_owned()))?;
        if let ActionRowComponent::InputText(x) = arow {
            match x.custom_id.as_str() {
                "item" => item = x.value.as_deref().unwrap_or_default(),
                "price" => price = x.value.as_deref().unwrap_or_default(),
                _ => continue,
            }
        }
    }

    bnd.cmd.create_response(
        &bnd.ctx.http,
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("✅ **Request Accepted!** The Bartenders have been notified.")
                .ephemeral(true)
        )
    ).await?;

    let embed = CreateEmbed::new()
        .title("Bar Request")
        .color(Color::Blue.throw())
        .description(item)
        .field("Requested price", price, false)
        .author(CreateEmbedAuthor::new(&bnd.cmd.user.name).icon_url(bnd.cmd.user.face()));

    let msg = ChannelId::new(bnd.init.log_channel.request_channel)
        .send_message(
            &bnd.ctx.http,
            CreateMessage::new()
                .embed(embed)
                .content(format!("<@&{}> {}", bnd.init.server_role.bartender_role, bnd.cmd.user.to_string()))
        )
        .await?;

    msg.channel_id
        .create_thread(
            &bnd.ctx.http,
            CreateThread::new("requested_items").kind(ChannelType::PublicThread)
        )
        .await?;

    Ok(())
}