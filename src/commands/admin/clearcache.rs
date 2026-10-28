use serenity::all::*;
use crate::{MyErr, SlashBundle};
use crate::reusable::component::bounty::BountySubmit;

#[hertz::hertz_slash_normal(0, false)]
pub async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ **Command Disabled:** The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(msg).ephemeral(true)
        )).await;
        return Ok(());
    }

    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().content("⏳ Processing cache clearance...").ephemeral(true)
    )).await;

    let member = bnd.cmd.member.as_ref().ok_or_else(|| {
        MyErr::Custom("❌ **Error:** Must be used in a server.".to_string())
    })?;

    let is_owner = bnd.cmd.user.id.get() == bnd.init.discord.author_id;
    let is_admin = member.permissions.unwrap_or(Permissions::empty()).administrator();

    let allowed_roles = [
        bnd.init.server_role.admin_role,
        bnd.init.server_role.maintainer_role,
        bnd.init.server_role.bartender_role,
        bnd.init.server_role.judge_role,
    ];

    let has_role_access = member.roles.iter().any(|r| allowed_roles.contains(&r.get()));

    if !is_owner && !is_admin && !has_role_access {
        let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new()
            .content("🛑 **Permission Denied:** You must be an Admin, Owner, or authorized Staff to use this command.")
        ).await;
        return Ok(());
    }

    let mut target_user_id = String::new();
    for opt in &bnd.cmd.data.options {
        if opt.name == "target" {
            if let CommandDataOptionValue::User(user_id) = &opt.value {
                target_user_id = user_id.to_string();
            }
        }
    }

    if target_user_id.is_empty() {
        let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new()
            .content("⚠️ **Notice:** Please specify a target user.")
        ).await;
        return Ok(());
    }

    let response_msg = if let Some(submit) = BountySubmit::open(&target_user_id).await {
        submit.delete().await;
        format!("✅ **Success:** The bounty cache for <@{}> has been permanently wiped.", target_user_id)
    } else {
        format!("ℹ️ **Info:** No active bounty cache was found for <@{}>.", target_user_id)
    };

    let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(response_msg)).await;
    Ok(())
}