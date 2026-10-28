use crate::event::interaction::Mybundle;
use crate::reusable::utils::color;
use crate::{MyErr, ModalBundle, Reg, Components};
use serenity::all::*;
use tracing::{error, warn};

struct RegisterAcknowledged<'a, 'b> {
    name: &'a str,
    uid: i32,
    bnd: &'a ModalBundle<'b>,
}

impl<'a, 'b> RegisterAcknowledged<'a, 'b> {
    fn new(name: &'a str, uid: i32, bnd: &'a ModalBundle<'b>) -> Self {
        Self { name, uid, bnd }
    }

    async fn add_roles(&self) -> Result<(), MyErr> {
        let rid = RoleId::new(self.bnd.init.server_role.register_role);
        if let Some(member) = &self.bnd.cmd.member {
            member.add_role(&self.bnd.ctx.http, rid).await?;
        }
        Ok(())
    }

    async fn log_to_user(&self, is_register: bool) -> Result<(), MyErr> {
        let action_word = if is_register { "created" } else { "bound" };
        let user = self.bnd.user();
        let ch = ChannelId::new(self.bnd.init.log_channel.account_channel);

        let embed_desc = format!(
            "{} just {} an account on the server. Remember that you still need to have a character in-game to fully utilize our Discord features. Hurry up and create one if you haven't!",
            user.to_string(),
            action_word
        );

        ch.send_message(
            &self.bnd.ctx.http,
            CreateMessage::new()
                .content(user.to_string())
                .embed(
                    CreateEmbed::new()
                        .title(format!("Account Successfully {} on Server", action_word.to_uppercase()))
                        .description(embed_desc)
                        .fields(vec![
                            ("Username", format!("`{}`", self.name), false),
                            ("User ID", format!("`{}`", self.uid), false),
                        ])
                        .author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
                        .colour(color("00", "ff", "00"))
                        .image("https://media.discordapp.net/attachments/1068440173479739393/1068458599627620392/cachedImage.png?width=807&height=455"),
                ),
        ).await?;
        Ok(())
    }
}

fn modal_register_row(name: &str, is_password: bool) -> CreateActionRow {
    let placeholder = if is_password {
        "Your MHFZ user password (ignore Discord warning)"
    } else {
        "Your MHFZ username on launcher"
    };
    CreateActionRow::InputText(
        CreateInputText::new(InputTextStyle::Short, name, name)
            .required(true)
            .placeholder(placeholder),
    )
}

pub fn modal_response(is_register: bool) -> CreateInteractionResponse {
    let (custom_id, title) = if is_register {
        ("register_m", "Register Command")
    } else {
        ("bind_m", "Bind Command")
    };
    CreateInteractionResponse::Modal(
        CreateModal::new(custom_id, title).components(vec![
            modal_register_row("username", false),
            modal_register_row("password", true),
        ]),
    )
}

#[hertz::hertz_combine_normal(60, false)]
pub async fn all<T: crate::Mybundle>(bnd: &T) -> Result<(), MyErr> {
    let is_register = bnd.name().as_str() != "bind";
    let user = bnd.user();
    if let Some(mut reg) = Reg::reverse_check(bnd, &user).await? {
        Components::response_adv(bnd, modal_response(is_register)).await?;
        reg.pg.close().await;
    }
    Ok(())
}

async fn send_error_response(bnd: &ModalBundle<'_>, msg: &str) {
    let edit_result = bnd.cmd.edit_response(
        &bnd.ctx.http,
        EditInteractionResponse::new().content(msg)
    ).await;
    
    match edit_result {
        Ok(_) => {
        }
        Err(edit_err) => {
            let err_str = edit_err.to_string();
            warn!("edit_response failed: {}", err_str);

            let followup_result = bnd.cmd.create_followup(
                &bnd.ctx.http,
                CreateInteractionResponseFollowup::new()
                    .content(msg)
                    .ephemeral(true)
            ).await;
            
            if let Err(followup_err) = followup_result {
                let followup_err_str = followup_err.to_string();
                error!("create_followup also failed: {}", followup_err_str);

                if let Ok(dm_channel) = bnd.user().create_dm_channel(&bnd.ctx.http).await {
                    let dm_result = dm_channel.send_message(
                        &bnd.ctx.http,
                        CreateMessage::new().content(format!(
                            "⚠️ **System Notice**\n\nYour recent `/bind` or `/create` command encountered an error:\n\n{}",
                            msg
                        ))
                    ).await;
                    
                    if let Err(dm_err) = dm_result {
                        error!("Failed to send DM as last resort: {}", dm_err);
                    }
                }
            }
        }
    }
}

async fn send_success_response(bnd: &ModalBundle<'_>, msg: &str) {
    let edit_result = bnd.cmd.edit_response(
        &bnd.ctx.http,
        EditInteractionResponse::new().content(msg)
    ).await;
    
    if let Err(edit_err) = edit_result {
        warn!("edit_response for success failed: {}", edit_err);
        
        let _ = bnd.cmd.create_followup(
            &bnd.ctx.http,
            CreateInteractionResponseFollowup::new()
                .content(msg)
                .ephemeral(true)
        ).await;
    }
}

#[hertz::hertz_modal_normal(0, false)]
async fn modal(bnd: &ModalBundle<'_>) -> Result<(), MyErr> {
    if let Err(defer_err) = bnd.cmd.defer(&bnd.ctx.http).await {
        warn!("Failed to defer modal: {}", defer_err);
    }
    
    let is_register = bnd.name().as_str() != "bind_m";
    let user = bnd.user();

    let reg = match Reg::no_check(bnd, &user).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to create Reg: {:?}", e);
            send_error_response(bnd, "❌ **System Error:** Failed to initialize database connection. Please try again later.").await;
            return Ok(());
        }
    };

    let mut name = String::new();
    let mut password = String::new();

    for comp in &bnd.cmd.data.components {
        if let Some(ActionRowComponent::InputText(input)) = comp.components.first() {
            match input.custom_id.as_str() {
                "username" => name = input.value.clone().unwrap_or_default(),
                "password" => password = input.value.clone().unwrap_or_default(),
                _ => {}
            }
        }
    }

    if name.trim().is_empty() {
        send_error_response(bnd, "❌ **Invalid Input:** Username cannot be empty.").await;
        return Ok(());
    }

    if password.is_empty() {
        send_error_response(bnd, "❌ **Invalid Input:** Password cannot be empty.").await;
        return Ok(());
    }

    let uid = match reg.pg.create_account(&name, &password, is_register).await {
        Ok(account_data) => account_data,
        Err(e) => {
            let error_debug = format!("{:?}", e);

            let msg = if error_debug.contains("DuplicateUser") {
                "⚠️ **IDENTITY CONFLICT DETECTED**\n\nThe username you provided is already registered. Please choose a different username.".to_string()
            } else if error_debug.contains("InvalidPassword") {
                "❌ **INVALID PASSWORD**\n\nThe password you entered is incorrect. Please verify your credentials and try again.\n\n**Tips:**\n• Make sure Caps Lock is off\n• Check for typos in your password\n• If you forgot your password, use command /change_password or contact a moderator".to_string()
            } else if error_debug.contains("AlreadyBound") {
                let ticket_channel_id = bnd.init.log_channel.ticket_channel_id;
                format!(
                    "❌ **ACCOUNT ALREADY BOUND**\n\n\
                    This game account is already linked to another Discord user.\n\n\
                    **What this means:**\n\
                    • Someone else has already registered this account to their Discord\n\
                    • Each game account can only be bound to **ONE** Discord user\n\n\
                    **What to do:**\n\
                    • If this is your account, contact an admin by opening a ticket here: <#{}> | open-ticket\n\
                    • If you made a typo, try again with the correct username",
                    ticket_channel_id
                )
            } else if error_debug.contains("CharacterNotOwned") {
                "❌ **CHARACTER NOT OWNED**\n\nYou don't have permission to access this character. This character belongs to another account.".to_string()
            } else if error_debug.contains("RowNotFound") {
                if is_register {
                    "❌ **REGISTRATION FAILED**\n\nAn unexpected error occurred during registration. Please try again.".to_string()
                } else {
                    "❌ **ACCOUNT NOT FOUND**\n\nThe username you provided does not exist in our database.\n\n**How to fix:**\n• Double-check your username spelling\n• Make sure you're using the correct MHFZ username\n• If you haven't created an account yet, use `/create` instead".to_string()
                }
            } else if !is_register && error_debug.contains("NoCharacter") {
                "❌ **NO CHARACTER DETECTED**\n\nBinding failed. Please create an in-game character first.\n\n**How to fix:**\n1. Launch MHFZ with your account\n2. Create a new character in the game\n3. Enter town at least once\n4. Try binding again with `/bind`".to_string()
            } else {
                error!("Database error during account operation: {:?}", e);
                "❌ **DATABASE ERROR:** Operation failed. Please try again later. If the problem persists, contact a moderator.".to_string()
            };

            send_error_response(bnd, &msg).await;
            return Ok(()); 
        }
    };

    let resp = RegisterAcknowledged::new(&name, uid.id, bnd);
    let _ = resp.log_to_user(is_register).await;
    let _ = resp.add_roles().await;
    
    let success_msg = if is_register {
		"✅ **Account successfully created!**\n\n\
		Your account has been registered.\n\n\
		**📋 Next Steps:**\n\
		1. Type the `/switch` command here and click `use` on the template character (it uses Great Sword and is HR1).\n\
		2. Login through the launcher using the case-sensitive credentials you registered.\n\
		3. Launch the game and create your character in-game."
	} else {
		"✅ **Account successfully bound!**\n\n\
		Your account has been linked and you can now use all Discord features.\n\n\
		**📋 Next Step:**\n\
		Use `/switch` command to select which character you want to use for Discord features"
	};
    
    send_success_response(bnd, &success_msg).await;

    Ok(())
}