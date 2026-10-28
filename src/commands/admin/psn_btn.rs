use serenity::all::*;
use crate::ComponentBundle;
use crate::PgConn;
use crate::event::interaction::Mybundle;
use tracing::error;

pub async fn discord_all(bnd: &ComponentBundle<'_>) {
    let user = bnd.user();
    
    let mut pg = match PgConn::create(bnd.init, user.id.to_string()).await {
        Ok(pg) => pg,
        Err(e) => {
            error!("Failed to create PgConn for PSN check: {:?}", e);
            let _ = bnd.cmd.create_response(
                &bnd.ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("❌ System Error: Failed to connect to database. Please try again later.")
                        .ephemeral(true)
                )
            ).await;
            return;
        }
    };
    
    let is_registered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM discord_register WHERE discord_id = $1)"
    )
    .bind(user.id.to_string())
    .fetch_one(&pg.pool)
    .await
    .unwrap_or(false);

    let _ = pg.close().await;
    
    if !is_registered {
        let msg = "🛑 **Account Required**\n\n\
            You must register an account before linking your PSN ID.\n\n\
            **Next Steps:**\n\
            • Use the **📝 Register** button above to create a new account\n\
            • Use the **🔗 Bind** button to link an existing account\n\n\
            After registration, you can link your PSN ID here.";
        
        let _ = bnd.cmd.create_response(
            &bnd.ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(msg)
                    .ephemeral(true)
            )
        ).await;
        return;
    }

    let psn_input = CreateActionRow::InputText(
        CreateInputText::new(InputTextStyle::Short, "PSN ID", "psn_id_input")
            .placeholder("Enter your PlayStation Network ID")
            .required(true)
            .min_length(3)
            .max_length(16)
    );
    
    let password_input = CreateActionRow::InputText(
        CreateInputText::new(InputTextStyle::Short, "Password", "password_input")
            .placeholder("Enter your account password")
            .required(true)
            .min_length(8)
            .max_length(100)
    );

    let modal = CreateInteractionResponse::Modal(
        CreateModal::new("psn_m", "Link PSN Account")
            .components(vec![psn_input, password_input])
    );

    if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, modal).await {
        error!("Failed to send PSN modal: {:?}", e);
    }
}