
use crate::reusable::component::Mytrait;
use std::time::Duration;
use crate::{MyErr, ItemPedia, Components, SlashBundle, Reg, reusable::{bitwise::ItemCode, component::{bounty::Title}}, event::interaction::ComponentBundle};
use serenity::{all::*, futures::StreamExt};
use tracing::warn;
use crate::reusable::component::market::{Market, Item, Trading};

fn render_market_components(filtered_items: &Vec<(String, String)>, page: usize, selected_item: &Option<String>, selected_qty: &Option<u16>, is_searching: bool) -> Vec<CreateActionRow> {
    let mut rows = Vec::new();
    let items_per_page = 23;
    let start_idx = page * items_per_page;
    let end_idx = std::cmp::min(start_idx + items_per_page, filtered_items.len());

    let mut item_options = Vec::new();
    for i in start_idx..end_idx {
        let label = if filtered_items[i].1.len() > 100 { format!("{}...", &filtered_items[i].1[..97]) } else { filtered_items[i].1.clone() };
        item_options.push(CreateSelectMenuOption::new(label, &filtered_items[i].0));
    }

    if item_options.is_empty() {
        item_options.push(CreateSelectMenuOption::new("❌ No items match your search", "none"));
    }

    let item_menu = CreateSelectMenu::new("m_item", CreateSelectMenuKind::String { options: item_options.into() })
        .placeholder(if is_searching { "🔍 Select from search results..." } else { "🛒 Select an Item..." })
        .min_values(1).max_values(1);
    rows.push(CreateActionRow::SelectMenu(item_menu));

    let qty_list = vec![1, 2, 3, 4, 5, 10, 15, 20, 25, 30, 40, 50, 100, 200, 500, 1000];
    let mut qty_options: Vec<_> = qty_list.iter().map(|q| CreateSelectMenuOption::new(format!("{} Pcs", q), q.to_string())).collect();
    // Allows players to type an exact amount (e.g. 14 or 33) instead of
    // placing multiple orders to reach the desired quantity.
    qty_options.push(CreateSelectMenuOption::new("✍️ Custom Amount...", "custom"));
    let qty_menu = CreateSelectMenu::new("m_qty", CreateSelectMenuKind::String { options: qty_options.into() })
        .placeholder("🔢 Select Quantity...").min_values(1).max_values(1).disabled(selected_item.is_none());
    rows.push(CreateActionRow::SelectMenu(qty_menu));

    let mut nav_buttons = Vec::new();
    nav_buttons.push(Components::normal_button("Prev", "m_prev", ButtonStyle::Primary, "⬅️").disabled(page == 0));
    if is_searching {
        nav_buttons.push(Components::normal_button("Clear Search", "m_search_clear", ButtonStyle::Danger, "✖️"));
    } else {
        nav_buttons.push(Components::normal_button("Search", "m_search", ButtonStyle::Secondary, "🔍"));
    }
    nav_buttons.push(Components::normal_button("Next", "m_next", ButtonStyle::Primary, "➡️").disabled(end_idx >= filtered_items.len()));
    rows.push(CreateActionRow::Buttons(nav_buttons));

    let mut action_buttons = vec![
        Components::normal_button("Clear", "m_clear", ButtonStyle::Danger, "🔄"),
        Components::normal_button("Cancel", "m_cancel", ButtonStyle::Secondary, "✖️"),
    ];

    if selected_item.is_some() && selected_qty.is_some() {
        action_buttons.insert(0, Components::normal_button("Order", "m_order", ButtonStyle::Success, "✅"));
    }
    rows.push(CreateActionRow::Buttons(action_buttons));
    rows
}

struct Handle {
    market: Market,
    item: Item,
    bought: u16,
}

impl Handle {
    async fn check(&self, reg: &Reg<'_>, pedia: &ItemPedia) -> Result<Bought, MyErr> {
        let (bc, event) = tokio::join!(reg.pg.get_coin(), reg.pg.get_event());
        let (bc, event) = (bc?, event?);
        let title = Title::new(event.title as u8).discount();
        let bon = (1.0 - title) * 100.0;
        let disc = format!("{}%", bon as u32);
        let total = self.item.price as i64 * self.bought as i64;
        let discounted = (total as f32 * title) as i64;
        let item_count = self.item.item.count as i64 * self.bought as i64;

        // Unlimited items skip the stock check entirely; limited items keep
        // the legacy behavior and also cover the out-of-stock (0) case.
        if !self.item.unlimited && self.bought as u32 > self.item.treshold {
            let msg = if self.item.treshold == 0 {
                format!("{} is currently out of stock. Please check back later.", self.item.item.text(pedia).unwrap_or_default())
            } else {
                format!("You bought {} item, but market now only had {} item", self.bought, self.item.treshold)
            };
            return Err(MyErr::Custom(msg));
        }
        if bc < discounted as i32 {
            return Err(MyErr::Custom(format!("You only have {} Bounty Coin but you need {} for transaction", Market::currency(bc as i64), Market::currency(discounted))));
        }
        if total > 2_147_483_647 {
            return Err(MyErr::Custom(format!("You try to begin transaction with {} BC, which exceeds the limit.", Market::currency(total))));
        }
        if item_count > 65_535 {
            return Err(MyErr::Custom(format!("You try to bought {item_count} total items. Our courier can only carry 65.535 items. Please reduce.")));
        }

        let change = bc as i64 - discounted;
        Ok(Bought {
            item: self.item.item.text(pedia).unwrap_or_default(),
            qty: self.bought as i64,
            total,
            change,
            former: bc,
            price: self.item.price as i32,
            unit: "item(s)".to_string(),
            discounted,
            disc,
        })
    }

    async fn transaction(&mut self, reg: &Reg<'_>, pedia: &ItemPedia) -> Result<(), MyErr> {
        let event = reg.pg.get_event().await?;
        let price = ((self.item.price * self.bought as u32) as f32 * Title::new(event.title as u8).discount()) as u32;
        let count = self.item.item.count * self.bought;
        // Unlimited items never deplete; limited items follow the legacy stock behavior.
        if !self.item.unlimited {
            self.item.treshold -= self.bought as u32;
        }
        self.market.bought(&self.item);
        let item = ItemCode {
            name: None,
            key: self.item.item.key.to_owned(),
            types: self.item.item.types,
            count,
        };
        reg.pg.market_user(&item, reg.cid, price, pedia).await?;
        Ok(())
    }

    /// Persists the remaining stock after a successful purchase.
    ///
    /// This used to also refresh the public "Server Market Stall" board. The board
    /// has been removed: it duplicated the `/trading market` menu, and editing it was
    /// the source of the 403 / 50005 failures whenever it had been posted by an older
    /// bot identity. Only the stock file still needs writing.
    async fn save_stock(&self) -> Result<(), MyErr> {
        self.market.save().await?;
        Ok(())
    }
}

pub struct Bought {
    item: String,
    qty: i64,
    total: i64,
    change: i64,
    former: i32,
    price: i32,
    discounted: i64,
    disc: String,
    unit: String,
}

impl Bought {
    pub fn new(item: String, qty: i64, total: i64, change: i64, former: i32, price: i32, unit: String, discounted: i64, disc: String) -> Self {
        Bought { item, qty, total, change, former, price, unit, discounted, disc }
    }

    pub fn create_embed(&self, bnd: &SlashBundle<'_>) -> CreateEmbed {
        CreateEmbed::new()
            .title("Receipt")
            .author(CreateEmbedAuthor::new(&bnd.cmd.user.name).icon_url(bnd.cmd.user.face()))
            .description(format!("{} Receipt at <t:{}:F>", &bnd.cmd.user, crate::reusable::utils::MyTime::now()))
            .field("Item", format!("{}\nBought x{} {}", &self.item, self.qty, &self.unit), false)
            .field("Price", format!("Per {}: {} x {} = {}", &self.unit, Market::currency(self.price as i64), self.qty, Market::currency(self.total)), false)
            .field("Discounted", format!("Discount {} for {} = {}", &self.disc, Market::currency(self.total), Market::currency(self.discounted)), false)
            .field("Currency", format!("Your Coin = {} - {} = {}", Market::currency(self.former as i64), Market::currency(self.discounted), Market::currency(self.change)), false)
            .color(crate::reusable::utils::Color::Random.throw())
    }

    async fn verify_caller(bnd: &SlashBundle<'_>, x: &ComponentInteraction) -> Result<bool, MyErr> {
        if x.user != bnd.cmd.user {
            x.response(bnd.ctx, Components::interaction_response("The button is not for you", true)).await?;
            return Ok(false);
        }
        Ok(true)
    }

    pub async fn confirmation(&self, bnd: &SlashBundle<'_>) -> Result<bool, MyErr> {
        let arow = CreateActionRow::Buttons(vec![
            Components::normal_button("Confirm", "yes", ButtonStyle::Primary, "👌"),
            Components::normal_button("Cancel", "nope", ButtonStyle::Danger, "👎"),
        ]);

        Components::response_adv(bnd, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(self.create_embed(bnd)).components(vec![arow]))).await?;
        let mut msg = bnd.cmd.get_msg(bnd.ctx).await?;
        let mut reply = msg.await_component_interactions(bnd.ctx).timeout(Duration::from_secs(60)).stream();

        while let Some(x) = reply.next().await {
            let bnd2 = ComponentBundle { cmd: &x, init: bnd.init, ctx: bnd.ctx, pedia: bnd.pedia, image: bnd.image };
            match x.data.custom_id.as_str() {
                "yes" => {
                    if Bought::verify_caller(bnd, &x).await? {
                        Components::response(&bnd2, "bought", true).await?;
                        msg.edit(&bnd.ctx.http, EditMessage::new().components(Vec::new())).await?;
                        return Ok(true);
                    }
                }
                "nope" => {
                    if Bought::verify_caller(bnd, &x).await? {
                        Components::response(&bnd2, "canceled", true).await?;
                        msg.delete(&bnd.ctx.http).await?;
                        return Ok(false);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    pub async fn confirmation_market(&self, bnd: &SlashBundle<'_>) -> Result<bool, MyErr> {
        let arow = CreateActionRow::Buttons(vec![
            Components::normal_button("Confirm", "yes", ButtonStyle::Primary, "👌"),
            Components::normal_button("Cancel", "nope", ButtonStyle::Danger, "👎"),
        ]);

        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🛒 **Please review your order receipt:**").embed(self.create_embed(bnd)).components(vec![arow])).await?;
        let mut msg = bnd.cmd.get_msg(bnd.ctx).await?;
        let mut reply = msg.await_component_interactions(bnd.ctx).timeout(Duration::from_secs(60)).stream();

        while let Some(x) = reply.next().await {
            let bnd2 = ComponentBundle { cmd: &x, init: bnd.init, ctx: bnd.ctx, pedia: bnd.pedia, image: bnd.image };
            match x.data.custom_id.as_str() {
                "yes" => {
                    if Bought::verify_caller(bnd, &x).await? {
                        Components::response(&bnd2, "bought", true).await?;
                        msg.edit(&bnd.ctx.http, EditMessage::new().components(Vec::new())).await?;
                        return Ok(true);
                    }
                }
                "nope" => {
                    if Bought::verify_caller(bnd, &x).await? {
                        Components::response(&bnd2, "canceled", true).await?;
                        msg.delete(&bnd.ctx.http).await?;
                        return Ok(false);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }
}

pub async fn auto(bnd: &SlashBundle<'_>, focus: &str) -> Result<(), MyErr> {
    let market = Market::new().await?;
    bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Autocomplete(CreateAutocompleteResponse::new().set_choices(market.auto(bnd.pedia, focus)))).await?;
    Ok(())
}

pub async fn slash(bnd: &SlashBundle<'_>, reg: &Reg<'_>) -> Result<(), MyErr> {
    let trade = Trading::new().await?;
    if !trade.market.enabled {
        return Err(MyErr::Custom("Trading market is currently disabled".to_string()));
    }
    bnd.cmd.defer(&bnd.ctx.http).await?;

    let market = Market::new().await?;
    let mut all_items_raw = Vec::new();

    for m in market.item_hash().values() {
        // All items stay listed, including out-of-stock ones; unlimited items show "∞".
        let stock_label = if m.unlimited { "∞".to_string() } else { m.treshold.to_string() };
        let item_name = m.item.text(bnd.pedia).unwrap_or_default();
        let display = format!("{} - {} (Stock: {} | Price: {})", m.item.key, item_name, stock_label, Market::currency(m.price as i64));
        all_items_raw.push((m.item.key.clone(), display, item_name));
    }

    all_items_raw.sort_by(|a, b| a.2.to_lowercase().cmp(&b.2.to_lowercase()));
    let all_items: Vec<(String, String)> = all_items_raw.into_iter().map(|(key, display, _)| (key, display)).collect();

    if all_items.is_empty() {
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ **The market is currently empty or completely sold out!**").components(vec![])).await?;
        return Ok(());
    }

    let mut current_page = 0;
    let mut selected_item_key: Option<String> = None;
    let mut selected_qty: Option<u16> = None;
    let mut search_query: Option<String> = None;
    let mut status_text = "🛒 **Server Market**\nPlease select an item and quantity to purchase.".to_string();

    let get_filtered_items = |query: &Option<String>| -> Vec<(String, String)> {
        match query {
            Some(q) => {
                let lower_q = q.to_lowercase();
                all_items.iter().filter(|(_, display)| display.to_lowercase().contains(&lower_q)).cloned().collect()
            }
            None => all_items.clone(),
        }
    };

    let mut filtered_items = get_filtered_items(&search_query);

    bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&status_text).components(render_market_components(&filtered_items, current_page, &selected_item_key, &selected_qty, search_query.is_some()))).await?;

    let msg_main = bnd.cmd.get_msg(bnd.ctx).await?;
    let mut stream = msg_main.await_component_interactions(bnd.ctx).timeout(Duration::from_secs(180)).stream();
    let mut proceed = false;

    while let Some(m) = stream.next().await {
        if m.user != bnd.cmd.user {
            m.response(bnd.ctx, Components::interaction_response("This menu is not for you", true)).await.unwrap_or_default();
            continue;
        }
        m.defer(&bnd.ctx.http).await.unwrap_or_default();
        let custom_id = m.data.custom_id.clone();

        match custom_id.as_str() {
            "m_search" => {
                bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🔍 **Type the keyword you want to search in this chat...** (e.g., ticket, gem)\n*You have 60 seconds to reply.*").components(vec![])).await?;
                if let Some(msg_reply) = bnd.cmd.channel_id.await_reply(&bnd.ctx).author_id(bnd.cmd.user.id).timeout(Duration::from_secs(60)).await {
                    let typed = msg_reply.content.trim().to_string();
                    let _ = msg_reply.delete(&bnd.ctx.http).await;
                    search_query = Some(typed);
                    current_page = 0;
                    selected_item_key = None;
                    status_text = format!("🛒 **Server Market** (🔍 Filtering by: `{}`)\nPlease select an item and quantity.", search_query.as_ref().unwrap());
                } else {
                    status_text = "⏳ **Search input timed out!** Returning to market...".to_string();
                }
            }
            "m_search_clear" => {
                search_query = None;
                current_page = 0;
                selected_item_key = None;
                status_text = "🛒 **Server Market**\nPlease select an item and quantity to purchase.".to_string();
            }
            "m_cancel" => {
                bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🛑 **Market Transaction Cancelled.**").components(vec![])).await?;
                return Ok(());
            }
            "m_clear" => {
                selected_item_key = None;
                selected_qty = None;
                status_text = if let Some(ref q) = search_query {
                    format!("🔄 **Selection Cleared.** (🔍 Filtering by: `{}`)\nPlease select an item and quantity again:", q)
                } else {
                    "🔄 **Selection Cleared.** Please select an item and quantity again:".to_string()
                };
            }
            "m_order" => {
                if selected_item_key.is_some() && selected_qty.is_some() {
                    proceed = true;
                    break;
                }
            }
            "m_prev" => { if current_page > 0 { current_page -= 1; } }
            "m_next" => { if (current_page + 1) * 23 < filtered_items.len() { current_page += 1; } }
            "m_item" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &m.data.kind {
                    if values.first().map(|v| v != "none").unwrap_or(false) {
                        selected_item_key = Some(values[0].clone());
                    }
                }
            }
            "m_qty" => {
                if let ComponentInteractionDataKind::StringSelect { values } = &m.data.kind {
                    if let Some(value) = values.first() {
                        if value == "custom" {
                            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🔢 **Type the exact amount you want to buy in this chat...** (e.g., 14 or 33)\n*You have 60 seconds to reply.*").components(vec![])).await?;
                            if let Some(msg_reply) = bnd.cmd.channel_id.await_reply(&bnd.ctx).author_id(bnd.cmd.user.id).timeout(Duration::from_secs(60)).await {
                                let typed = msg_reply.content.trim().to_string();
                                let _ = msg_reply.delete(&bnd.ctx.http).await;
                                match typed.parse::<u16>() {
                                    Ok(qty) if qty >= 1 => {
                                        selected_qty = Some(qty);
                                        status_text = format!("🔢 **Custom Amount Set:** {} pcs selected.", qty);
                                    }
                                    _ => {
                                        selected_qty = None;
                                        status_text = "❌ **Invalid Amount:** Please enter a whole number of at least 1. Select a quantity again.".to_string();
                                    }
                                }
                            } else {
                                status_text = "⏳ **Custom amount input timed out!** Returning to market...".to_string();
                            }
                        } else {
                            selected_qty = value.parse::<u16>().ok();
                        }
                    }
                }
            }
            _ => {}
        }

        filtered_items = get_filtered_items(&search_query);
        if selected_item_key.is_some() && selected_qty.is_some() {
            status_text = format!("✅ **Ready to Order:** Quantity set to {} pcs. Click **Order** to proceed to payment, or **Clear** to change.", selected_qty.unwrap());
        }

        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content(&status_text).components(render_market_components(&filtered_items, current_page, &selected_item_key, &selected_qty, search_query.is_some()))).await?;
    }

    if proceed {
        let item_key = selected_item_key.unwrap();
        let bought_qty = selected_qty.unwrap();
        let item = match market.item_hash().get(&item_key) {
            Some(i) => i.clone(),
            None => {
                bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("❌ **Error:** Selected item no longer exists in the market.").components(vec![])).await?;
                return Ok(());
            }
        };

        let mut handle = Handle { market, item, bought: bought_qty };
        let receipt = handle.check(reg, bnd.pedia).await?;

        if receipt.confirmation_market(bnd).await? {
            // The purchase is committed here: coins are debited and the item is
            // queued for delivery. Any failure past this point must never be
            // reported as a failed transaction, because the player has paid.
            handle.transaction(reg, bnd.pedia).await?;

            // Persisting stock is part of the sale; a failure here is worth logging
            // but must never turn a completed transaction into an error embed.
            if let Err(e) = handle.save_stock().await {
                warn!(error = %e, "Could not persist market stock after a successful purchase; sale stands");
            }

            bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("🎉 **Transaction Successful!** Thank you for your purchase. Please check your in-game inventory.").components(vec![])).await?;
        }
    } else {
        bnd.cmd.edit_response(&bnd.ctx.http, EditInteractionResponse::new().content("⏳ **Market Transaction Timed Out.**").components(vec![])).await?;
    }

    Ok(())
}