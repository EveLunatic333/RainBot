use serenity::all::*;
use crate::{MyErr, MsgBundle, Components};
use crate::reusable::component::market::Tag;
use crate::reusable::utils::dumb_matching;

#[hertz::hertz_msg(false)]
async fn message(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    let tag = Tag::new().await?;
    let arg = bnd.msg.content.split_whitespace().nth(1).ok_or_else(|| MyErr::Custom("Missing command argument.".to_string()))?;
    let names: Vec<_> = tag.tag.iter().map(|x| x.command.clone()).collect();
    let mut embed = CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .footer(CreateEmbedFooter::new(format!("Requested by {}", &bnd.msg.author.name)).icon_url(bnd.msg.author.face()));

    match arg {
        "list" => {
            let desc = names.iter().map(|i| format!("`?tag {}`", i)).collect::<Vec<_>>().join(", ");
            embed = embed.title("List of Tag Commands").description(desc);
        }
        _ => {
            if names.contains(&arg.to_string()) {
                let item = tag.tag.iter().find(|x| x.command == arg).unwrap();
                embed = embed.title(&item.command).description(&item.desc).image(&item.url);
            } else {
                let mut filtered = vec!["Maybe This Command is what you need\n".to_string()];
                for name in &names {
                    if dumb_matching(name, arg) > 0.4 {
                        filtered.push(format!("`?tag {}`", name));
                        filtered.push(", ".to_string());
                    }
                }
                if filtered.len() > 2 {
                    let len = filtered.len() - 2;
                    embed = embed.title("Tag Not Found").description(filtered[..len].join(""));
                } else {
                    embed = embed.title("Tag Not Found").description("No matching commands found.");
                }
            }
        }
    }

    if let Some(x) = &bnd.msg.referenced_message {
        embed = embed.author(CreateEmbedAuthor::new(&x.author.name).icon_url(x.author.face()));
    }

    bnd.msg.channel_id.send_message(&bnd.ctx.http, Components::msg_reply_raw(bnd).embed(embed)).await?;
    Ok(())
}