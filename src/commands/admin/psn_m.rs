use crate::event::interaction::Mybundle;
use serenity::all::*;
use crate::{ModalBundle, MyErr};
use crate::reusable::component::registered::Reg;
use crate::reusable::utils::password;
use tracing::error;
use sqlx::Row;

pub async fn discord_all(bnd: &ModalBundle<'_>) -> Result<(), MyErr> {
    let mut psn_id = String::new();
    let mut password_input = String::new();
    
    for action_row in &bnd.cmd.data.components {
        for component in &action_row.components {
            if let ActionRowComponent::InputText(input) = component {
                match input.custom_id.as_str() {
                    "psn_id_input" => psn_id = input.value.clone().unwrap_or_default(),
                    "password_input" => password_input = input.value.clone().unwrap_or_default(),
                    _ => {}
                }
            }
        }
    }
    
    let trimmed_psn = psn_id.trim();
    let user = bnd.user();

    if trimmed_psn.is_empty() {
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("❌ **Invalid Input:** PSN ID cannot be empty.")
                    .ephemeral(true)
            )
        ).await;
        return Ok(());
    }
    
    if password_input.is_empty() {
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("❌ **Invalid Input:** Password is required to link your PSN ID.")
                    .ephemeral(true)
            )
        ).await;
        return Ok(());
    }

    let response_msg = async {
        let reg = match Reg::no_check(bnd, &user).await {
            Ok(r) => r,
            Err(_) => return "🛑 **Access Denied:** System error during authentication check. Please try again.".to_string(),
        };

        let row = match sqlx::query("SELECT user_id FROM discord_register WHERE discord_id = $1")
            .bind(user.id.get().to_string())
            .fetch_one(&reg.pg.pool)
            .await 
        {
            Ok(r) => r,
            Err(_) => return "🛑 **Account Not Found**\n\nYour Discord account is not linked to any game account yet.\n\n**How to fix:**\n1. Use the **📝 Register** button to create a new account\n2. Or use the **🔗 Bind** button to link an existing account\n3. Then try linking your PSN ID again".to_string(),
        };

        let account_id: i32 = match row.try_get("user_id") {
            Ok(id) => id,
            Err(e) => {
                error!("Schema mismatch: {}", e);
                return "❌ **Database Error:** Database column error.".to_string();
            }
        };

        let password_hash = match reg.pg.get_password_hash(account_id).await {
            Ok(hash) => hash,
            Err(e) => {
                error!("Failed to get password hash: {}", e);
                return "❌ **Database Error:** Failed to retrieve account information.".to_string();
            }
        };

        let is_valid = match password::verify(&password_input, &password_hash) {
            Ok(valid) => valid,
            Err(e) => {
                error!("Password verification error: {}", e);
                return "❌ **System Error:** Password verification failed.".to_string();
            }
        };

        if !is_valid {
            return "❌ **Invalid Password:** The password you entered is incorrect.".to_string();
        }

        match reg.pg.is_account_bound_to_user(account_id, &user.id.to_string()).await {
            Ok(true) => {
                return "❌ **Account Already Bound:** This game account is already linked to another Discord user.".to_string();
            }
            Ok(false) => {}
            Err(e) => {
                error!("Failed to check bind status: {}", e);
                return "❌ **Database Error:** Failed to check account binding status.".to_string();
            }
        }

        match sqlx::query("UPDATE users SET psn_id = $1 WHERE id = $2")
            .bind(trimmed_psn)
            .bind(account_id)
            .execute(&reg.pg.pool)
            .await
        {
            Ok(_) => format!(
                "✅ **Success!**\n\nPSN ID `{}` has been linked to your account.\n\nYou can now use features that require PSN authentication.",
                trimmed_psn
            ),
            Err(e) => {
                error!("Database update failed: {}", e);
                "❌ **Database Error:** Failed to link PSN account. Please try again later.".to_string()
            }
        }
    }.await;

    bnd.cmd.create_response(
        &bnd.ctx.http,
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().content(response_msg).ephemeral(true)
        )
    ).await.map_err(|e| MyErr::Custom(format!("Response delivery failed: {}", e)))?;

    Ok(())
}