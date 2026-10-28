
use crate::event::interaction::Mybundle;
use serenity::all::*;
use tokio::io::AsyncWriteExt;
use tokio::fs::File;
use crate::reusable::config::Init;
use crate::{Components, Reg, SlashBundle, ComponentBundle, MyErr};
use crate::reusable::utils::{Color, MyTime};
use std::path::Path;
use std::time::{SystemTime, Duration};

pub struct FileSave {
    pub name: String,
    pub bin: Vec<u8>,
}

struct SaveJudge<'a> {
    cmd: &'a CommandInteraction,
    files: Vec<FileSave>,
}

impl<'a> SaveJudge<'a> {
    async fn get_save(cmd: &'a CommandInteraction) -> SaveJudge<'a> {
        let savelist = [
            "savedata.bin", "partner.bin", "decomyset.bin", "hunternavi.bin",
            "otomoairou.bin", "platebox.bin", "platedata.bin", "platemyset.bin",
            "rengokudata.bin", "savemercenary.bin", "skin_hist.bin",
        ];
        let mut container = Vec::new();
        let resolved = &cmd.data.resolved;

        for data in &cmd.data.options {
            if let CommandDataOptionValue::Attachment(att) = &data.value {
                if let Some(e) = resolved.attachments.get(att) {
                    if savelist.contains(&e.filename.as_str()) && e.size < 5_000_000 {
                        if let Ok(bytes) = e.download().await {
                            container.push(FileSave { name: e.filename.clone(), bin: bytes });
                        }
                    }
                }
            }
        }
        SaveJudge { cmd, files: container }
    }

    fn make_embed(&self, init: &Init) -> CreateEmbed {
        let mut x = String::from("```\n");
        for i in &self.files {
            x.push_str(&i.name);
            x.push('\n');
        }
        x.push_str("```");

        CreateEmbed::new()
            .title("Save Result")
            .description(format!(
                "The savefile result that already filtered.\n**NOTES PLEASE READ**\n>>> Do not login into the game until an admin approves your file. Do not worry if it takes them more than <t:{}:R>, it will be automatically accepted.",
                MyTime::elapsed(init.bot_config.transfer_timeout as i64)
            ))
            .author(CreateEmbedAuthor::new(&self.cmd.user.name).icon_url(self.cmd.user.face()))
            .field("Filtered File(s)", x, true)
            .color(Color::Blue.throw())
    }

    async fn save_to_file(&self) -> Result<(), MyErr> {
        let save_dir = Path::new(".").join("save");
        let user_filename = format!("{}.txt", self.cmd.user.id);
        let mut direction = File::create(&save_dir.join(&user_filename)).await?;
        let mut leeway = String::new();

        for data in &self.files {
            let name = format!("{}_{}", self.cmd.user.id, data.name);
            tokio::fs::write(save_dir.join(&name), &data.bin).await?;
            if !leeway.is_empty() { leeway.push(','); }
            leeway.push_str(&name);
        }
        direction.write_all(leeway.as_bytes()).await?;
        Ok(())
    }

    fn make_button(&self) -> CreateActionRow {
        let uid = self.cmd.user.id.to_string();
        CreateActionRow::Buttons(vec![
            Components::normal_button("approve", &format!("{}*save*a", uid), ButtonStyle::Primary, "👍"),
            Components::normal_button("disaprove", &format!("{}*save*d", uid), ButtonStyle::Danger, "👎"),
        ])
    }
}

struct SaveAcknowladge {
    uid: String,
    accept: bool,
}

impl SaveAcknowladge {
    fn check(data: Vec<&str>) -> Result<SaveAcknowladge, MyErr> {
        if let (Some(uid_str), Some(action)) = (data.first(), data.last()) {
            return Ok(SaveAcknowladge { uid: uid_str.to_string(), accept: *action == "a" });
        }
        Err(MyErr::Custom("Cannot retrieve data from button interaction. Please report this issue.".to_owned()))
    }

    async fn get_files(&self) -> Result<Vec<FileSave>, MyErr> {
        let save_dir = Path::new(".").join("save");
        let dir_content = tokio::fs::read_to_string(save_dir.join(format!("{}.txt", self.uid))).await?;
        let mut container = Vec::new();

        for path in dir_content.split(',') {
            if path.is_empty() { continue; }
            let bytea = tokio::fs::read(save_dir.join(path)).await?;
            let mut name = path.split('_').last().unwrap_or("").split('.').next().unwrap_or("");
            if name == "hist" { name = "skin_hist"; }
            container.push(FileSave { bin: bytea, name: name.to_owned() });
        }
        Ok(container)
    }
}

#[hertz::hertz_slash_reg(300, true)]
async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;
    let data = SaveJudge::get_save(bnd.cmd).await;

    if data.files.is_empty() {
        return Err(MyErr::Custom("No matched file detected. Please ensure your save file is named correctly and does not exceed the size limit.".to_string()));
    }

    data.save_to_file().await?;
    let content = EditInteractionResponse::new().embed(data.make_embed(bnd.init));
    let ch = ChannelId::new(bnd.init.log_channel.transfer_channel);
    Components::edit_adv(bnd, content).await?;

    let msg = ch.send_message(&bnd.ctx.http, CreateMessage::new()
        .content(format!("<@&{}>", bnd.init.server_role.judge_role))
        .embed(data.make_embed(bnd.init))
        .components(vec![data.make_button()])
    ).await?;

    tokio::time::sleep(Duration::from_secs(bnd.init.bot_config.transfer_timeout)).await;
    auto_accept(msg, bnd, reg).await?;
    Ok(())
}

async fn auto_accept(msg: Message, bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let mut current_msg = msg.channel_id.message(&bnd.ctx.http, msg.id).await?;
    if current_msg.components.is_empty() { return Ok(()); }

    let ack = SaveAcknowladge { uid: bnd.cmd.user.id.to_string(), accept: true };
    let files = ack.get_files().await?;
    let user = bnd.user();

    for file in &files {
        reg.pg.transfer_file(file, reg.cid).await?;
    }

    let _ = user.direct_message(&bnd.ctx.http, CreateMessage::new().content("Your save file has been approved automatically by the system. You may login to the game now.")).await;

    current_msg.edit(&bnd.ctx.http, EditMessage::new()
        .content(format!("Approved automatically by system at <t:{}:F>", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs()))
        .components(vec![])
    ).await?;
    Ok(())
}

#[hertz::hertz_button_normal(0, false)]
async fn button(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    Components::response(bnd, "Beginning lengthy operation. Please do not interact with buttons temporarily.", true).await?;
    let data: Vec<&str> = bnd.cmd.data.custom_id.split('*').collect();
    let check = SaveAcknowladge::check(data)?;
    let uid_parsed = check.uid.parse::<u64>().map_err(|_| MyErr::Custom("Invalid User ID format".to_string()))?;
    let target_user = UserId::new(uid_parsed).to_user(&bnd.ctx.http).await.map_err(|_| MyErr::Custom("Failed to fetch user".to_string()))?;

    if check.accept {
        let files = check.get_files().await?;
        let mut reg = Reg::check(bnd, &target_user).await?;
        for file in &files {
            reg.pg.transfer_file(file, reg.cid).await?;
        }
        let _ = target_user.direct_message(&bnd.ctx.http, CreateMessage::new().content(&format!("Your save file has been approved by {}. You may login to the game now.", bnd.cmd.user.name))).await;
        let mut msg = bnd.cmd.message.clone();
        msg.edit(&bnd.ctx.http, EditMessage::new()
            .content(format!("Approved by {} at <t:{}:F>", bnd.cmd.user.name, SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs()))
            .components(vec![])
        ).await?;
        reg.pg.close().await;
    } else {
        let _ = target_user.direct_message(&bnd.ctx.http, CreateMessage::new().content(&format!("Your save file has been rejected by {}. Please check your files and try again.", bnd.cmd.user.name))).await;
        bnd.cmd.message.delete(&bnd.ctx.http).await?;
    }
    Ok(())
}