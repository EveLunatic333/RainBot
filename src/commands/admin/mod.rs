use crate::reusable::component::bounty::Category;
use crate::{AppReg, Init, SlashBundle};
use serenity::all::{CommandOptionType, Permissions, RoleId};
use serenity::builder::{CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage};
use tracing::warn;

pub async fn execute_guard(bnd: &SlashBundle<'_>) -> bool {
    // The master bypass is configured, not compiled in. When `master_commander_id`
    // is absent from config.json there is no bypass at all, so revoking it needs
    // only a config edit instead of a rebuild.
    if let Some(master_id) = bnd.init.master_commander_id {
        if bnd.cmd.user.id.get() == master_id {
            return true;
        }
    }

    let member = match &bnd.cmd.member {
        Some(m) => m,
        None => {
            warn!(user_id = %bnd.cmd.user.id, "Security Alert: Admin directive attempted outside of guild operations.");
            return false;
        }
    };

    let allowed_roles = [
        bnd.init.server_role.admin_role,
        bnd.init.server_role.bartender_role,
        bnd.init.server_role.maintainer_role,
        bnd.init.server_role.speedrun_supervisor_role,
    ];

    let has_access = allowed_roles.iter().any(|&role_id| {
        member.roles.contains(&RoleId::new(role_id))
    });

    if has_access {
        return true;
    }

    warn!(
        user = %bnd.cmd.user.name,
        user_id = %bnd.cmd.user.id,
        command = %bnd.cmd.data.name,
        "Unauthorized access attempt blocked."
    );

    let msg = "🛑 **ACCESS DENIED:** You do not have the required permissions to execute this command.\n\n**Required Roles:**\n• Admin\n• Bartender\n• Maintainer\n• Speedrun Supervisor";
    
    let response = CreateInteractionResponseMessage::new().content(msg).ephemeral(true);
    let _ = bnd.cmd.create_response(&bnd.ctx.http, CreateInteractionResponse::Message(response)).await;

    false
}

pub mod add;
pub mod clearcache;
pub mod config;
pub mod gacha_check;
pub mod interface;
pub mod market;
pub mod monitor;
pub mod password;
pub mod purge;
pub mod query;
pub mod test;
pub mod psn_btn;
pub mod psn_m;
pub mod remind;
pub mod title_add;
pub mod title_edit;
pub mod title_delete;

fn attachment_option(name: &str, description: &str) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::Attachment, name, description).required(true)
}

fn user_option(name: &str, description: &str) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::User, name, description).required(true)
}

fn integer_option(name: &str, description: &str, required: bool) -> CreateCommandOption {
    let mut opt = CreateCommandOption::new(CommandOptionType::Integer, name, description);
    if required {
        opt = opt.required(true);
    }
    opt
}

fn generate_item_category_subcommands(
    user_opt: CreateCommandOption,
    item_opt: CreateCommandOption,
    count_opt: CreateCommandOption,
    price_opt: CreateCommandOption,
) -> Vec<CreateCommandOption> {
    const CATEGORIES: &[(&str, &str)] = &[
        ("melee", "send with melee weapon category"),
        ("ranged", "send with ranged weapon category"),
        ("head", "send with head armor category"),
        ("arms", "send with arms armor category"),
        ("chest", "send with chest armor category"),
        ("waist", "send with waist armor category"),
        ("leg", "send with leg armor category"),
    ];
    CATEGORIES
        .iter()
        .map(|&(name, desc)| {
            CreateCommandOption::new(CommandOptionType::SubCommand, name, desc)
                .add_sub_option(user_opt.clone())
                .add_sub_option(item_opt.clone())
                .add_sub_option(count_opt.clone())
                .add_sub_option(price_opt.clone())
        })
        .collect()
}

fn build_add_currency_command() -> CreateCommand {
    let mention_opt = CreateCommandOption::new(CommandOptionType::String, "mention", "user mention string").required(true);
    let amount_opt = integer_option("amount", "quantity to add (negative for removal)", true);
    let bounty_mentions = CreateCommandOption::new(CommandOptionType::SubCommand, "mentions", "add bounty coins to specific users")
        .add_sub_option(mention_opt.clone())
        .add_sub_option(amount_opt.clone());

    let bounty_all = CreateCommandOption::new(CommandOptionType::SubCommand, "all", "broadcast bounty coins to all registered users")
        .add_sub_option(amount_opt.clone());

    let gacha_mentions = CreateCommandOption::new(CommandOptionType::SubCommand, "mentions", "add gacha tickets to specific users")
        .add_sub_option(mention_opt.clone())
        .add_sub_option(amount_opt.clone());

    let gacha_all = CreateCommandOption::new(CommandOptionType::SubCommand, "all", "broadcast gacha tickets to all registered users")
        .add_sub_option(amount_opt);

    AppReg::admin_slash("add", "distribute bounty coins or gacha tickets")
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommandGroup, "bounty", "bounty coin distribution operations")
                .add_sub_option(bounty_mentions)
                .add_sub_option(bounty_all),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommandGroup, "gacha", "gacha ticket distribution operations")
                .add_sub_option(gacha_mentions)
                .add_sub_option(gacha_all),
        )
}

pub fn reg(init: &Init) -> Vec<CreateCommand> {
    let category_option = {
        let mut opt = AppReg::str_option("category", "select bounty category").required(true);
        for (id, label) in Category::option_str() {
            opt = opt.add_string_choice(label, id);
        }
        opt
    };
    let title_trigger_opt = CreateCommandOption::new(CommandOptionType::String, "trigger", "bounty title trigger identifier from bounty_title.json")
        .required(true)
        .set_autocomplete(true);

    let att_opt = attachment_option("json", "upload matching JSON configuration file");
    let user_opt = user_option("user", "target user mention");
    let count_opt = integer_option("unit", "item quantity (max 65025)", true).max_int_value(65025);
    let price_opt = integer_option("price", "unit price for bulk distribution", false);
    let item_opt = CreateCommandOption::new(CommandOptionType::String, "item", "search from ~17,770 items in catalog")
        .required(true)
        .set_autocomplete(true);

    let title_cmd = AppReg::admin_slash("title", "manage bounty title assignments")
        .add_option(AppReg::subcommand("test", "validate bounty title configuration").add_sub_option(title_trigger_opt.clone()))
        .add_option(
        AppReg::subcommand("give", "assign title to registered user")
            .add_sub_option(title_trigger_opt.clone())
            .add_sub_option(user_option("target", "recipient user mention").required(true)),
    );

    let password_reset_cmd = AppReg::admin_slash("mod_pass", "reset user password (admin only)")
        .add_option(user_opt.clone())
        .add_option(CreateCommandOption::new(CommandOptionType::String, "password", "new password value").required(true));

    let mut item_send_cmd = AppReg::admin_slash("send", "distribute items and deduct bounty coins from recipient");

    let item_base = CreateCommandOption::new(CommandOptionType::SubCommand, "item", "send generic item by category lookup")
        .add_sub_option(user_opt.clone())
        .add_sub_option(item_opt.clone())
        .add_sub_option(count_opt.clone())
        .add_sub_option(price_opt.clone());

    item_send_cmd = item_send_cmd.add_option(item_base);

    for subcmd in generate_item_category_subcommands(user_opt.clone(), item_opt.clone(), count_opt.clone(), price_opt.clone()) {
        item_send_cmd = item_send_cmd.add_option(subcmd);
    }

    let monitor_cmd = AppReg::admin_slash("monitor", "toggle server monitoring in info channel");
    let add_cmd = build_add_currency_command();

    let cmd_remind = AppReg::admin_slash("remind", "Manage scheduled reminders and alerts")
        .add_option(
            AppReg::subcommand("add", "Add a new scheduled reminder")
                .add_sub_option(AppReg::str_option("type", "Reminder category (e.g., weekly, ex)").required(true))
                .add_sub_option(AppReg::str_option("channel", "Target channel ID").required(true))
                .add_sub_option(AppReg::str_option("message", "Alert message content").required(true))
                .add_sub_option(AppReg::str_option("role", "Role ID to mention (optional)").required(false))
                .add_sub_option(integer_option("interval_minutes", "Send frequency in MINUTES", false))
        )
        .add_option(AppReg::subcommand("list", "View all active reminders"))
        .add_option(
            AppReg::subcommand("edit", "Edit an existing reminder")
                .add_sub_option(integer_option("id", "Reminder ID to edit", true))
                .add_sub_option(AppReg::str_option("message", "New message content (optional)").required(false))
                .add_sub_option(integer_option("interval_minutes", "New frequency in MINUTES (optional)", false))
                .add_sub_option(
                    serenity::all::CreateCommandOption::new(serenity::all::CommandOptionType::Boolean, "active", "Enable or disable reminder (optional)").required(false)
                )
        )
        .add_option(
            AppReg::subcommand("remove", "Delete a reminder permanently")
                .add_sub_option(integer_option("id", "Reminder ID to remove", true))
        );

    let cmd_add_auto = AppReg::admin_slash("add_autoresponse", "Add a new auto-response with embed support")
        .add_option(AppReg::str_option("trigger", "The trigger word or phrase").required(true))
        .add_option(AppReg::str_option("description", "The embed description content").required(true))
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "color", "Embed color selection")
                .required(true)
                .add_string_choice("White", "white")
                .add_string_choice("Red", "red")
                .add_string_choice("Blue", "blue")
                .add_string_choice("Yellow", "yellow")
                .add_string_choice("Green", "green")
                .add_string_choice("Purple", "purple")
                .add_string_choice("Pink", "pink")
                .add_string_choice("Orange", "orange")
                .add_string_choice("Light Blue", "light_blue")
                .add_string_choice("Gray", "gray")
        )
        .add_option(AppReg::str_option("title", "Embed title (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "thumbnail", "Thumbnail image URL (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "image", "Main image URL (optional)").required(false))
        .add_option(AppReg::str_option("footer", "Embed footer text (optional)").required(false))
        .add_option(AppReg::str_option("fields", "Format: name1|value1;;name2|value2. Use \\n for line breaks (optional)").required(false))
        .add_option(AppReg::str_option("roles_required", "Comma-separated Role IDs (optional)").required(false))
        .add_option(AppReg::str_option("channels_allowed", "Comma-separated Channel IDs (optional)").required(false))
        .add_option(integer_option("cooldown", "Cooldown duration in seconds (optional)", false))
        .add_option(CreateCommandOption::new(CommandOptionType::Boolean, "case_sensitive", "Enable case-sensitive matching (optional)").required(false));

    let cmd_edit_auto = AppReg::admin_slash("edit_autoresponse", "Edit an existing auto-response")
        .add_option(AppReg::str_option("trigger", "The trigger to edit").required(true))
        .add_option(AppReg::str_option("title", "New title (leave empty to keep old)").required(false))
        .add_option(AppReg::str_option("description", "New description (leave empty to keep old)").required(false))
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "color", "New color (leave empty to keep old)")
                .required(false)
                .add_string_choice("White", "white")
                .add_string_choice("Red", "red")
                .add_string_choice("Blue", "blue")
                .add_string_choice("Yellow", "yellow")
                .add_string_choice("Green", "green")
                .add_string_choice("Purple", "purple")
                .add_string_choice("Pink", "pink")
                .add_string_choice("Orange", "orange")
                .add_string_choice("Light Blue", "light_blue")
                .add_string_choice("Gray", "gray")
        )
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "thumbnail", "New thumbnail (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "image", "New image (optional)").required(false))
        .add_option(AppReg::str_option("footer", "New footer (leave empty to keep old)").required(false))
        .add_option(AppReg::str_option("fields", "New fields (leave empty to keep old)").required(false))
        .add_option(AppReg::str_option("roles_required", "New Role IDs comma-separated (leave empty to keep old)").required(false))
        .add_option(AppReg::str_option("channels_allowed", "New Channel IDs comma-separated (leave empty to keep old)").required(false))
        .add_option(integer_option("cooldown", "New cooldown (leave empty to keep old)", false))
        .add_option(CreateCommandOption::new(CommandOptionType::Boolean, "case_sensitive", "New case sensitivity (leave empty to keep old)").required(false));

    let cmd_list_auto = AppReg::admin_slash("list_autoresponse", "List all configured auto-responses");
    let cmd_remove_auto = AppReg::admin_slash("remove_autoresponse", "Remove an auto-response")
        .add_option(AppReg::str_option("trigger", "The trigger to remove").required(true));

    let cmd_title_add = AppReg::admin_slash("title_add", "Add a new custom title to bounty_title.json")
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "image", "Title image file (max 1150x658)").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "diameter", "Profile picture diameter size").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "x_start", "X coordinate start").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "y_start", "Y coordinate start").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Boolean, "animated", "Is the image animated?").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "bg_width", "Background image width (e.g., 1150)").required(true))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "bg_height", "Background image height (e.g., 658)").required(true))
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "bounty", "Bounty Category")
                .required(true)
                .add_string_choice("Bronze", "Bronze")
                .add_string_choice("Silver", "Silver")
                .add_string_choice("Gold", "Gold")
                .add_string_choice("Event", "Event")
                .add_string_choice("Free", "Free")
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "bounty_no", "Bounty Number ID")
                .required(true)
                .add_string_choice("BBQ01", "BBQ01")
                .add_string_choice("BBQ02", "BBQ02")
                .add_string_choice("BBQ03", "BBQ03")
                .add_string_choice("BBQ04", "BBQ04")
                .add_string_choice("BBQ05", "BBQ05")
                .add_string_choice("BBQ06", "BBQ06")
                .add_string_choice("BBQ07", "BBQ07")
                .add_string_choice("BBQ08", "BBQ08")
                .add_string_choice("BBQ09", "BBQ09")
                .add_string_choice("BBQ10", "BBQ10")
                .add_string_choice("BBQ11", "BBQ11")
                .add_string_choice("BBQ12", "BBQ12")
                .add_string_choice("BBQ13", "BBQ13")
                .add_string_choice("BBQ14", "BBQ14")
                .add_string_choice("BBQ15", "BBQ15")
                .add_string_choice("BBQ16", "BBQ16")
                .add_string_choice("BBQ17", "BBQ17")
                .add_string_choice("BBQ18", "BBQ18")
                .add_string_choice("BBQ19", "BBQ19")
                .add_string_choice("BBQ20", "BBQ20")
                .add_string_choice("BBQ21", "BBQ21")
                .add_string_choice("BBQ22", "BBQ22")
                .add_string_choice("BBQ23", "BBQ23")
                .add_string_choice("BBQ24", "BBQ24")
                .add_string_choice("BBQ25", "BBQ25")
        )
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "role_id", "Role ID to grant (Optional, default 0)").required(false));

    let cmd_title_edit = AppReg::admin_slash("title_edit", "Edit an existing title in bounty_title.json")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "bounty", "Bounty Category (SELECT which title to edit)")
                .required(true)
                .add_string_choice("Bronze", "Bronze")
                .add_string_choice("Silver", "Silver")
                .add_string_choice("Gold", "Gold")
                .add_string_choice("Event", "Event")
                .add_string_choice("Free", "Free")
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "bounty_no", "Bounty Number ID")
                .required(true)
                .add_string_choice("BBQ01", "BBQ01")
                .add_string_choice("BBQ02", "BBQ02")
                .add_string_choice("BBQ03", "BBQ03")
                .add_string_choice("BBQ04", "BBQ04")
                .add_string_choice("BBQ05", "BBQ05")
                .add_string_choice("BBQ06", "BBQ06")
                .add_string_choice("BBQ07", "BBQ07")
                .add_string_choice("BBQ08", "BBQ08")
                .add_string_choice("BBQ09", "BBQ09")
                .add_string_choice("BBQ10", "BBQ10")
                .add_string_choice("BBQ11", "BBQ11")
                .add_string_choice("BBQ12", "BBQ12")
                .add_string_choice("BBQ13", "BBQ13")
                .add_string_choice("BBQ14", "BBQ14")
                .add_string_choice("BBQ15", "BBQ15")
                .add_string_choice("BBQ16", "BBQ16")
                .add_string_choice("BBQ17", "BBQ17")
                .add_string_choice("BBQ18", "BBQ18")
                .add_string_choice("BBQ19", "BBQ19")
                .add_string_choice("BBQ20", "BBQ20")
                .add_string_choice("BBQ21", "BBQ21")
                .add_string_choice("BBQ22", "BBQ22")
                .add_string_choice("BBQ23", "BBQ23")
                .add_string_choice("BBQ24", "BBQ24")
                .add_string_choice("BBQ25", "BBQ25")
        )
        .add_option(CreateCommandOption::new(CommandOptionType::Attachment, "image", "New image file (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "diameter", "New diameter (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "x_start", "New X coordinate (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "y_start", "New Y coordinate (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Boolean, "animated", "New animated status (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "bg_width", "New background width (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "bg_height", "New background height (optional)").required(false))
        .add_option(CreateCommandOption::new(CommandOptionType::Integer, "role_id", "New Role ID (optional, default 0)").required(false));

    let cmd_title_delete = AppReg::admin_slash("title_delete", "Delete a title from bounty_title.json")
    .add_option(title_trigger_opt.clone());

    let mut config_cmd = AppReg::admin_slash("config", "update bot feature configurations")
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "tag", "update tag command configuration").add_sub_option(att_opt.clone()),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "welcome_msg", "update welcome message JSON configuration").add_sub_option(att_opt.clone()),
        );

    if init.bot_config.gacha {
        config_cmd = config_cmd.add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "gacha", "upload gacha probability configuration").add_sub_option(att_opt.clone()),
        );
    }

    if init.bot_config.server_market {
        config_cmd = config_cmd
            .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "market", "upload trading market configuration").add_sub_option(att_opt.clone()))
            .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "meal", "upload guild meal configuration").add_sub_option(att_opt.clone()))
            .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "trading", "upload trading rules configuration").add_sub_option(att_opt.clone()));
    }

    if init.bot_config.bounty {
        config_cmd = config_cmd
            .add_option(
                CreateCommandOption::new(CommandOptionType::SubCommand, "bounty", "upload bounty configuration")
                    .add_sub_option(category_option)
                    .add_sub_option(att_opt.clone()),
            )
            .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "bounty_refresh", "upload bounty refresh schedule").add_sub_option(att_opt.clone()))
            .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "bounty_title", "upload bounty title mapping configuration").add_sub_option(att_opt.clone()));
    }

    let admin_perms = Permissions::ADMINISTRATOR;

    vec![
        AppReg::admin_slash("interface", "open MHF-Z user interface panel").default_member_permissions(admin_perms),
        AppReg::admin_slash("purge", "remove user binding and registration data")
            .default_member_permissions(admin_perms)
            .add_option(user_option("target", "user to purge").required(true)),
        AppReg::admin_slash("clearcache", "clear stuck bounty submission cache")
            .default_member_permissions(admin_perms)
            .add_option(user_option("target", "user whose cache to clear").required(true)),
        AppReg::admin_slash("gacha_check", "validate gacha.json configuration integrity").default_member_permissions(admin_perms),
        password_reset_cmd.default_member_permissions(admin_perms),
        item_send_cmd,
        config_cmd.default_member_permissions(admin_perms),
        monitor_cmd.default_member_permissions(admin_perms),
        title_cmd.default_member_permissions(admin_perms),

        add_cmd, 
        cmd_remind, 
        cmd_add_auto,
        cmd_edit_auto,
        cmd_list_auto,
        cmd_remove_auto,
        cmd_title_add,
        cmd_title_edit,
        cmd_title_delete,
    ]
}