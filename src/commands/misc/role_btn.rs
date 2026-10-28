use serenity::all::*;
use crate::{MyErr, ComponentBundle};
use tracing::error;

#[hertz::hertz_button_normal(0, false)]
pub async fn all(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    bnd.cmd.defer_ephemeral(&bnd.ctx.http).await.unwrap_or_default();
    let target_role_id = bnd.init.server_role.member_role;
    let role_id = RoleId::new(target_role_id);

    if let Some(member) = bnd.cmd.member.clone() {
        if member.roles.contains(&role_id) {
            let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("ℹ️ You already have the Member role!")).await;
        } else {
            match member.add_role(&bnd.ctx.http, role_id).await {
                Ok(_) => {
                    let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("✅ **Success!** Member role has been assigned to you.")).await;
                }
                Err(e) => {
                    error!(error = %e, "Failed to assign member role");
                    let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ **Error:** Failed to assign role. Check bot permissions.")).await;
                }
            }
        }
    }
    Ok(())
}