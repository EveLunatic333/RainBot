use crate::reusable::component::Mytrait;
use crate::event::interaction::Mybundle;
use std::time::Duration;
use serenity::all::*;
use serenity::futures::StreamExt;
use crate::{MyErr, SlashBundle, PgConn, Components, ComponentBundle};
use crate::reusable::component::bounty::{Methode, BBQ, Category, BountySubmit, Bounty, BountyTitle, BountyRefresh};
use serenity::all::{CommandInteraction, Context, Message};
use crate::reusable::component::shutdown::Shutdown;
use tracing::{warn, error};

const ITEMS_PER_PAGE: usize = 23;
const MENU_TIMEOUT: Duration = Duration::from_secs(120);
const SAME_CATEGORY_COOLDOWN: i64 = 40 * 60 * 60;
const DIFF_CATEGORY_COOLDOWN: i64 = 20 * 60 * 60;
const MAX_ATTACHMENTS: usize = 4;

fn create_select_menu(custom_id: &str, options: Vec<(String, String)>, disabled: bool) -> CreateSelectMenu {
    let menu_options: Vec<_> = options.iter().map(|(value, label)| CreateSelectMenuOption::new(label, value)).collect();
    CreateSelectMenu::new(custom_id, CreateSelectMenuKind::String { options: menu_options.into() })
        .min_values(1)
        .max_values(1)
        .disabled(disabled)
}

fn create_paginated_select_menu(custom_id: &str, all_options: &[(String, String)], page: usize, disabled: bool) -> CreateSelectMenu {
    let start_idx = page * ITEMS_PER_PAGE;
    let end_idx = (start_idx + ITEMS_PER_PAGE).min(all_options.len());
    let mut menu_options = Vec::new();
    if page > 0 {
        menu_options.push(CreateSelectMenuOption::new("⬅️ Previous Page", "page_prev"));
    }
    for i in start_idx..end_idx {
        let full_label = format!("{} - {}", all_options[i].0, all_options[i].1);
        let truncated_label = if full_label.len() > 100 { format!("{}...", &full_label[..97]) } else { full_label };
        menu_options.push(CreateSelectMenuOption::new(truncated_label, &all_options[i].0));
    }
    if end_idx < all_options.len() {
        menu_options.push(CreateSelectMenuOption::new("Next Page ➡️", "page_next"));
    }
    CreateSelectMenu::new(custom_id, CreateSelectMenuKind::String { options: menu_options.into() })
        .min_values(1)
        .max_values(1)
        .disabled(disabled)
}

fn build_action_rows(menus: Vec<CreateSelectMenu>, buttons: Vec<CreateButton>) -> Vec<CreateActionRow> {
    let mut rows: Vec<CreateActionRow> = menus.into_iter().map(CreateActionRow::SelectMenu).collect();
    if !buttons.is_empty() {
        rows.push(CreateActionRow::Buttons(buttons));
    }
    rows
}

fn render_submission_ui(all_bbq: &[(String, String)], page: usize, method_selected: bool, category_selected: bool, bbq_selected: bool) -> Vec<CreateActionRow> {
    let method_menu = create_select_menu("method_sel", Methode::option_str(), method_selected);
    let category_menu = create_select_menu("category_sel", Category::option_str(), category_selected);
    let bbq_menu = create_paginated_select_menu("bbq_sel", all_bbq, page, bbq_selected);
    let mut buttons = vec![Components::normal_button("Reset", "reset", ButtonStyle::Danger, "🔄")];
    if method_selected && category_selected && bbq_selected {
        buttons.insert(0, Components::normal_button("Done", "done", ButtonStyle::Success, "✅"));
    }
    build_action_rows(vec![method_menu, category_menu, bbq_menu], buttons)
}

fn render_distribution_ui(all_bbq: &[(String, String)], page: usize, method_selected: bool, category_selected: bool, bbq_selected: bool) -> Vec<CreateActionRow> {
    let method_menu = create_select_menu("d_methode", Methode::option_str(), method_selected);
    let category_menu = create_select_menu("d_cat", Category::option_str(), category_selected);
    let bbq_menu = create_paginated_select_menu("d_bbq", all_bbq, page, bbq_selected);
    let mut buttons = vec![
        Components::normal_button("Reset", "d_reset", ButtonStyle::Danger, "🔄"),
        Components::normal_button("Cancel", "d_cancel", ButtonStyle::Secondary, "✖️"),
    ];
    if method_selected && category_selected && bbq_selected {
        buttons.insert(0, Components::normal_button("OK", "d_ok", ButtonStyle::Success, "✅"));
    }
    build_action_rows(vec![method_menu, category_menu, bbq_menu], buttons)
}

async fn check_command_enabled(bnd: &SlashBundle<'_>) -> Result<bool, MyErr> {
    let cmd_name = bnd.cmd.data.name.as_str();
    let is_enabled = bnd.init.commands_toggle.get(cmd_name).unwrap_or(&true);
    if !is_enabled {
        let msg = format!("❌ Command Disabled: The `{}` feature is currently deactivated by the Administrator.", cmd_name);
        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(msg).ephemeral(true));
        let _ = bnd.cmd.create_response(&bnd.ctx.http, response).await;
        return Ok(false);
    }
    Ok(true)
}

fn parse_menu_value(value: &str) -> Option<u8> {
    if value == "page_prev" || value == "page_next" {
        return None;
    }
    value.parse::<u8>().ok()
}

async fn safe_get_msg(cmd: &CommandInteraction, ctx: &Context) -> Result<Message, MyErr> {
    match cmd.get_response(&ctx.http).await {
        Ok(msg) => Ok(msg),
        Err(e) => {
            if e.to_string().contains("10008") || e.to_string().contains("Unknown Message") {
                return Err(MyErr::Custom("Message context expired. Please retry the command.".to_string()));
            }
            Err(MyErr::from(e))
        }
    }
}

async fn process_submission(bnd: &SlashBundle<'_>, attachment_urls: Vec<String>, mention_string: String) -> Result<(), MyErr> {
    if !check_command_enabled(bnd).await? { return Ok(()); }
    let user_id_str = bnd.cmd.user.id.to_string();
    if let Some(existing_submission) = BountySubmit::open(&user_id_str).await {
        let judge_channel_id = bnd.init.bounty.judge_ch;
        let mut is_valid_submission = false;
        if existing_submission.msg_id != 0 {
            match ChannelId::new(judge_channel_id)
                .message(&bnd.ctx.http, existing_submission.msg_id)
                .await
            {
                Ok(_) => {
                    is_valid_submission = true;
                }
                Err(_) => {
                    warn!(user_id = %user_id_str, msg_id = existing_submission.msg_id, "Auto-cleaning stuck submission (message not found in judge channel)");
                    existing_submission.delete().await;
                }
            }
        } else {
            warn!(user_id = %user_id_str, "Auto-cleaning stuck submission (missing msg_id)");
            existing_submission.delete().await;
        }
        if is_valid_submission {
            let error_msg = "⏳ **Ongoing Submission Detected:**\nYou currently have a bounty submission pending review by the Guild Gals.\n\n⚙️ **System Notice:**\nPlease wait for your previous submission to be processed. If a submission remains pending for more than 24 hours without an admin response, the system will automatically cancel it and clear your status.";
            let response = CreateInteractionResponseMessage::new().content(error_msg).ephemeral(true);
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
            warn!(user_id = %bnd.cmd.user.id, "Blocked concurrent bounty submission attempt.");
            return Ok(());
        }
    }
    // Atomically reserve the in-progress slot so two concurrent /bounty submit
    // flows from the same user cannot both proceed and overwrite each other's
    // pending data. The reservation is always released after the flow ends.
    if !BountySubmit::reserve(&user_id_str).await {
        let error_msg = "📝 **Submission Already In Progress:**\nYou are already filling out a bounty submission form. Please finish that form first, or wait for it to time out, before submitting again.";
        let response = CreateInteractionResponseMessage::new().content(error_msg).ephemeral(true);
        let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;
        warn!(user_id = %user_id_str, "Blocked concurrent bounty submission form.");
        return Ok(());
    }
    let result = submission_flow(bnd, attachment_urls, mention_string).await;
    BountySubmit::unreserve(&user_id_str).await;
    result
}

async fn submission_flow(bnd: &SlashBundle<'_>, attachment_urls: Vec<String>, mention_string: String) -> Result<(), MyErr> {
    let user_id_str = bnd.cmd.user.id.to_string();
    bnd.cmd.defer(&bnd.ctx.http).await.map_err(MyErr::from)?;
    let submitter_id = bnd.cmd.user.id;
    let mut target_users = vec![submitter_id];
    let mut clean_mentions = Vec::new();
    for user_id in Components::get_mentions(&mention_string) {
        if !target_users.contains(&user_id) {
            target_users.push(user_id);
            clean_mentions.push(user_id);
        } else {
            warn!(user_id = %user_id, submitter_id = %submitter_id, "Duplicate reward mention detected and blocked");
        }
    }
    let mut validation_errors = Vec::new();
    let msg_main = safe_get_msg(bnd.cmd, bnd.ctx).await?;
    for uid in &target_users {
        let pg_conn = match PgConn::create(bnd.init, uid.to_string()).await {
            Ok(conn) => conn,
            Err(e) => {
                warn!(user_id = %uid, error = %e, "Failed to create database connection for bounty validation");
                validation_errors.push(format!("- <@{}> database connection failed.", uid));
                continue;
            }
        };
        if pg_conn.get_event().await.is_err() {
            validation_errors.push(format!("- <@{}> is not registered or character-bound.", uid));
        }
    }
    if !validation_errors.is_empty() {
        let error_msg = format!("❌ **Bounty Submission Blocked!**\nThe following validation issues were detected:\n{}", validation_errors.join("\n"));
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(error_msg)).await?;
        if let Some(state) = BountySubmit::open(&user_id_str).await { state.delete().await; }
        return Ok(());
    }
    let all_bbq = BBQ::option_str();
    let mut current_page = 0;
    let mut selected_method: Option<u8> = None;
    let mut selected_category: Option<u8> = None;
    let mut selected_bbq: Option<u8> = None;
    let mut status_message = "📝 **Bounty Details Required:** Please select all options below:".to_string();
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new()
        .content(&status_message)
        .components(render_submission_ui(&all_bbq, current_page, selected_method.is_some(), selected_category.is_some(), selected_bbq.is_some()))
    ).await?;
    let mut interaction_stream = msg_main.await_component_interactions(&bnd.ctx).timeout(MENU_TIMEOUT).stream();
    while let Some(interaction) = interaction_stream.next().await {
        if interaction.user.id != bnd.cmd.user.id {
            let _ = interaction.response(&bnd.ctx, Components::interaction_response("This menu is restricted to the command initiator.", true)).await;
            continue;
        }
        let _ = interaction.defer(&bnd.ctx.http).await;
        let custom_id = interaction.data.custom_id.clone();
        match custom_id.as_str() {
            "reset" => {
                selected_method = None; selected_category = None; selected_bbq = None; current_page = 0;
                status_message = "🔄 **Selection Cleared:** Please select your bounty details again:".to_string();
            }
            "done" => {
                if selected_method.is_some() && selected_category.is_some() && selected_bbq.is_some() { break; }
            }
            "method_sel" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                    if let Some(val) = values.first().and_then(|v| parse_menu_value(v)) { selected_method = Some(val); }
                }
            }
            "category_sel" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                    if let Some(val) = values.first().and_then(|v| parse_menu_value(v)) { selected_category = Some(val); }
                }
            }
            "bbq_sel" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                    if let Some(val) = values.first() {
                        match val.as_str() {
                            "page_next" => current_page += 1,
                            "page_prev" if current_page > 0 => current_page -= 1,
                            other => { if let Some(parsed) = parse_menu_value(other) { selected_bbq = Some(parsed); } }
                        }
                    }
                }
            }
            _ => {}
        }
        if selected_method.is_some() && selected_category.is_some() && selected_bbq.is_some() {
            status_message = "✅ **Ready to Submit:** All details selected. Click **Done** to finalize or **Reset** to adjust.".to_string();
        }
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new()
            .content(&status_message)
            .components(render_submission_ui(&all_bbq, current_page, selected_method.is_some(), selected_category.is_some(), selected_bbq.is_some()))
        ).await?;
    }
    if let (Some(method_val), Some(cat_val), Some(bbq_val)) = (selected_method, selected_category, selected_bbq) {
        let final_result = handle_final_submission(bnd, &attachment_urls, &clean_mentions, method_val, cat_val, bbq_val, &target_users, submitter_id, &msg_main).await;
        if final_result.is_err() {
            if let Some(state) = BountySubmit::open(&user_id_str).await { state.delete().await; }
        }
        return final_result;
    } else {
        if let Some(state) = BountySubmit::open(&user_id_str).await { state.delete().await; }
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ **Cancelled:** Selection timed out or was aborted. Your pending status has been cleared.").components(vec![])).await?;
        Ok(())
    }
}

async fn handle_final_submission(bnd: &SlashBundle<'_>, urls: &[String], mentions: &[UserId], method_val: u8, cat_val: u8, bbq_val: u8, target_users: &[UserId], submitter_id: UserId, msg_main: &Message) -> Result<(), MyErr> {
    let category = Category::new(cat_val)?;
    let bbq_type = BBQ::new(bbq_val)?;
    let mut pg = PgConn::create(bnd.init, bnd.cmd.user.id.to_string()).await?;
    let bounty_data = Box::new(Bounty::new(&category).await?);
    let mut cooldown_config = Box::new(BountyRefresh::new(true).await?);
    let member = bnd.cmd.member.clone().ok_or_else(|| MyErr::Custom("Guild member data unavailable".into()))?;
    let mut cooldown_violations = Vec::new();
    for uid in target_users {
        let pg_temp = PgConn::create(bnd.init, uid.to_string()).await?;
        if let Ok(event) = pg_temp.get_event().await {
            let parts: Vec<&str> = event.latest_bounty.split('_').collect();
            if parts.len() >= 2 {
                if let (Some(hist_cat), Some(hist_bbq)) = (parts[0].parse::<u8>().ok().and_then(|v| Category::new(v).ok()), parts[1].parse::<u8>().ok().and_then(|v| BBQ::new(v).ok())) {
                    let cooldown_duration = if hist_cat == category && hist_bbq == bbq_type { SAME_CATEGORY_COOLDOWN } else { DIFF_CATEGORY_COOLDOWN };
                    let cooldown_end = event.latest_bounty_time + cooldown_duration;
                    let current_time = crate::reusable::utils::MyTime::now();
                    if cooldown_end > current_time && category != Category::Event {
                        cooldown_violations.push(format!("- <@{}> is still on cooldown until <t:{}:R>.", uid, cooldown_end));
                    }
                }
            }
        }
    }
    let is_free_cooldown = if category == Category::Free {
        let cooldown_value = match bbq_type {
            BBQ::BBQ01 => cooldown_config.bbq1, BBQ::BBQ02 => cooldown_config.bbq2, BBQ::BBQ03 => cooldown_config.bbq3,
            BBQ::BBQ04 => cooldown_config.bbq4, BBQ::BBQ05 => cooldown_config.bbq5, BBQ::BBQ06 => cooldown_config.bbq6,
            BBQ::BBQ07 => cooldown_config.bbq7, BBQ::BBQ08 => cooldown_config.bbq8, BBQ::BBQ09 => cooldown_config.bbq9,
            BBQ::BBQ10 => cooldown_config.bbq10, BBQ::BBQ11 => cooldown_config.bbq11, BBQ::BBQ12 => cooldown_config.bbq12,
            BBQ::BBQ13 => cooldown_config.bbq13, BBQ::BBQ14 => cooldown_config.bbq14, BBQ::BBQ15 => cooldown_config.bbq15,
            BBQ::BBQ16 => cooldown_config.bbq16, BBQ::BBQ17 => cooldown_config.bbq17, BBQ::BBQ18 => cooldown_config.bbq18,
            BBQ::BBQ19 => cooldown_config.bbq19, BBQ::BBQ20 => cooldown_config.bbq20, BBQ::BBQ21 => cooldown_config.bbq21,
            BBQ::BBQ22 => cooldown_config.bbq22, BBQ::BBQ23 => cooldown_config.bbq23, BBQ::BBQ24 => cooldown_config.bbq24,
            BBQ::BBQ25 => cooldown_config.bbq25,
        };
        cooldown_value == 0
    } else {
        false
    };
    if is_free_cooldown { cooldown_violations.push("- This Free Bounty category is currently on global server cooldown.".to_string()); }
    if !cooldown_violations.is_empty() {
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("❌ **Cooldown Detected!**\nSubmission blocked due to:\n{}", cooldown_violations.join("\n"))).components(vec![])).await?;
        return Ok(());
    }
    let primary_url = urls.first().ok_or_else(|| MyErr::Custom("No attachment URL available".into()))?;
    let mut submission = BountySubmit::new(bnd, false, *member, mentions.to_vec(), &mut pg, &bounty_data, primary_url, Methode::new(method_val), bbq_type, category.clone()).await?;
    if !submission.cooldown(&mut cooldown_config).await.unwrap_or(false) {
        submission.delete().await;
        error!(user_id = %bnd.cmd.user.id, "Final cooldown verification failed during bounty submission");
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ **Submission Error:** Cooldown verification failed at the final step.").components(vec![])).await?;
        return Ok(());
    }
    let all_urls_text = urls.join("\n");
    let mut confirmation_msg = format!("✅ **Bounty Successfully Submitted!**\nThe Guild Gals are reviewing your submission.\n\n👤 **Submitter:** <@{}>", submitter_id);
    if !mentions.is_empty() {
        confirmation_msg.push_str(&format!("\n👥 **Party Members:** {}", mentions.iter().map(|id| format!(" <@{}>", id)).collect::<Vec<_>>().join(", ")));
    }
    confirmation_msg.push_str(&format!("\n\n🔗 **Attachment Preview ({} Images):**\n{}", urls.len(), all_urls_text));
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&confirmation_msg).components(vec![])).await?;
    let jump_url = bnd.cmd.data.resolved.messages.values().next().map(|msg| {
        let guild_str = msg.guild_id.map(|g| g.get().to_string()).unwrap_or_else(|| "@me".to_string());
        format!("https://discord.com/channels/{}/{}/{}", guild_str, msg.channel_id.get(), msg.id.get())
    }).or_else(|| {
        let guild_str = bnd.cmd.guild_id.map(|g| g.get().to_string()).unwrap_or_else(|| "@me".to_string());
        Some(format!("https://discord.com/channels/{}/{}/{}", guild_str, msg_main.channel_id.get(), msg_main.id.get()))
    });
    let mut final_components = submission.button();
    if let Some(url) = jump_url {
        final_components.push(CreateActionRow::Buttons(vec![CreateButton::new_link(url).label("View Submission").emoji('🔗')]));
    }
    let judge_embed = submission.embed().image(primary_url.clone());
    let judge_msg = ChannelId::new(bnd.init.bounty.judge_ch)
        .send_message(&bnd.ctx.http, CreateMessage::new()
            .embed(judge_embed)
            .components(final_components))
        .await?;
    submission.msg_id = judge_msg.id.get();
    submission.save(&bnd.user().id.to_string()).await;
    if let Err(e) = cooldown_config.cooldown(bnd).await {
        submission.delete().await;
        return Err(e);
    }
    if let Err(e) = cooldown_config.save(true).await {
        submission.delete().await;
        return Err(e);
    }
    if let Err(e) = Shutdown::save().await {
        submission.delete().await;
        return Err(e.into());
    }
    Ok(())
}

#[hertz::hertz_slash_normal(60, false)]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !check_command_enabled(bnd).await? { return Ok(()); }
    let msg = bnd.cmd.data.resolved.messages.values().next().ok_or_else(|| MyErr::Custom("Referenced message not found".into()))?;
    if msg.attachments.is_empty() { return Err(MyErr::Custom("No attachment found in referenced message".into())); }
    let urls: Vec<String> = msg.attachments.iter().take(MAX_ATTACHMENTS).map(|a| a.url.clone()).collect();
    let mention_string = msg.mentions.iter().map(|u| u.id.to_string()).collect::<Vec<_>>().join(" ");
    process_submission(bnd, urls, mention_string).await
}

pub(super) async fn submit(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !check_command_enabled(bnd).await? { return Ok(()); }
    if bnd.cmd.data.resolved.attachments.is_empty() { return Err(MyErr::Custom("No image attachment provided".into())); }
    let urls: Vec<String> = bnd.cmd.data.resolved.attachments.values().take(MAX_ATTACHMENTS).map(|a| a.url.clone()).collect();
    let mut mention_string = String::new();
    if let Ok(options) = Components::sub_options(bnd) {
        for opt in options {
            if opt.name == "mentions" {
                if let CommandDataOptionValue::String(value) = &opt.value { mention_string = value.clone(); }
            }
        }
    }
    process_submission(bnd, urls, mention_string).await
}

async fn try_distrib(bnd: &SlashBundle<'_>) -> Result<(), String> {
    if !check_command_enabled(bnd).await.map_err(|e| format!("Command check failed: {:?}", e))? { return Ok(()); }
    bnd.cmd.defer(&bnd.ctx.http).await.map_err(|e| format!("Failed to defer interaction: {:?}", e))?;
    let mut mention_string = String::new();
    for opt in &bnd.cmd.data.options {
        if opt.name == "mentions" {
            match &opt.value {
                CommandDataOptionValue::String(value) => mention_string = value.clone(),
                CommandDataOptionValue::User(user_id) => mention_string = format!("<@{}>", user_id.get()),
                _ => {}
            }
        }
    }
    let mut target_users = Components::get_mentions(&mention_string);
    if target_users.is_empty() { target_users.push(bnd.cmd.user.id); }
    let all_bbq = BBQ::option_str();
    let mut current_page = 0;
    let mut selected_method: Option<u8> = None;
    let mut selected_category: Option<u8> = None;
    let mut selected_bbq: Option<u8> = None;
    let mut status_message = format!("🛠️ **Admin Manual Distribution**\nSelect **Method**, **Category**, and **BBQ** to distribute to **{}** users.", target_users.len());
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&status_message).components(render_distribution_ui(&all_bbq, current_page, selected_method.is_some(), selected_category.is_some(), selected_bbq.is_some()))).await.map_err(|e| format!("Failed to render initial UI: {:?}", e))?;
    let msg_main = bnd.cmd.get_msg(bnd.ctx).await.map_err(|e| format!("Failed to get message context: {:?}", e))?;
    let mut interaction_stream = msg_main.await_component_interactions(&bnd.ctx).timeout(MENU_TIMEOUT).stream();
    let mut cancelled = false;
    while let Some(interaction) = interaction_stream.next().await {
        if interaction.user.id != bnd.cmd.user.id {
            let _ = interaction.response(&bnd.ctx, Components::interaction_response("Admin command restricted to executor.", true)).await;
            continue;
        }
        interaction.defer(&bnd.ctx.http).await.map_err(|e| format!("Failed to defer interaction: {:?}", e))?;
        let custom_id = interaction.data.custom_id.clone();
        match custom_id.as_str() {
            "d_cancel" => { cancelled = true; break; }
            "d_reset" => { selected_method = None; selected_category = None; selected_bbq = None; current_page = 0; status_message = "🔄 **Selection Cleared:** Please select Method, Category, and BBQ again:".to_string(); }
            "d_ok" => { if selected_method.is_some() && selected_category.is_some() && selected_bbq.is_some() { break; } }
            "d_methode" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind { selected_method = values.first().and_then(|v| parse_menu_value(v)); }
            }
            "d_cat" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind { selected_category = values.first().and_then(|v| parse_menu_value(v)); }
            }
            "d_bbq" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                    if let Some(val) = values.first() {
                        match val.as_str() {
                            "page_next" => current_page += 1,
                            "page_prev" if current_page > 0 => current_page -= 1,
                            other => selected_bbq = parse_menu_value(other),
                        }
                    }
                }
            }
            _ => {}
        }
        if selected_method.is_some() && selected_category.is_some() && selected_bbq.is_some() {
            status_message = "✅ **Ready to Distribute:** Click **OK** to confirm, or **Reset** to adjust.".to_string();
        }
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&status_message).components(render_distribution_ui(&all_bbq, current_page, selected_method.is_some(), selected_category.is_some(), selected_bbq.is_some()))).await.map_err(|e| format!("Failed to update UI: {:?}", e))?;
    }
    if cancelled {
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🛑 **Distribution Cancelled by Admin.**").components(vec![])).await.map_err(|e| format!("Failed to send cancellation message: {:?}", e))?;
        return Ok(());
    }
    if let (Some(method_val), Some(cat_val), Some(bbq_val)) = (selected_method, selected_category, selected_bbq) {
        execute_distribution(bnd, &target_users, method_val, cat_val, bbq_val).await
    } else {
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ **Cancelled:** Selection timed out without completing all required fields.").components(vec![])).await.map_err(|e| format!("Failed to send timeout message: {:?}", e))?;
        Ok(())
    }
}

async fn execute_distribution(bnd: &SlashBundle<'_>, target_users: &[UserId], method_val: u8, cat_val: u8, bbq_val: u8) -> Result<(), String> {
    let category = Category::new(cat_val).map_err(|_| "Failed to parse category value".to_string())?;
    let bbq_type = BBQ::new(bbq_val).map_err(|_| "Failed to parse BBQ value".to_string())?;
    let bounty_data = Box::new(Bounty::new(&category).await.map_err(|e| format!("Failed to fetch bounty data: {:?}", e))?);
    let dummy_image_url = "https://discord.com/assets/none.png".to_string();
    let guild_id = bnd.cmd.guild_id.ok_or("Command must be executed within a guild context")?;
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ **Processing Reward Distribution...** Executing database operations.").components(vec![])).await.map_err(|e| format!("Failed to send processing message: {:?}", e))?;
    let mut successful_users = Vec::new();
    let mut skipped_users = Vec::new();
    for uid in target_users {
        let mut pg_conn = match PgConn::create(bnd.init, uid.to_string()).await {
            Ok(conn) => conn,
            Err(_) => { warn!(user_id = %uid, "Failed to create database connection for distribution"); skipped_users.push(*uid); continue; }
        };
        if pg_conn.get_event().await.is_err() { warn!(user_id = %uid, "User not registered, skipping distribution"); skipped_users.push(*uid); continue; }
        let target_member = match guild_id.member(&bnd.ctx.http, *uid).await {
            Ok(member) => member,
            Err(_) => { warn!(user_id = %uid, "Failed to fetch guild member, skipping"); skipped_users.push(*uid); continue; }
        };
        let mut submission = BountySubmit::new(bnd, true, target_member, vec![], &mut pg_conn, &bounty_data, &dummy_image_url, Methode::new(method_val), bbq_type.clone(), category.clone()).await.map_err(|e| format!("Failed to initialize reward submission: {:?}", e))?;
        submission.reward(true, bnd, &mut pg_conn).await.map_err(|e| format!("Failed to apply reward: {:?}", e))?;
        let _ = ChannelId::new(bnd.init.bounty.conquered_ch).send_message(&bnd.ctx.http, CreateMessage::new().embed(submission.embed())).await;
        successful_users.push(*uid);
    }
    let mut report = String::from("✅ **Mass Distribution Complete!**\n");
    if !successful_users.is_empty() {
        report.push_str(&format!("\n🎯 **Successfully Distributed ({} users):**\n{}", successful_users.len(), successful_users.iter().map(|id| format!(" <@{}>", id)).collect::<Vec<_>>().join(", ")));
    }
    if !skipped_users.is_empty() {
        report.push_str(&format!("\n\n⚠️ **Skipped (Not Registered or Left Server - {} users):**\n{}", skipped_users.len(), skipped_users.iter().map(|id| format!(" <@{}>", id)).collect::<Vec<_>>().join(", ")));
    }
    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&report).components(vec![])).await.map_err(|e| format!("Failed to send distribution report: {:?}", e))?;
    Ok(())
}

#[hertz::hertz_slash_normal(0, false)]
async fn distribution(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !check_command_enabled(bnd).await? { return Ok(()); }
    match try_distrib(bnd).await {
        Ok(_) => Ok(()),
        Err(error_msg) => {
            error!(error = %error_msg, "Fatal error in admin distribution workflow");
            let _ = bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("❌ **System Error Captured:**\n`{}`\nPlease report this to the developer.", error_msg)).components(vec![])).await;
            Ok(())
        }
    }
}

#[hertz::hertz_button_normal(0, false)]
async fn button(bnd: &ComponentBundle<'_>) -> Result<(), MyErr> {
    let mut code_parts = bnd.cmd.data.custom_id.split('_');
    let prefix = code_parts.next().unwrap_or_default();
    if prefix != "bounty" { return Ok(()); }
    let user_id = code_parts.next().ok_or_else(|| MyErr::Custom("Missing user identifier in button payload".into()))?;
    let action_state = code_parts.next().ok_or_else(|| MyErr::Custom("Missing action state in button payload".into()))?;
    let mut submission = match BountySubmit::open(user_id).await {
        Some(data) => data,
        None => {
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("❌ Submission data has expired or been processed.").ephemeral(true))).await;
            return Ok(());
        }
    };
    let original_message = bnd.cmd.message.clone();
    let jump_url = original_message.components.iter().flat_map(|row| &row.components).find_map(|comp| {
        if let ActionRowComponent::Button(btn) = comp {
            serde_json::to_value(btn).ok().and_then(|json| json.get("url")?.as_str().map(str::to_string))
        } else { None }
    });
    match action_state {
        "c" => {
            let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("⏳ Fetching bounty details...").ephemeral(true))).await;
            let bounty_data = Box::new(Bounty::new(&submission.category).await?);
            let embed = bounty_data.desc(bnd, &submission.bbq, &submission.category)?;
            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("✅ **Bounty Details Retrieved:**").embed(embed)).await?;
            Ok(())
        }
        "r" => {
            let reject_buttons = vec![
                Components::normal_button("Invalid Image", &format!("bounty_{}_rm1", user_id), ButtonStyle::Danger, "🖼️"),
                Components::normal_button("Wrong Equipment", &format!("bounty_{}_rm2", user_id), ButtonStyle::Danger, "⚔️"),
                Components::normal_button("Rule Violation", &format!("bounty_{}_rm3", user_id), ButtonStyle::Danger, "📜"),
                Components::normal_button("Custom Reason", &format!("bounty_{}_rm4", user_id), ButtonStyle::Primary, "✍️"),
                Components::normal_button("Cancel", &format!("bounty_{}_rm5", user_id), ButtonStyle::Secondary, "✖️"),
            ];
            let mut action_rows = vec![CreateActionRow::Buttons(reject_buttons)];
            if let Some(url) = &jump_url {
                action_rows.push(CreateActionRow::Buttons(vec![CreateButton::new_link(url).label("View Submission").emoji('🔗')]));
            }
            bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new().content("⚠️ **Rejecting Submission:** Select a reason:").components(action_rows))).await?;
            Ok(())
        }
        state if state.starts_with("rm") => {
            bnd.cmd.defer(&bnd.ctx.http).await?;
            let mut final_components = submission.button();
            if let Some(url) = &jump_url {
                final_components.push(CreateActionRow::Buttons(vec![CreateButton::new_link(url).label("View Submission").emoji('🔗')]));
            }
            if state == "rm5" {
                bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🔄 **Review Cancelled.**").components(final_components)).await?;
                return Ok(());
            }
            let mut reason = match state {
                "rm1" => "Screenshot is unclear or does not meet requirements.".to_string(),
                "rm2" => "Equipment shown does not match the bounty requirements.".to_string(),
                "rm3" => "Submission violates server or bounty rules.".to_string(),
                "rm4" => String::new(),
                _ => "Unspecified rejection reason.".to_string(),
            };
            if state == "rm4" {
                let expiry_timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 60;
                bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(format!("⚠️ **Custom Reason:** Please type your reason now. Expires <t:{}:R>\n❌ Type `cancel` to abort.", expiry_timestamp)).components(vec![])).await?;
                if let Some(reply) = original_message.channel_id.await_reply(&bnd.ctx).author_id(bnd.cmd.user.id).timeout(Duration::from_secs(60)).await {
                    let typed_reason = reply.content.trim().to_string();
                    let _ = reply.delete(&bnd.ctx.http).await;
                    if typed_reason.to_lowercase() == "cancel" {
                        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🔄 **Review Cancelled.**").components(final_components)).await?;
                        return Ok(());
                    }
                    reason = typed_reason;
                } else {
                    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ **Input Timeout.** Rejection cancelled.").components(final_components)).await?;
                    return Ok(());
                }
            }
            let rejection_message = format!("❌ **Bounty Rejected**\n**User:** <@{user_id}>\n**Guild Gal:** <@{}>\n**Reason:** {reason}\n\n*Please contact <@{}> if you require clarification or wish to discuss this evaluation.*", bnd.cmd.user.id, bnd.cmd.user.id);
            let _ = ChannelId::new(bnd.init.bounty.receptionist_ch).send_message(&bnd.ctx.http, CreateMessage::new().content(rejection_message)).await;
            // Return the consumed Free Bounty slot now that the submission was
            // rejected, so the available count is not permanently reduced.
            if submission.category == Category::Free {
                if let Err(e) = BountyRefresh::rejected(&submission.bbq, bnd).await {
                    error!(user_id = %user_id, error = %e, "Failed to restore Free Bounty count after rejection");
                }
            }
            submission.delete().await;
            let _ = original_message.delete(&bnd.ctx.http).await;
            Ok(())
        }
        "a" => {
            bnd.cmd.defer(&bnd.ctx.http).await?;
            let title_config = BountyTitle::new().await?;
            submission.title(bnd, &title_config).await?;
            let _ = ChannelId::new(bnd.init.bounty.receptionist_ch).send_message(&bnd.ctx.http, CreateMessage::new().content(format!("✅ The bounty submitted by <@{}> has been approved by {} ", user_id, bnd.cmd.user.name))).await;
            let mut pg_conn = PgConn::create(bnd.init, user_id.to_owned()).await?;
            let recipient_cid = match pg_conn.get_char_id_by_discord(user_id.parse::<u64>().unwrap_or(0)).await {
                Ok(cid) => cid,
                Err(_) => {
                    let _ = ChannelId::new(bnd.init.bounty.receptionist_ch).send_message(&bnd.ctx.http, CreateMessage::new().content(format!("❌ Failed to inject reward for <@{}> because user is not registered in the database.", user_id))).await;
                    submission.delete().await;
                    return Ok(());
                }
            };
            if !pg_conn.validate_character_exists(recipient_cid).await? {
                let _ = ChannelId::new(bnd.init.bounty.receptionist_ch).send_message(&bnd.ctx.http, CreateMessage::new().content(format!("❌ Failed to inject reward for <@{}> because character ID {} is invalid.", user_id, recipient_cid))).await;
                submission.delete().await;
                return Ok(());
            }
            let reward_injection_result = submission.reward(false, bnd, &mut pg_conn).await;
            submission.delete().await;
            let _ = original_message.delete(&bnd.ctx.http).await;
            if let Err(e) = reward_injection_result { return Err(e); }
            let _ = ChannelId::new(bnd.init.bounty.conquered_ch).send_message(&bnd.ctx.http, CreateMessage::new().embed(submission.embed())).await;
            Ok(())
        }
        _ => {
            warn!(custom_id = %bnd.cmd.data.custom_id, "Unknown bounty button action state");
            Ok(())
        }
    }
}