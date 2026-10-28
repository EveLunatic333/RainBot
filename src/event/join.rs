use serenity::all::*;
use serenity::builder::CreateAttachment;
use serde::{Deserialize, Serialize};
use std::fs;
use crate::{Init, MyErr, ComponentBundle, Components};
use crate::reusable::utils;

#[derive(Debug, Serialize, Deserialize)]
pub struct WelcomeConfig {
    pub button_role_id: u64,
    pub guide_link: String,
    pub welcome_embed: EmbedConfig,
    pub dm_rules: Vec<EmbedConfig>,
    pub leave_embed: EmbedConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbedConfig {
    pub title: String,
    pub description: String,
    pub color: String,
    pub image_url: String,
    pub fields: Vec<FieldConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FieldConfig {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

impl EmbedConfig {
    fn get_color(&self) -> Color {
        if self.color.to_uppercase() == "RANDOM" { return utils::Color::Random.throw(); }
        let clean_hex = self.color.trim_start_matches('#').trim_start_matches("0x");
        u32::from_str_radix(clean_hex, 16).map(Color::new).unwrap_or(Color::DARK_GREY)
    }
}

impl WelcomeConfig {
    pub fn load(default_role: u64) -> Self {
        let path = "static/welcome.json";
        let _ = fs::create_dir_all("static");
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(config) = serde_json::from_str(&data) { return config; }
        }
        let default_config = WelcomeConfig {
            button_role_id: default_role,
            guide_link: "https://discord.com/channels/937230168223789066/952959980749881434".to_string(),
            welcome_embed: EmbedConfig {
                title: "Welcome To Our Server".to_string(),
                description: "Welcome `{username}` to our server, we are happy to have you as our {count}'s member.".to_string(),
                color: "RANDOM".to_string(),
                image_url: "banner.png".to_string(),
                fields: vec![
                    FieldConfig { name: "About Server".to_string(), value: "This server is a community for Monster Hunter Frontier players global.".to_string(), inline: false },
                    FieldConfig { name: "Access Rules".to_string(), value: "Please press the button below and check your DM for the full rules list.".to_string(), inline: false },
                ]
            },
            dm_rules: vec![
                EmbedConfig {
                    title: "Server Rules 🎉".to_string(),
                    description: "Please follow our rules to maintain a healthy environment.".to_string(),
                    color: "1ABC9C".to_string(),
                    image_url: String::new(),
                    fields: vec![
                        FieldConfig { name: "🛑 No Cheating".to_string(), value: "Use of Cheat Engine or memory scanners is strictly prohibited.".to_string(), inline: false }
                    ]
                }
            ],
            leave_embed: EmbedConfig {
                title: "Member Leave".to_string(),
                description: "{mention} left at <t:{time}:F>\nRemaining members: {count}".to_string(),
                color: "8B0000".to_string(),
                image_url: String::new(),
                fields: vec![]
            }
        };
        if let Ok(json_str) = serde_json::to_string_pretty(&default_config) {
            let _ = fs::write(path, json_str);
        }
        default_config
    }
}

fn replace_placeholders(text: &str, user: &User, count: usize, time: u64) -> String {
    text.replace("{username}", &user.name)
        .replace("{mention}", &user.to_string())
        .replace("{count}", &count.to_string())
        .replace("{time}", &time.to_string())
}

fn build_embed(cfg: &EmbedConfig, user: &User, count: usize, time: u64) -> (CreateEmbed, Option<String>) {
    let mut e = CreateEmbed::new()
        .title(replace_placeholders(&cfg.title, user, count, time))
        .description(replace_placeholders(&cfg.description, user, count, time))
        .color(cfg.get_color());
    let mut local_image = None;
    if !cfg.image_url.is_empty() {
        if cfg.image_url.starts_with("http") {
            e = e.image(&cfg.image_url);
        } else {
            e = e.image(format!("attachment://{}", cfg.image_url));
            local_image = Some(cfg.image_url.clone());
        }
    }
    for f in &cfg.fields {
        let name = replace_placeholders(&f.name, user, count, time);
        let val = replace_placeholders(&f.value, user, count, time);
        e = e.field(name, val, f.inline);
    }
    (e, local_image)
}

#[hertz::hertz_button_normal(0, false)]
async fn button(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let config = WelcomeConfig::load(bnd.init.server_role.member_role);
    let member = bnd.cmd.member.clone().ok_or(MyErr::Custom("Member data not found.".into()))?;
    let role = RoleId::new(config.button_role_id);
    if member.roles.contains(&role) {
        Components::response(bnd, "❌ You are already verified as a Member.", true).await?;
    } else {
        member.add_role(&bnd.ctx.http, role).await?;
        Components::response(bnd, "✅ Verification successful! Welcome to the server.", true).await?;
    }
    Ok(())
}

pub(super) async fn join(ctx: &Context, member: &Member, init: &Init) -> Result<(), MyErr> {
    let config = WelcomeConfig::load(init.server_role.member_role);
    let arow = CreateActionRow::Buttons(vec![
        Components::normal_button("Verify Access", "join", ButtonStyle::Primary, "✅"),
        CreateButton::new_link(&config.guide_link).label("Server Guide"),
    ]);
    let count = member.guild_id.to_partial_guild_with_counts(&ctx.http).await?
        .approximate_member_count.unwrap_or(0) as usize;
    let (mut embed, local_img) = build_embed(&config.welcome_embed, &member.user, count, 0);
    embed = embed.author(CreateEmbedAuthor::new(&member.user.name).icon_url(member.face()));
    let mut msg = CreateMessage::new()
        .embed(embed)
        .components(vec![arow])
        .content(member.user.to_string());
    if let Some(filename) = local_img {
        if let Ok(att) = CreateAttachment::path(format!("image/{}", filename)).await {
            msg = msg.add_file(att);
        }
    }
    ChannelId::new(init.log_channel.join_channel).send_message(&ctx.http, msg).await?;
    let mut dm_embeds = Vec::new();
    let mut dm_attachments = Vec::new();
    for r in &config.dm_rules {
        let (e, file_opt) = build_embed(r, &member.user, count, 0);
        dm_embeds.push(e);
        if let Some(filename) = file_opt {
            if let Ok(att) = CreateAttachment::path(format!("image/{}", filename)).await {
                dm_attachments.push(att);
            }
        }
    }
    if !dm_embeds.is_empty() {
        let mut dm_msg = CreateMessage::new().embeds(dm_embeds);
        for att in dm_attachments {
            dm_msg = dm_msg.add_file(att);
        }
        let _ = member.user.direct_message(&ctx.http, dm_msg).await;
    }
    Ok(())
}

pub(super) async fn leave(ctx: &Context, user: &User, guild: &GuildId, init: &Init) -> Result<(), MyErr> {
    let config = WelcomeConfig::load(init.server_role.member_role);
    let count = guild.to_partial_guild_with_counts(&ctx.http).await?
        .approximate_member_count.unwrap_or(0) as usize;
    let roles_str = match &user.member {
        Some(m) => m.roles.iter().map(|r| format!("<@&{}>", r)).collect::<Vec<_>>().join(" "),
        None => "Role data lost.".to_string(),
    };
    let now = utils::MyTime::now() as u64;
    let (mut embed, local_img) = build_embed(&config.leave_embed, user, count, now);
    embed = embed.field("Roles Inventory", roles_str, false);
    let mut msg = CreateMessage::new().embed(embed);
    if let Some(filename) = local_img {
        if let Ok(att) = CreateAttachment::path(format!("image/{}", filename)).await {
            msg = msg.add_file(att);
        }
    }
    ChannelId::new(init.log_channel.leave_channel).send_message(&ctx.http, msg).await?;
    Ok(())
}