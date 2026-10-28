
use crate::reusable::component::Mytrait;
use std::time::Duration;
use serenity::all::*;
use serenity::futures::StreamExt;
use crate::{MyErr, SlashBundle, Components};
use crate::reusable::component::bounty::{BBQ, Category, Bounty};

#[hertz::hertz_slash_normal(60, false)]
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

    let options = bnd.cmd.data.options.first().ok_or_else(|| MyErr::Custom("No subcommand specified.".to_owned()))?;
    match options.name.as_str() {
        "submit" => super::submit::submit(bnd).await?,
        "pedia" => pedia(bnd).await?,
        _ => {}
    }
    Ok(())
}

fn select_menu(id: &str, op: Vec<(String, String)>) -> CreateSelectMenu {
    let options: Vec<_> = op.iter().map(|x| CreateSelectMenuOption::new(&x.1, &x.0)).collect();
    CreateSelectMenu::new(id, CreateSelectMenuKind::String { options: options.into() }).min_values(1).max_values(1)
}

fn paged_select_menu(id: &str, all_options: &[(String, String)], page: usize, disabled: bool) -> CreateSelectMenu {
    let mut current_options = Vec::new();
    let items_per_page = 23;
    let start_idx = page * items_per_page;
    let end_idx = std::cmp::min(start_idx + items_per_page, all_options.len());

    if page > 0 { current_options.push(CreateSelectMenuOption::new("⬅️ Previous Page", "page_prev")); }
    for i in start_idx..end_idx {
        let full_label = format!("{} - {}", all_options[i].0, all_options[i].1);
        let truncated_label = if full_label.len() > 100 { format!("{}...", &full_label[..97]) } else { full_label };
        current_options.push(CreateSelectMenuOption::new(truncated_label, &all_options[i].0));
    }
    if end_idx < all_options.len() { current_options.push(CreateSelectMenuOption::new("Next Page ➡️", "page_next")); }

    CreateSelectMenu::new(id, CreateSelectMenuKind::String { options: current_options.into() }).min_values(1).max_values(1).disabled(disabled)
}

fn render_pedia_components(all_bbq: &[(String, String)], page: usize, c: &Option<u8>, b: &Option<u8>) -> Vec<CreateActionRow> {
    let c_menu = select_menu("category_sel", Category::option_str()).disabled(c.is_some());
    let b_menu = paged_select_menu("bbq_sel", all_bbq, page, b.is_some());
    let mut buttons = vec![Components::normal_button("Reset", "reset_pedia", ButtonStyle::Danger, "🔄")];
    if c.is_some() && b.is_some() { buttons.insert(0, Components::normal_button("View Bounty", "view_pedia", ButtonStyle::Success, "🔍")); }
    vec![CreateActionRow::SelectMenu(c_menu), CreateActionRow::SelectMenu(b_menu), CreateActionRow::Buttons(buttons)]
}

pub(super) async fn pedia(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    let all_bbq = BBQ::option_str();
    let mut current_page = 0;
    let mut category: Option<u8> = None;
    let mut bbq: Option<u8> = None;
    let mut status_text = "📚 **Bounty Pedia:** Please select Category and BBQ below:".to_string();

    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().content(&status_text).components(render_pedia_components(&all_bbq, current_page, &category, &bbq))
    )).await.map_err(MyErr::from)?;

    let mut msg_main = bnd.cmd.get_msg(bnd.ctx).await?;
    let mut stream = msg_main.await_component_interactions(&bnd.ctx).timeout(Duration::from_secs(120)).stream();

    while let Some(m) = stream.next().await {
        if m.user.id != bnd.cmd.user.id {
            let _ = m.response(bnd.ctx, Components::interaction_response("This menu is restricted to the command initiator.", true)).await;
            continue;
        }
        let _ = m.defer(&bnd.ctx.http).await;

        match m.data.custom_id.as_str() {
            "reset_pedia" => { category = None; bbq = None; current_page = 0; status_text = "🔄 **Selection Cleared:** Please select again:".to_string(); }
            "view_pedia" => { if category.is_some() && bbq.is_some() { break; } }
            "category_sel" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &m.data.kind {
                    if let Some(val) = values.first() { category = val.parse::<u8>().ok(); }
                }
            }
            "bbq_sel" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &m.data.kind {
                    if let Some(val) = values.first() {
                        match val.as_str() {
                            "page_next" => current_page += 1,
                            "page_prev" if current_page > 0 => current_page -= 1,
                            other => bbq = other.parse::<u8>().ok(),
                        }
                    }
                }
            }
            _ => {}
        }

        if category.is_some() && bbq.is_some() { status_text = "✅ **Ready to View:** Click **View Bounty** to see the details, or **Reset** to pick another.".to_string(); }
        let _ = msg_main.edit(&bnd.ctx.http, EditMessage::new().content(&status_text).components(render_pedia_components(&all_bbq, current_page, &category, &bbq))).await;
    }

    if let (Some(c), Some(b)) = (category, bbq) {
        let cat = Category::new(c)?;
        let bbq_val = BBQ::new(b)?;
        let bounty_data = Box::new(Bounty::new(&cat).await?);
        let embed = bounty_data.desc(bnd, &bbq_val, &cat)?;
        let _ = msg_main.edit(&bnd.ctx.http, EditMessage::new().content("✅ **Bounty Pedia Retrieved:").embed(embed).components(vec![])).await;
    } else {
        let _ = msg_main.edit(&bnd.ctx.http, EditMessage::new().content("⏳ **Cancelled:** Selection timed out.").components(vec![])).await;
    }
    Ok(())
}