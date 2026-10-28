use serenity::all::*;
use crate::{MyErr, SlashBundle, Components, Reg, PgConn};
use tracing::{warn, info, error};

const MAX_AUTOCOMPLETE_RESULTS: usize = 25;

#[hertz::hertz_slash_reg(20, false)]
pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let mut subcommand_name = None;
    let mut guild_id = None;
    let mut target_user = None;

    for opt in &bnd.cmd.data.options {
        if let CommandDataOptionValue::SubCommand(sub_opts) = &opt.value {
            subcommand_name = Some(opt.name.as_str());
            for sub_opt in sub_opts {
                match &sub_opt.value {
                    CommandDataOptionValue::Integer(val) => guild_id = Some(*val),
                    CommandDataOptionValue::User(user_id) => target_user = Some(*user_id),
                    _ => {}
                }
            }
        }
    }

    match subcommand_name {
        Some("join") => join(bnd, reg, guild_id.unwrap_or(0)).await?,
        Some("leave") => leave(bnd, reg).await?,
        Some("clear_app") => clear_app(bnd, reg, target_user).await?,
        Some("extend_food") => super::extend_food::slash(bnd, reg).await?,
        Some("info") | None => see(bnd, reg, guild_id.unwrap_or(0)).await?,
        Some(unknown) => {
            warn!(subcommand = %unknown, "Unknown guild subcommand received");
            return Ok(());
        }
    }
    Ok(())
}

#[hertz::hertz_auto]
pub async fn auto(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    for opt in Components::sub_options(bnd)? {
        if let CommandDataOptionValue::Autocomplete { kind: _, value } = &opt.value {
            match opt.name.as_str() {
                "target_guild" | "food" => {
                    return super::extend_food::auto(bnd).await;
                }
                "guild" | _ => {
                    let mut pg = PgConn::create(bnd.init, bnd.cmd.user.id.to_string()).await?;
                    let list = pg.guild_list().await?;
                    let query_lower = value.to_lowercase();
                    let mut choices: Vec<_> = list
                        .iter()
                        .filter_map(|guild| {
                            let name_lower = guild.name.to_lowercase();
                            if name_lower.contains(&query_lower) || name_lower.starts_with(&query_lower) {
                                Some(serenity::builder::AutocompleteChoice::new(guild.name.clone(), guild.id.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    if choices.len() > MAX_AUTOCOMPLETE_RESULTS {
                        choices.truncate(MAX_AUTOCOMPLETE_RESULTS);
                    }

                    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Autocomplete(
                        CreateAutocompleteResponse::new().set_choices(choices)
                    )).await;
                    pg.close().await;
                }
            }
        }
    }
    Ok(())
}

async fn join(bnd: &SlashBundle<'_>, reg: &Reg<'_>, guild_id: i64) -> Result<(), MyErr> {
    if let Err(e) = reg.pg.clear_guild_application(reg.cid).await {
        warn!(error = %e, char_id = reg.cid, "Failed to auto-purge guild_applications");
    }
    reg.pg.guild_join(guild_id, reg.cid).await?;
    Components::response(bnd, "✅ **Mission Accomplished:** You have successfully joined the Guild!", true).await?;
    Ok(())
}

async fn leave(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let current_guild_id = reg.pg.guild_id(reg.cid).await?;
    if current_guild_id == 0 {
        return Components::response(bnd, "⚠️ **System Alert:** You are not currently a member of any guild.", true).await;
    }
    if let Err(e) = reg.pg.guild_leave(reg.cid).await {
        error!(error = %e, char_id = reg.cid, guild_id = current_guild_id, "Failed to leave guild via database");
        return Components::response(bnd, "❌ **Critical Failure:** Could not process guild withdrawal due to database error.", true).await;
    }
    info!(char_id = reg.cid, guild_id = current_guild_id, "User left guild successfully");
    Components::response(bnd, "✅ **Mission Accomplished:** You have successfully left your guild.", true).await?;
    Ok(())
}

async fn clear_app(bnd: &SlashBundle<'_>, reg: &Reg<'_>, target_user: Option<UserId>) -> Result<(), MyErr> {
    let admin_role = RoleId::new(bnd.init.server_role.admin_role);
    let is_admin = bnd.cmd.member.as_ref().map(|m| m.roles.contains(&admin_role)).unwrap_or(false);
    if !is_admin {
        return Components::response(bnd, "🛑 **Access Denied:** Only administrators with proper clearance can execute this emergency operation.", true).await;
    }

    let target_uid = match target_user {
        Some(uid) => uid,
        None => return Components::response(bnd, "❌ **Error:** Target user is invalid or not specified.", true).await,
    };

    let target_char_id = match reg.pg.get_char_id_by_discord(target_uid.get()).await {
        Ok(cid) => cid,
        Err(_) => return Components::response(bnd, "❌ **Failed:** This user is not registered in the Rain Server database.", true).await,
    };

    match reg.pg.clear_guild_application(target_char_id).await {
        Ok(_) => {
            info!(admin = %bnd.cmd.user.id, target = %target_uid, char_id = target_char_id, "Manually cleared stuck guild application");
            Components::response(bnd, &format!("✅ **Rescue Operation Successful:** Stuck guild application data for <@{}> (Char ID: {}) has been removed from the database! The user can now join a guild via Clan Hall.", target_uid.get(), target_char_id), false).await?;
        }
        Err(e) => {
            error!(error = %e, admin = %bnd.cmd.user.id, target = %target_uid, "Failed to manually clear guild_applications");
            Components::response(bnd, "❌ **Failed:** An error occurred while accessing the database. Please report this to technical support!", true).await?;
        }
    }
    Ok(())
}

async fn see(bnd: &SlashBundle<'_>, reg: &Reg<'_>, guild_id: i64) -> Result<(), MyErr> {
    let (guild_data, member_count) = reg.pg.guild_search(guild_id).await?;
    let embed = guild_data.embed(member_count, bnd).await?;
    Components::response_adv(bnd, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed))).await?;
    Ok(())
}

use crate::reusable::postgress::guild::Guild;
impl Guild {
    async fn embed(&self, member_count: i64, bnd: &SlashBundle<'_>) -> Result<CreateEmbed, MyErr> {
        let created_timestamp = self.created.timestamp();
        let leader_mention = self.discord_id.as_ref().map(|id| format!("<@{}>", id)).unwrap_or_else(|| "None".to_string());
        let mut embed = CreateEmbed::new()
            .title(&self.name)
            .fields(vec![
                ("General Details", format!("🆔 Guild ID: {}\n🏛️ Created: <t:{}:R>\n🧑 Member Count: {}/60\n🎖️ Rank Points: {}", self.id, created_timestamp, member_count, self.rank_rp), false),
                ("Leader Details", format!("🆔 Leader ID: {}\n🏷️ Leader Name: {}\n🎮 Leader Discord: {}", self.leader_id, self.lead_name, leader_mention), false),
            ])
            .color(crate::reusable::utils::Color::Random.throw());

        if let Some(discord_id_str) = &self.discord_id {
            if let Ok(discord_id) = discord_id_str.parse::<u64>() {
                if let Ok(user) = UserId::new(discord_id).to_user(&bnd.ctx.http).await {
                    embed = embed.footer(CreateEmbedFooter::new(format!("Owned by {}", user.name)).icon_url(user.face()));
                }
            }
        }
        Ok(embed)
    }
}