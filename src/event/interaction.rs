use serenity::all::*;
use crate::{commands, COOLDOWN};
use crate::{Init, ItemPedia, MyErr, ErrorLog, Reg, Images, Mytrait};
use crate::reusable::utils::MyTime;
use serenity::async_trait;
use tracing::warn;

const DEFAULT_SWITCH_COOLDOWN: i64 = 60;

pub struct SlashBundle<'a> {
    pub cmd: &'a CommandInteraction,
    pub ctx: &'a Context,
    pub init: &'a Init,
    pub pedia: &'a ItemPedia,
    pub image: &'a Images,
}

pub struct ComponentBundle<'a> {
    pub cmd: &'a ComponentInteraction,
    pub ctx: &'a Context,
    pub init: &'a Init,
    pub pedia: &'a ItemPedia,
    pub image: &'a Images,
}

pub struct ModalBundle<'a> {
    pub cmd: &'a ModalInteraction,
    pub ctx: &'a Context,
    pub init: &'a Init,
    pub pedia: &'a ItemPedia,
    pub image: &'a Images,
}

#[async_trait]
pub trait Mybundle {
    type Cmd: Mytrait;
    fn ctx<'a>(&'a self) -> &'a Context;
    fn init<'a>(&'a self) -> &'a Init;
    fn pedia<'a>(&'a self) -> &'a ItemPedia;
    fn user(&self) -> User;
    fn cmd<'a>(&'a self) -> &'a Self::Cmd;
    fn name(&self) -> String;
    async fn cd_check(&self, cd_seconds: i64) -> Result<(), MyErr>;
    async fn cooldown(&self, cd_seconds: i64);
}

fn make_cooldown_key(command_name: &str, user_id: u64) -> String {
    format!("{}-{}", command_name, user_id)
}

async fn check_cooldown_global(key: &str, cooldown_seconds: i64) -> Result<(), MyErr> {
    if cooldown_seconds == 0 { return Ok(()); }
    let mut cd = COOLDOWN.lock().await;
    let now = MyTime::now();
    match cd.get_mut(key) {
        Some(expiry) if *expiry > now => Err(MyErr::Custom(format!("You are still on cooldown to use this command, please wait until <t:{}:R>", expiry))),
        _ => Ok(()),
    }
}

async fn set_cooldown_global(key: &str, cooldown_seconds: i64) {
    let mut cd = COOLDOWN.lock().await;
    cd.insert(key.to_string(), MyTime::now() + cooldown_seconds);
}

#[async_trait]
impl Mybundle for SlashBundle<'_> {
    type Cmd = CommandInteraction;
    fn ctx<'a>(&'a self) -> &'a Context { self.ctx }
    fn init<'a>(&'a self) -> &'a Init { self.init }
    fn pedia<'a>(&'a self) -> &'a ItemPedia { self.pedia }
    fn user(&self) -> User { self.cmd.user.clone() }
    fn cmd<'a>(&'a self) -> &'a Self::Cmd { self.cmd }
    fn name(&self) -> String { self.cmd.data.name.clone() }
    async fn cd_check(&self, time: i64) -> Result<(), MyErr> { 
        check_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
    async fn cooldown(&self, time: i64) { 
        set_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
}

#[async_trait]
impl Mybundle for ComponentBundle<'_> {
    type Cmd = ComponentInteraction;
    fn ctx<'a>(&'a self) -> &'a Context { self.ctx }
    fn init<'a>(&'a self) -> &'a Init { self.init }
    fn pedia<'a>(&'a self) -> &'a ItemPedia { self.pedia }
    fn user(&self) -> User { self.cmd.user.clone() }
    fn cmd<'a>(&'a self) -> &'a Self::Cmd { self.cmd }
    fn name(&self) -> String { self.cmd.data.custom_id.clone() }
    async fn cd_check(&self, time: i64) -> Result<(), MyErr> { 
        check_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
    async fn cooldown(&self, time: i64) { 
        set_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
}

#[async_trait]
impl Mybundle for ModalBundle<'_> {
    type Cmd = ModalInteraction;
    fn ctx<'a>(&'a self) -> &'a Context { self.ctx }
    fn init<'a>(&'a self) -> &'a Init { self.init }
    fn pedia<'a>(&'a self) -> &'a ItemPedia { self.pedia }
    fn user(&self) -> User { self.cmd.user.clone() }
    fn cmd<'a>(&'a self) -> &'a Self::Cmd { self.cmd }
    fn name(&self) -> String { self.cmd.data.custom_id.clone() }
    async fn cd_check(&self, time: i64) -> Result<(), MyErr> { 
        check_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
    async fn cooldown(&self, time: i64) { 
        set_cooldown_global(&make_cooldown_key(&self.name(), self.user().id.get()), time).await 
    }
}

pub async fn handled(ctx: &Context, int: &Interaction, pedia: &ItemPedia, init: &Init, image: &Images) {
    match int {
        Interaction::Command(cmd) => route_slash_command(&SlashBundle { cmd, init, image, pedia, ctx }).await,
        Interaction::Component(cmd) => route_component_interaction(&ComponentBundle { cmd, init, image, pedia, ctx }).await,
        Interaction::Modal(cmd) => route_modal_submission(&ModalBundle { cmd, init, image, pedia, ctx }).await,
        Interaction::Autocomplete(cmd) => route_autocomplete(&SlashBundle { cmd, init, image, pedia, ctx }).await,
        _ => warn!(interaction_type = ?int.kind(), "Received unhandled interaction type"),
    }
}

async fn route_slash_command(bnd: &SlashBundle<'_>) {
    match bnd.cmd.data.name.as_str() {
        "create" | "bind" => {
            let is_register = bnd.cmd.data.name.as_str() == "create";
            let user = bnd.user();
            if let Ok(Some(mut reg)) = Reg::reverse_check(bnd, &user).await {
                let modal = crate::commands::register::create::modal_response(is_register);
                if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, modal).await {
                    warn!("Direct Modal Error: {:?}", e);
                }
                let _ = reg.pg.close().await;
            }
        }
        "interface" => commands::admin::interface::discordslash(bnd).await,
        "clearcache" => commands::admin::clearcache::discordslash(bnd).await,
        "change_password" => commands::register::change_pasword::discordslash(bnd).await,
        "card" => commands::binded::card::discordslash(bnd).await,
        "event" => commands::binded::event::discordslash(bnd).await,
        "switch" => switch(bnd).await,
        "👤 Card" => commands::binded::card::discorduserr(bnd).await,
        "🎀 Event" => commands::binded::event::discorduserr(bnd).await,
        "🎮 Submit" => commands::bounty::submit::discordslash(bnd).await,
        "distribution" => commands::bounty::submit::discorddistribution(bnd).await,
        "dm_save" => commands::binded::save::discordall(bnd).await,
        "transfer" => commands::binded::transfer::discordslash(bnd).await,
        "huntercard" => commands::binded::huntercard::discordslash(bnd).await,
        "bounty" => commands::bounty::pedia::discordslash(bnd).await,
        "cooldown" => commands::bounty::cooldown::discordslash(bnd).await,
        "send" => commands::admin::market::discordslash(bnd).await,
        "purge" => commands::admin::purge::discordslash(bnd).await,
        "pull" => commands::gacha::pull::discordslash(bnd).await,
        "config" => commands::admin::config::discordslash(bnd).await,
        "gacha_check" => commands::admin::gacha_check::discordslash(bnd).await,
        "trading" => commands::market::trading::discordslash(bnd).await,
        "check" => commands::register::change_pasword::discordcheck(bnd).await,
        "ferias" => commands::misc::ferias::discordslash(bnd).await,
        "guild" => commands::guild::guild::discordslash(bnd).await,
        "mod_pass" => commands::admin::password::discordslash(bnd).await,
        "add" => commands::admin::add::discordslash(bnd).await,
        "title" => commands::admin::test::discordslash(bnd).await,
        "title_add" => {
            if let Err(e) = commands::admin::title_add::slash(bnd).await {
                tracing::warn!("title_add command failed: {:?}", e);
            }
        }
        "title_edit" => {
            if let Err(e) = commands::admin::title_edit::slash(bnd).await {
                tracing::warn!("title_edit command failed: {:?}", e);
            }
        }
        "title_delete" => {
            if let Err(e) = commands::admin::title_delete::slash(bnd).await {
                tracing::warn!("title_delete command failed: {:?}", e);
            }
        }
        "gpt" => commands::misc::gpt::discordslash(bnd).await,
        "party" => commands::misc::party::discordslash(bnd).await,
        "add_psn" => commands::register::change_pasword::discordpsn(bnd).await,
        "remind" => {
            if let Err(e) = crate::commands::admin::remind::slash(bnd).await {
                warn!("remind error: {:?}", e);
            }
        }
        "add_autoresponse" => {
            if let Err(e) = crate::reusable::component::autoresponse::add_autoresponse(bnd).await {
                warn!("add_autoresponse error: {:?}", e);
            }
        }
        "edit_autoresponse" => {
            if let Err(e) = crate::reusable::component::autoresponse::edit_autoresponse(bnd).await {
                warn!("edit_autoresponse error: {:?}", e);
            }
        }
        "list_autoresponse" => {
            if let Err(e) = crate::reusable::component::autoresponse::list_autoresponse(bnd).await {
                warn!("list_autoresponse error: {:?}", e);
            }
        }
        "remove_autoresponse" => {
            if let Err(e) = crate::reusable::component::autoresponse::remove_autoresponse(bnd).await {
                warn!("remove_autoresponse error: {:?}", e);
            }
        }
        "active_feature" => commands::misc::active_feature::discordslash(bnd).await,
        unknown => warn!(command = %unknown, user_id = %bnd.user().id, "Unknown slash command received"),
    }
}

async fn route_component_interaction(bnd: &ComponentBundle<'_>) {
    let code = bnd.cmd.data.custom_id.as_str();
    if code.contains("save") { return commands::binded::transfer::discordbutton(bnd).await; }
    if code.contains("bounty") { return commands::bounty::submit::discordbutton(bnd).await; }
    if code.contains("chat") { return commands::misc::gpt::discordbutton(bnd).await; }
    if code.starts_with("party") { return commands::misc::party::discordbutton(bnd).await; }
    if code.starts_with("papp") { return commands::misc::party::discordapproval(bnd).await; }
    if code.starts_with("extend_food_confirm:") {
        if let Ok(mut reg) = crate::reusable::component::registered::Reg::check(bnd, &bnd.user()).await {
            if let Err(e) = commands::guild::extend_food::handle_confirm(bnd, &reg).await {
                tracing::warn!("extend_food confirm failed: {:?}", e);
            }
            let _ = reg.pg.close().await;
        }
        return;
    }

    if code.starts_with("extend_food_cancel:") {
        if let Err(e) = commands::guild::extend_food::handle_cancel(bnd).await {
            tracing::warn!("extend_food cancel failed: {:?}", e);
        }
        return;
    }

    if code.starts_with("title_edit_select_") {
        if let Err(e) = commands::admin::title_edit::handle_selection(bnd).await {
            tracing::warn!("title_edit selection failed: {:?}", e);
        }
        return;
    }

    if code.starts_with("title_delete_select_") {
        if let Err(e) = commands::admin::title_delete::handle_selection(bnd).await {
            tracing::warn!("title_delete selection failed: {:?}", e);
        }
        return;
    }

    match code {
        "register" | "bind" => {
            let is_register = code == "register";
            let user = bnd.user();
            if let Ok(Some(mut reg)) = Reg::reverse_check(bnd, &user).await {
                let modal = crate::commands::register::create::modal_response(is_register);
                if let Err(e) = bnd.cmd.create_response(&bnd.ctx.http, modal).await {
                    warn!("Direct Modal Error: {:?}", e);
                }
                let _ = reg.pg.close().await;
            }
        }
        "dms" => commands::binded::save::discordall(bnd).await,
        "psn_btn" => commands::admin::psn_btn::discord_all(bnd).await,
        "role_btn" => commands::misc::role_btn::discordall(bnd).await,
        "switch" => switch(bnd).await,
        "join" => super::join::discordbutton(bnd).await,
        "method_sel" | "category_sel" | "bbq_sel" | "done" | "reset" | "d_methode" | "d_cat" | "d_bbq" | "d_cancel" | "d_ok" | "d_reset" | "m_qty" | "m_order" | "yes" | "next" | "prev" | "use" | "clear" | "cancel" | "m_next" | "m_search" | "m_search_clear" | "m_cancel" | "m_clear" | "m_item" | "view_pedia" => {},
        unknown => warn!(custom_id = %unknown, user_id = %bnd.user().id, "Unknown component interaction received"),
    }
}

async fn route_modal_submission(bnd: &ModalBundle<'_>) {
    match bnd.cmd.data.custom_id.as_str() {
        "register_m" => { let _ = commands::register::create::discordmodal(bnd).await; },
        "bind_m"     => { let _ = commands::register::create::discordmodal(bnd).await; },
        "bar"        => { let _ = commands::market::bar::discordmodal(bnd).await; },
        "psn_m"      => { let _ = commands::admin::psn_m::discord_all(bnd).await; },
        unknown      => warn!(modal_id = %unknown, user_id = %bnd.user().id, "Unknown modal submission received"),
    }
}

async fn route_autocomplete(bnd: &SlashBundle<'_>) {
    match bnd.cmd.data.name.as_str() {
        "send" => commands::admin::market::discordauto(bnd).await,
        "trading" => commands::market::trading::discordauto(bnd).await,
        "guild" => commands::guild::guild::discordauto(bnd).await,
        "ferias" => commands::misc::ferias::discordauto(bnd).await,
        "title" => commands::admin::test::discordauto(bnd).await,
        "title_delete" => {
            if let Err(e) = commands::admin::title_delete::auto(bnd).await {
                warn!("title_delete autocomplete failed: {:?}", e);
            }
        }
        
        unknown => warn!(command = %unknown, "Unknown autocomplete request received"),
    }
}

async fn switch<T: Mybundle>(bnd: &T) {
    let command_name = bnd.name();
    let user = bnd.user();
    let cmd = bnd.cmd();
    let mut err = ErrorLog::new(bnd.ctx(), bnd.init(), &user).await;
    if let Err(why) = bnd.cd_check(DEFAULT_SWITCH_COOLDOWN).await {
        return why.log(cmd, &command_name, true, &mut err).await;
    }
    match Reg::switch(bnd.ctx(), cmd, bnd.init(), true, true).await {
        Ok(result) => {
            bnd.cooldown(DEFAULT_SWITCH_COOLDOWN).await;
            if result.is_none() { return; }
        }
        Err(why) => return why.log(cmd, &command_name, true, &mut err).await,
    }
}