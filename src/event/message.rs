use serenity::all::*;
use crate::{Init, MyErr, ItemPedia, commands};
use crate::reusable::component::sightengine;
use crate::PgConn;

pub struct MsgBundle<'a> {
    pub ctx: &'a Context,
    pub msg: &'a Message,
    pub init: &'a Init,
    pub pedia: &'a ItemPedia,
}

pub async fn msg_handler(ctx: &Context, msg: &Message, init: &Init, pedia: &ItemPedia) {
    if msg.author.bot || msg.webhook_id.is_some() {
        return;
    }

    if init.discord.sightengine_enabled && !msg.attachments.is_empty() {
        if let Some(keys) = sightengine::SIGHTENGINE_KEYS.as_ref() {
            for att in &msg.attachments {
                if att.content_type.as_ref().map_or(false, |c| c.starts_with("image/")) {
                    if sightengine::scan_image(&att.url, &keys.api_user, &keys.api_secret).await {
                        let _ = msg.delete(&ctx.http).await;
                        let warning = format!(
                            "🛑 **Security Alert**: <@{}>, your upload was flagged for violating community guidelines and has been removed.",
                            msg.author.id
                        );
                        let _ = msg.channel_id.say(&ctx.http, warning).await;
                        return;
                    }
                }
            }
        }
    }

    if msg.content.starts_with(&init.discord.prefix) {
        let bnd = MsgBundle { ctx, msg, init, pedia };
        let cmd_body = &msg.content[init.discord.prefix.len()..];
        let command_name = cmd_body.split_whitespace().next().unwrap_or("");

        match command_name {
            "test"    => discordtest(&bnd).await,
            "ping"    => discordping(&bnd).await,
            "execute" => commands::admin::query::discordmsg(&bnd).await,
            "query"   => commands::admin::query::discordqry(&bnd).await,
            "tag"     => commands::misc::tag::discordmessage(&bnd).await,
            _ => (),
        }
    }
}

#[hertz::hertz_msg(false)]
async fn test(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    bnd.msg.channel_id.send_message(
        &bnd.ctx.http,
        CreateMessage::new().embed(
            CreateEmbed::new()
                .title("✅ Connection Verified")
                .description("System is operational and responding to requests.")
                .color(0x00ff00),
        ),
    ).await?;
    Ok(())
}

#[hertz::hertz_msg(false)]
async fn ping(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    let now = std::time::Instant::now();
    let mut msg = crate::Components::msg(bnd, "📡 Analyzing system latency...").await?;
    let discord_latency = format!("`{:.2?}`", now.elapsed());

    let pg_start = std::time::Instant::now();
    let mut post = PgConn::create(bnd.init, bnd.init.discord.author_id.to_string()).await?;
    let pg_latency = format!("`{:.2?}`", pg_start.elapsed());

    let fs_start = std::time::Instant::now();
    let _fs_test = Init::new().await?;
    let fs_latency = format!("`{:.2?}`", fs_start.elapsed());

    let embed = CreateEmbed::new()
        .title("⚙️ System Diagnostics")
        .field("Discord API", discord_latency, true)
        .field("PostgreSQL", pg_latency, true)
        .field("File System", fs_latency, true)
        .color(crate::reusable::utils::Color::Random.throw())
        .footer(CreateEmbedFooter::new(format!("Requested by {}", bnd.msg.author.name)));

    msg.edit(&bnd.ctx.http, EditMessage::new().content("").embed(embed)).await?;
    post.close().await;
    Ok(())
}