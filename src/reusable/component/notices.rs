//! Notice builders for every refusal outside the bounty flow.
//!
//! Each function here is pure: it takes the facts a command module already has
//! and returns a [`Notice`]. The command modules keep deciding *whether* to
//! refuse; this module only decides *how it reads*.
//!
//! Keeping the wording here rather than inline in forty `content()` calls means
//! it can be unit-tested without a Discord client, and a change to the house
//! style reaches every refusal at once.

use super::notice::{Notice, ProgressRow};

/// Formats a coin amount the way the market does, without depending on it.
fn coins(amount: i64) -> String {
    let negative = amount < 0;
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if negative {
        grouped.insert(0, '-');
    }
    format!("{grouped} BC")
}

// ---------------------------------------------------------------------------
// Party
// ---------------------------------------------------------------------------

/// Someone who is not the leader pressed a leader-only button.
pub fn party_leader_only(action: &str, leader: Option<u64>) -> Notice {
    let mut notice = Notice::refusal(
        "Leader Only",
        format!("Only the party leader can {action} this party."),
    )
    .footer("Party button");

    if let Some(id) = leader {
        notice = notice
            .detail(format!("The leader of this party is <@{id}>."))
            .action("Ask the leader to do it, or start your own party with `/party`.");
    } else {
        notice = notice.action("Start your own party with `/party`.");
    }
    notice
}

/// The party is already at its configured size.
pub fn party_full(size: usize, members: usize, leader: Option<u64>) -> Notice {
    let mut notice = Notice::refusal(
        "Party Full",
        "There is no free slot in this party.",
    )
    .footer("Party button")
    .progress(ProgressRow::new("Members", members as i64, size as i64))
    .action("Wait for someone to leave, or join another party that still has room.");

    if let Some(id) = leader {
        notice = notice.detail(format!("The leader of this party is <@{id}>."));
    }
    notice
}

/// A click arrived while another click on the same party is still being handled.
pub fn party_busy() -> Notice {
    Notice::info(
        "Still Processing",
        "Another click on this party is still being handled.",
    )
    .footer("Party button")
    .action("Wait a moment, then press the button again.")
}

/// The leader tried to leave their own party.
pub fn party_leader_cannot_leave(leader: u64) -> Notice {
    Notice::refusal(
        "Leader Cannot Leave",
        "Leaving would strand the party you are leading.",
    )
    .footer("Party button")
    .detail(format!("<@{leader}> is the leader of this party."))
    .action("Use **Close Party** if the party is finished, or **Cancel Recruitment** to withdraw the post.")
}

/// The clicker is not on the roster.
pub fn party_not_a_member() -> Notice {
    Notice::info("Not a Member", "You are not on this party's roster.")
        .footer("Party button")
        .action("Press **Join** first if you want to take part.")
}

/// The clicker is already on the roster.
pub fn party_already_member(size: usize, members: usize) -> Notice {
    Notice::info("Already Joined", "You are already on this party's roster.")
        .footer("Party button")
        .progress(ProgressRow::new("Members", members as i64, size as i64))
        .action("Nothing to do — you are already counted in this party.")
}

/// The recruitment post could not be sent.
pub fn party_post_failed() -> Notice {
    Notice::failure(
        "Party Not Posted",
        "The recruitment post could not be sent to the target channel.",
    )
    .footer("Party command")
    .action("Check that the bot can send messages in that channel, then run `/party` again.")
}

/// The typed edit had the wrong shape.
pub fn party_edit_bad_format() -> Notice {
    Notice::refusal(
        "Party Not Updated",
        "The text you typed does not match the required format.",
    )
    .footer("Party edit")
    .detail("Expected `quest | location`.")
    .action("Type it again as `quest | location`, or press Reset to cancel the edit.")
}

/// Nobody answered the edit prompt in time.
pub fn party_edit_timeout(seconds: u64) -> Notice {
    Notice::info(
        "Party Not Updated",
        "The edit prompt expired before a reply arrived.",
    )
    .footer("Party edit")
    .detail(format!("The prompt waits {seconds} seconds."))
    .action("Press **Edit** and type the new details right away.")
}

/// A join request was already approved or rejected.
pub fn party_request_handled() -> Notice {
    Notice::info("Request Already Handled", "This join request was already answered.")
        .footer("Party panel")
        .action("Nothing to do — the leader already decided on this request.")
}

/// The party post itself is gone.
pub fn party_post_gone() -> Notice {
    Notice::info("Party No Longer Valid", "This party post can no longer be read.")
        .footer("Party panel")
        .action("Run `/party` to start a new recruitment.")
}

/// The player is already on the roster, so the request is redundant.
pub fn party_requester_is_member() -> Notice {
    Notice::info(
        "Already a Member",
        "This player is already on the roster, so the request was cleared.",
    )
    .footer("Party panel")
    .action("Nothing to do — they are already counted in this party.")
}

/// A second join attempt while one is already pending.
pub fn party_request_pending() -> Notice {
    Notice::info(
        "Request Already Sent",
        "You already asked to join this party and the leader has not answered yet.",
    )
    .footer("Party button")
    .action("Nothing to do — wait for the leader to accept or decline.")
}

/// A panel button refers to a party that has since been removed.
pub fn party_no_longer_exists() -> Notice {
    Notice::info("Party Gone", "This party no longer exists.")
        .footer("Party panel")
        .action("Run `/party` to start a new recruitment.")
}

/// The leader declined a request because the party filled up first.
pub fn party_declined_full(size: usize) -> Notice {
    Notice::info(
        "Request Declined",
        "The party filled up before this request was reviewed, so it was declined.",
    )
    .footer("Party panel")
    .progress(ProgressRow::new("Members", size as i64, size as i64))
    .action("Join another party that still has a free slot.")
}

// ---------------------------------------------------------------------------
// Market
// ---------------------------------------------------------------------------

/// A limited item had fewer units left than the player asked for.
pub fn market_out_of_stock(item: &str, wanted: i64, left: i64) -> Notice {
    let mut notice = Notice::refusal(
        "Out of Stock",
        format!("**{item}** does not have enough units left for this order."),
    )
    .command("/market");

    if wanted > 0 {
        notice = notice.progress(ProgressRow::new("Requested", wanted, wanted.max(left)));
    }
    if left <= 0 {
        notice = notice
            .detail(format!("**{item}** has 0 units left."))
            .action("Wait for a restock, or pick another item.");
    } else {
        notice = notice
            .detail(format!("**{item}** has {left} units left."))
            .action(format!("Reduce the quantity to {left} or fewer and try again."));
    }
    notice
}

/// The player cannot afford the order.
pub fn market_insufficient_balance(balance: i64, price: i64, label: &str) -> Notice {
    let short = price.saturating_sub(balance);
    Notice::refusal(
        "Not Enough Bounty Coin",
        "Your balance does not cover this order.",
    )
    .command("/market")
    .progress(ProgressRow::new("Your balance", balance, price.max(1)))
    .detail(format!("Order total: **{}**.", coins(price)))
    .detail(format!("You are short **{}**.", coins(short)))
    .action(format!("Earn more Bounty Coin, or buy fewer {label}."))
}

/// Same numbers, but formatted with the market's own thousands separators.
///
/// Used by the `/market` flow, which already renders balances as `Bc 1,000.00`
/// so the notice must not switch notation mid-screen.
pub fn market_insufficient_balance_formatted(balance: i64, price: i64, format: impl Fn(i64) -> String) -> Notice {
    let short = price.saturating_sub(balance);
    Notice::refusal(
        "Not Enough Bounty Coin",
        "Your balance does not cover this order.",
    )
    .command("/market")
    .progress(ProgressRow::new("Your balance", balance, price.max(1)))
    .detail(format!("Order total: **{}**.", format(price)))
    .detail(format!("You are short **{}**.", format(short)))
    .action("Earn more Bounty Coin, or order fewer items.")
}

/// The order total exceeds what the market can charge in one transaction.
pub fn market_total_too_large(total: i64, limit: i64) -> Notice {
    Notice::refusal(
        "Order Too Large",
        "This order total is above the amount the market can charge at once.",
    )
    .command("/market")
    .progress(ProgressRow::new("Order total", total, limit))
    .detail(format!("The limit is **{}**.", coins(limit)))
    .action("Split the purchase into several smaller orders.")
}

/// The order carries more units than the courier can deliver.
pub fn market_too_many_items(items: i64, limit: i64) -> Notice {
    Notice::refusal(
        "Too Many Items",
        "This order carries more units than can be delivered in one transaction.",
    )
    .command("/market")
    .progress(ProgressRow::new("Units", items, limit))
    .detail(format!("The courier carries at most **{limit}** units per order."))
    .action("Reduce the quantity and buy again.")
}

/// The market is switched off in configuration.
pub fn trading_disabled() -> Notice {
    Notice::refusal("Market Disabled", "The market is turned off right now.")
        .command("/trading")
        .action("Ask an administrator to enable the market in `trading.json`.")
}

/// One trading category is closed.
pub fn trading_section_closed(what: &str) -> Notice {
    Notice::refusal(format!("{what} Closed"), format!("{what} trading is turned off right now."))
        .command("/trading")
        .action("Ask an administrator to reopen it, or choose a different unit.")
}

/// A non-positive quantity was requested.
pub fn trading_bad_quantity() -> Notice {
    Notice::refusal("Quantity Rejected", "The quantity must be a positive number.")
        .command("/trading")
        .action("Type a quantity of 1 or more.")
}

/// The purchase needs a guild, but the player has none.
pub fn trading_no_guild(refunded: bool) -> Notice {
    let mut notice = Notice::refusal(
        "No Guild",
        "This purchase is delivered to a guild, but you do not belong to one.",
    )
    .command("/trading");
    if refunded {
        notice = notice.detail("Your Bounty Coin was refunded.");
    }
    notice.action("Join or create a guild with `/guild`, then try again.")
}

/// The selected item vanished between rendering the menu and confirming.
pub fn market_item_gone() -> Notice {
    Notice::refusal(
        "Item Unavailable",
        "The selected item is no longer in the market.",
    )
    .command("/market")
    .action("Refresh the market with `/market` and pick another item.")
}

/// The market has nothing to sell.
pub fn market_empty() -> Notice {
    Notice::info("Market Empty", "There is nothing in stock right now.")
        .command("/market")
        .action("Check back after the next restock.")
}

/// The restaurant is switched off.
pub fn meal_disabled() -> Notice {
    Notice::refusal("Restaurant Closed", "The restaurant is turned off right now.")
        .command("/trading restourant")
        .action("Ask an administrator to reopen it.")
}

/// A food order uses a non-positive quantity.
pub fn meal_bad_quantity() -> Notice {
    Notice::refusal("Quantity Rejected", "The quantity must be a positive number.")
        .command("/trading restourant")
        .action("Type a quantity of 1 or more.")
}

/// A food order exceeds the maximum hours the restaurant will book.
pub fn meal_too_long(requested_hours: i64, max_hours: i64) -> Notice {
    Notice::refusal(
        "Order Too Long",
        "That many hours of food cannot be booked in one order.",
    )
    .command("/trading restourant")
    .progress(ProgressRow::new("Hours", requested_hours, max_hours.max(1)))
    .detail(format!("The maximum per order is **{max_hours}** hours."))
    .action("Reduce the hours and order again.")
}

/// The selected food is not in today's menu.
pub fn meal_food_not_found() -> Notice {
    Notice::refusal(
        "Food Not Found",
        "The selected food is not on the menu right now.",
    )
    .command("/trading restourant")
    .action("Reopen the menu and pick a food that is listed.")
}

/// A food order needs a guild, but the player has none.
pub fn meal_no_guild() -> Notice {
    Notice::refusal(
        "No Guild",
        "Food buffs are delivered to a guild, but you do not belong to one.",
    )
    .command("/trading restourant")
    .action("Join or create a guild with `/guild`, then order again.")
}

/// A raw button payload of a restaurant order was unreadable.
pub fn meal_bad_select() -> Notice {
    Notice::failure(
        "Stale Selection",
        "The food selection carried data the bot cannot read.",
    )
    .footer("Restaurant menu")
    .action("Reopen the menu and select a food again.")
}

/// The market screen was closed by pressing Cancel.
pub fn market_cancelled() -> Notice {
    Notice::info("Transaction Cancelled", "The order was cancelled before payment.")
        .command("/market")
        .action("Run `/market` again if you still want to buy.")
}

/// The market screen closed itself because nothing happened.
pub fn market_timed_out() -> Notice {
    Notice::info("Market Timed Out", "The market screen closed after a period of inactivity.")
        .command("/market")
        .action("Run `/market` again to start a new order.")
}

/// The player already holds the role a button would grant.
pub fn role_already_held(role: &str) -> Notice {
    Notice::info("Role Already Held", format!("You already have the **{role}** role."))
        .footer("Role button")
        .action("Nothing to do — you already have it.")
}

/// Discord refused the role assignment.
pub fn role_assign_failed(role: &str, reason: &str) -> Notice {
    Notice::failure(
        "Role Not Assigned",
        format!("Discord did not allow the **{role}** role to be added."),
    )
    .footer("Role button")
    .cause(reason)
    .action("Ask an administrator to check that the bot's role sits above this role and has Manage Roles.")
}

// ---------------------------------------------------------------------------
// Guild
// ---------------------------------------------------------------------------

/// The player is not in a guild, so guild-only actions cannot run.
pub fn guild_not_a_member() -> Notice {
    Notice::info("Not in a Guild", "You are not a member of any guild.")
        .command("/guild")
        .action("Join or create a guild first.")
}

/// A guild withdrawal failed in the database.
pub fn guild_withdraw_failed() -> Notice {
    Notice::failure(
        "Withdrawal Failed",
        "The guild withdrawal could not be processed because the database returned an error.",
    )
    .command("/guild")
    .action("Try again in a moment. If it keeps failing, report it to an administrator.")
}

/// The target of a guild action could not be resolved.
pub fn guild_invalid_target() -> Notice {
    Notice::refusal(
        "Invalid Target",
        "No usable player was named for this action.",
    )
    .command("/guild")
    .action("Mention the player explicitly and run the command again.")
}

/// The named player has no database account.
pub fn guild_target_not_registered() -> Notice {
    Notice::refusal(
        "Player Not Registered",
        "That player has no account in the Rain Server database.",
    )
    .command("/guild")
    .action("Ask them to run `/bind` first.")
}

/// A food purchase was attempted for something that does not exist.
pub fn guild_food_not_found(food_id: i64) -> Notice {
    Notice::refusal(
        "Food Not Found",
        "The selected food is no longer in the restaurant menu.",
    )
    .footer("Guild food button")
    .detail(format!("Food id `{food_id}` was not found."))
    .action("Reopen the restaurant menu and pick a food that is listed.")
}

/// The extension duration is not one of the accepted lengths.
///
/// The restaurant sells 30-day and 60-day extensions only; any other number is
/// refused rather than rounded, because the price is computed from it.
pub fn guild_bad_duration(days: i64) -> Notice {
    Notice::refusal(
        "Duration Not Accepted",
        "Food extensions come in 30-day or 60-day blocks only.",
    )
    .footer("Guild food")
    .detail(format!("Requested: **{days}** days."))
    .detail("Accepted: **30** or **60** days.")
    .action("Choose either 30 or 60 days.")
}

/// A days value in a button payload could not be read as a number.
pub fn guild_bad_days(raw: &str) -> Notice {
    Notice::failure(
        "Stale Button",
        "This food button carries a days value the bot cannot read.",
    )
    .footer("Guild food button")
    .detail(format!("Raw value: `{raw}`."))
    .action("Reopen the restaurant menu to get fresh buttons.")
}

/// A button payload was malformed.
pub fn guild_bad_button() -> Notice {
    Notice::failure(
        "Stale Button",
        "This button carries data the bot cannot read, usually because it is from an older post.",
    )
    .footer("Guild food button")
    .action("Reopen the menu to get fresh buttons.")
}

/// A prefix command was typed without its argument.
pub fn tag_missing_argument() -> Notice {
    Notice::refusal("Missing Argument", "This prefix command needs an argument.")
        .footer("Prefix command")
        .action("Type the command followed by its argument and send it again.")
}

// ---------------------------------------------------------------------------
// Bounty submission lifecycle
// ---------------------------------------------------------------------------

/// The judge pressed a button on a submission that was already processed.
pub fn bounty_submission_expired() -> Notice {
    Notice::info(
        "Submission Already Processed",
        "The submission behind this button has expired or has already been decided.",
    )
    .footer("Bounty judge button")
    .action("Nothing to do — the submission is no longer pending.")
}

/// The submission form timed out before every field was chosen.
pub fn bounty_form_timed_out() -> Notice {
    Notice::info(
        "Submission Cancelled",
        "The form closed before every field was chosen, so nothing was submitted.",
    )
    .command("/submit")
    .action("Run `/submit` again and pick the category and bounty number.")
}

/// The judge closed the review without deciding.
pub fn bounty_review_cancelled() -> Notice {
    Notice::info(
        "Review Cancelled",
        "The review was closed without approving or rejecting the submission.",
    )
    .footer("Bounty judge button")
    .action("Press Approve or Reject on the submission to decide it.")
}

/// The judge did not type a rejection reason in time.
pub fn bounty_rejection_timed_out() -> Notice {
    Notice::info(
        "Rejection Cancelled",
        "No reason was typed before the prompt expired, so the submission stands.",
    )
    .footer("Bounty judge button")
    .action("Press Reject again and type the reason within the prompt window.")
}

/// The administrator cancelled a mass distribution.
pub fn bounty_distribution_cancelled() -> Notice {
    Notice::info(
        "Distribution Cancelled",
        "The reward distribution was cancelled, so nothing was granted.",
    )
    .footer("Distribution button")
    .action("Nothing to do — no member was changed.")
}

// ---------------------------------------------------------------------------
// Admin: reminders
// ---------------------------------------------------------------------------

/// The reminder scheduler was never started.
pub fn scheduler_unavailable() -> Notice {
    Notice::failure(
        "Scheduler Unavailable",
        "The reminder scheduler is not running, so reminders cannot be managed.",
    )
    .command("/remind")
    .action("Report this to the developer; it needs a restart-side fix.")
}

/// A subcommand was not provided at all.
pub fn remind_no_subcommand(available: &[&str]) -> Notice {
    Notice::refusal("Nothing Selected", "No reminder action was chosen.")
        .command("/remind")
        .detail(format!("Available: {}.", available.join(", ")))
        .action("Pick one of the listed actions.")
}

/// A subcommand was recognised but its payload was the wrong shape.
pub fn remind_bad_subcommand(name: &str) -> Notice {
    Notice::failure(
        "Subcommand Unreadable",
        "The subcommand data could not be read, so nothing was changed.",
    )
    .command("/remind")
    .detail(format!("Subcommand: `{name}`."))
    .cause("expected a subcommand payload, got another kind")
    .action("Re-run the command and pick a subcommand from the list.")
}

/// The subcommand name is not one this command offers.
pub fn remind_unknown_subcommand(name: &str, available: &[&str]) -> Notice {
    Notice::refusal(
        "Unknown Action",
        format!("`{name}` is not a reminder action."),
    )
    .command("/remind")
    .detail(format!("Available: {}.", available.join(", ")))
    .action("Pick one of the listed actions.")
}

/// A required reminder field was left empty.
pub fn remind_field_missing(field: &str) -> Notice {
    Notice::refusal(
        "Field Required",
        format!("The **{field}** field is required but was left empty."),
    )
    .command("/remind")
    .action(format!("Fill in **{field}** and run the command again."))
}

/// A reminder id was not supplied.
pub fn remind_id_missing() -> Notice {
    Notice::refusal("Reminder Id Required", "No reminder id was given.")
        .command("/remind")
        .action("Use `/remind list` to see the ids, then pass the one you mean.")
}

/// A channel reference could not be resolved to a channel.
pub fn remind_channel_not_found(input: &str, reason: &str) -> Notice {
    Notice::failure(
        "Channel Not Found",
        format!("`{input}` did not match any text channel in this server."),
    )
    .command("/remind")
    .cause(reason)
    .detail("Accepted forms: raw id `1492149802052485371`, mention `<#1492149802052485371>`, or name `#announcements`.")
    .action("Copy the channel id from the channel itself, or use the mention form.")
}

/// A reminder can only be scheduled inside a guild.
pub fn remind_needs_guild() -> Notice {
    Notice::refusal(
        "Server Only",
        "Reminders are delivered to a channel in this server, so they cannot be set up here.",
    )
    .command("/remind")
    .action("Run the command inside the server that has the target channel.")
}

/// A channel mention was written in a form the bot cannot parse.
pub fn remind_bad_channel_mention(input: &str) -> Notice {
    Notice::refusal(
        "Channel Mention Unreadable",
        format!("`{input}` looks like a channel mention but its id could not be read."),
    )
    .command("/remind")
    .action("Use the raw channel id, or copy the mention by typing `#channel` in Discord.")
}

/// A reminder time is outside the accepted range.
pub fn remind_bad_interval(minutes: i64, max_minutes: i64) -> Notice {
    Notice::refusal(
        "Interval Out of Range",
        "That reminder interval cannot be scheduled.",
    )
    .command("/remind")
    .progress(ProgressRow::new("Minutes", minutes, max_minutes.max(1)))
    .detail(format!("Allowed: **1 to {max_minutes}** minutes."))
    .action(format!("Enter a value between 1 and {max_minutes}."))
}

// ---------------------------------------------------------------------------
// Admin: title editing
// ---------------------------------------------------------------------------
//
// These edit `static/bounty_title.json`, which production reads on startup. A
// silent or vague failure here can leave a malformed file that only surfaces on
// the next restart, so every refusal names the file, the step and the entry.

/// The edit session behind these buttons has expired.
pub fn title_edit_session_expired() -> Notice {
    Notice::info(
        "Edit Session Expired",
        "This edit session is no longer open, so the button did nothing.",
    )
    .command("/title_edit")
    .action("Run `/title_edit` again to start a new session.")
}

/// Someone other than the editor pressed the button.
pub fn title_edit_not_yours(owner: Option<u64>) -> Notice {
    let mut notice = Notice::info(
        "Not Your Session",
        "This edit session belongs to another administrator.",
    )
    .command("/title_edit");
    if let Some(id) = owner {
        notice = notice.detail(format!("The session was started by <@{id}>."));
    }
    notice.action("Start your own session with `/title_edit`.")
}

/// `bounty_title.json` could not be read from disk.
pub fn title_file_unreadable(path: &str, reason: &str) -> Notice {
    Notice::failure(
        "Title File Unreadable",
        "The title configuration could not be read, so nothing was changed.",
    )
    .command("/title_edit")
    .detail(format!("File: `{path}`."))
    .cause(reason)
    .action("Check that the file exists and is readable, then retry the edit.")
}

/// `bounty_title.json` is not valid JSON.
pub fn title_file_malformed(path: &str, reason: &str) -> Notice {
    Notice::failure(
        "Title File Malformed",
        "The title configuration is not valid JSON, so nothing was changed.",
    )
    .command("/title_edit")
    .detail(format!("File: `{path}`."))
    .cause(reason)
    .action("Fix the JSON by hand or restore a backup before editing again. The bot reads this file on startup.")
}

/// The entry the session refers to is no longer in the file.
pub fn title_entry_missing(index: usize, trigger: &str) -> Notice {
    Notice::failure(
        "Entry Not Found",
        "The entry this session was editing is no longer in the file, so nothing was changed.",
    )
    .command("/title_edit")
    .detail(format!("Entry index: **{index}**."))
    .detail(format!("Trigger being matched: `{trigger}`."))
    .action("Reopen the file with `/title_edit` and select the entry again; it may have been removed by someone else.")
}

/// A required modal field was left empty.
pub fn title_field_missing(field: &str) -> Notice {
    Notice::refusal(
        "Field Required",
        format!("The **{field}** field is required but was left empty."),
    )
    .command("/title_edit")
    .action(format!("Fill in **{field}** and submit the form again."))
}

/// A button payload carried an unreadable prefix or index.
pub fn title_bad_button(reason: &str) -> Notice {
    Notice::failure(
        "Stale Button",
        "This title-editing button carries data the bot cannot read.",
    )
    .command("/title_edit")
    .cause(reason)
    .action("Run `/title_edit` again to get fresh buttons.")
}

/// The attachment for a new title image could not be found or downloaded.
pub fn title_image_unavailable(reason: &str) -> Notice {
    Notice::failure(
        "Image Not Applied",
        "The new title image could not be fetched, so the entry was not changed.",
    )
    .command("/title_edit")
    .cause(reason)
    .action("Re-attach the image and submit the form again.")
}

/// The moderator does not hold an admin role.
pub fn admin_only(command: &str) -> Notice {
    Notice::refusal(
        "Access Denied",
        "This command is restricted to administrators.",
    )
    .command(command)
    .action("Ask a server administrator to run it.")
}

// ---------------------------------------------------------------------------
// Assistant (/gpt)
// ---------------------------------------------------------------------------

/// The assistant was asked for something it does not do.
pub fn gpt_unknown_request() -> Notice {
    Notice::refusal(
        "Request Not Understood",
        "That is not one of the actions the assistant offers.",
    )
    .command("/gpt")
    .detail("Available: **chat** and **image**.")
    .action("Pick one of the listed actions and try again.")
}

/// A chat request arrived without a question.
pub fn gpt_no_question() -> Notice {
    Notice::refusal("Nothing to Answer", "No question was included with this request.")
        .command("/gpt")
        .action("Type your question and send it again.")
}

// ---------------------------------------------------------------------------
// Binded commands (card, huntercard, transfer)
// ---------------------------------------------------------------------------

/// No user was resolved from the command arguments.
pub fn no_target_user() -> Notice {
    Notice::refusal("No Player Selected", "This action needs a player, but none was found.")
        .action("Mention the player explicitly and run the command again.")
}

/// The named player has no account.
pub fn target_not_registered() -> Notice {
    Notice::refusal(
        "Player Not Registered",
        "That player is not registered as a Hunter.",
    )
    .action("Ask them to run `/bind` first.")
}

/// A hunter card title exceeds the word limit.
pub fn huntercard_title_too_long(words: usize, limit: usize) -> Notice {
    Notice::refusal(
        "Title Too Long",
        "A hunter title may not exceed the word limit.",
    )
    .command("/huntercard")
    .progress(ProgressRow::new("Words", words as i64, limit as i64))
    .action(format!("Shorten the title to {limit} words or fewer."))
}

/// The card render engine failed.
pub fn huntercard_render_failed(reason: &str) -> Notice {
    Notice::failure(
        "Card Not Rendered",
        "The card image could not be generated.",
    )
    .command("/huntercard")
    .cause(reason)
    .action("Try again. If it keeps failing, report it to an administrator.")
}

/// The card storage directory could not be created.
pub fn huntercard_storage_failed(reason: &str) -> Notice {
    Notice::failure(
        "Storage Unavailable",
        "The hunter card storage directory could not be created.",
    )
    .command("/huntercard")
    .cause(reason)
    .action("Report this to an administrator; it needs a server-side fix.")
}

/// A missing subcommand option.
pub fn missing_subcommand() -> Notice {
    Notice::refusal("Nothing Selected", "No subcommand was provided with this command.")
        .action("Pick one of the subcommands that the command offers.")
}

/// The button payload could not be parsed.
pub fn bad_button_payload() -> Notice {
    Notice::failure(
        "Stale Button",
        "This button carries data the bot cannot read, usually because it is from an older post.",
    )
    .action("Run the command again to get a fresh button.")
}

/// A save file was not found on the message.
pub fn transfer_no_file() -> Notice {
    Notice::refusal(
        "No Save File",
        "No matching save file was found on the message.",
    )
    .command("/transfer")
    .detail("The file must be attached and named correctly, and within the size limit.")
    .action("Attach the save file and run the command again.")
}

/// A saved user id in a button payload was not a number.
pub fn transfer_bad_user_id(raw: &str) -> Notice {
    Notice::failure(
        "Stale Button",
        "The user id carried by this button is not readable.",
    )
    .footer("Transfer button")
    .detail(format!("Raw value: `{raw}`."))
    .action("Reopen the transfer post to get fresh buttons.")
}

/// The target user could not be fetched from Discord.
pub fn transfer_user_fetch_failed() -> Notice {
    Notice::failure(
        "Player Unavailable",
        "Discord did not return the player for this transfer.",
    )
    .footer("Transfer button")
    .action("Try again shortly; if it keeps failing, check that the player still shares a server with the bot.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coins_are_grouped_and_labelled() {
        assert_eq!(coins(0), "0 BC");
        assert_eq!(coins(1), "1 BC");
        assert_eq!(coins(1_000), "1,000 BC");
        assert_eq!(coins(2_500), "2,500 BC");
        assert_eq!(coins(65_535), "65,535 BC");
        assert_eq!(coins(-1_000), "-1,000 BC");
    }

    #[test]
    fn an_insufficient_balance_notice_shows_the_shortfall() {
        let view = market_insufficient_balance_formatted(1_000, 2_500, coins).view();
        assert!(view.title.contains("Not Enough Bounty Coin"), "got {}", view.title);
        let progress = view.field("📊 Progress").expect("progress field");
        assert!(progress.contains("1,000/2,500") || progress.contains("1000/2500"), "got {progress}");
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("2,500 BC"), "got {details}");
        assert!(details.contains("1,500 BC"), "got {details}");
    }

    #[test]
    fn an_out_of_stock_notice_names_the_item_and_the_remaining_units() {
        let view = market_out_of_stock("Gacha Ticket", 10, 3).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("3 units left"), "got {details}");
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("3 or fewer"), "got {action}");
    }

    #[test]
    fn an_out_of_stock_notice_with_nothing_left_suggests_waiting() {
        let view = market_out_of_stock("Gacha Ticket", 10, 0).view();
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("restock"), "got {action}");
    }

    #[test]
    fn a_full_party_shows_the_roster_against_its_size() {
        let view = party_full(4, 4, Some(42)).view();
        assert!(view.title.contains("Party Full"), "got {}", view.title);
        let progress = view.field("📊 Progress").expect("progress field");
        assert!(progress.contains("4/4"), "got {progress}");
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("<@42>"), "got {details}");
    }

    #[test]
    fn a_leader_only_notice_names_the_real_leader() {
        let view = party_leader_only("edit", Some(7)).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("<@7>"), "got {details}");
    }

    #[test]
    fn a_leader_only_notice_without_a_known_leader_still_offers_a_step() {
        let view = party_leader_only("close", None).view();
        assert!(view.field("📋 Details").is_none(), "got {:?}", view.fields);
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("/party"), "got {action}");
    }

    #[test]
    fn a_busy_party_is_informational_not_an_error() {
        let view = party_busy().view();
        assert!(view.title.starts_with("ℹ️"), "got {}", view.title);
    }

    #[test]
    fn a_render_failure_is_an_error_and_carries_the_cause() {
        let view = huntercard_render_failed("missing font").view();
        assert!(view.title.starts_with("🛑"), "got {}", view.title);
        let cause = view.field("🔍 Cause").expect("cause field");
        assert!(cause.contains("missing font"), "got {cause}");
    }

    #[test]
    fn a_refusal_is_a_warning_not_an_error() {
        let view = party_edit_bad_format().view();
        assert!(view.title.starts_with("⚠️"), "got {}", view.title);
    }

    #[test]
    fn the_edit_prompt_timeout_names_the_window() {
        let view = party_edit_timeout(60).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("60 seconds"), "got {details}");
    }

    #[test]
    fn a_food_duration_refusal_names_both_accepted_lengths() {
        let view = guild_bad_duration(45).view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("45"), "got {details}");
        assert!(details.contains("30") && details.contains("60"), "got {details}");
    }

    #[test]
    fn an_unreadable_days_value_is_reported_as_a_stale_button() {
        let view = guild_bad_days("abc").view();
        assert!(view.title.starts_with("🛑"), "got {}", view.title);
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("`abc`"), "got {details}");
    }

    #[test]
    fn a_transfer_without_a_file_points_at_the_attachment() {
        let view = transfer_no_file().view();
        let action = view.field("✅ What to do").expect("action field");
        assert!(action.contains("Attach"), "got {action}");
    }

    #[test]
    fn a_stale_transfer_button_shows_the_raw_value() {
        let view = transfer_bad_user_id("abc").view();
        let details = view.field("📋 Details").expect("details field");
        assert!(details.contains("`abc`"), "got {details}");
    }

    #[test]
    fn every_builder_names_a_recovery_step_or_says_there_is_none() {
        let notices: Vec<(&str, Notice)> = vec![
            ("meal_disabled", meal_disabled()),
            ("meal_bad_quantity", meal_bad_quantity()),
            ("meal_too_long", meal_too_long(2_000, 1_440)),
            ("meal_food_not_found", meal_food_not_found()),
            ("meal_no_guild", meal_no_guild()),
            ("meal_bad_select", meal_bad_select()),
            ("market_cancelled", market_cancelled()),
            ("market_timed_out", market_timed_out()),
            ("role_already_held", role_already_held("Member")),
            ("role_assign_failed", role_assign_failed("Member", "missing permissions")),
            ("gpt_unknown_request", gpt_unknown_request()),
            ("gpt_no_question", gpt_no_question()),
            ("tag_missing_argument", tag_missing_argument()),
            ("party_request_pending", party_request_pending()),
            ("party_no_longer_exists", party_no_longer_exists()),
            ("party_declined_full", party_declined_full(4)),
            ("admin_only", admin_only("/remind")),
            ("scheduler_unavailable", scheduler_unavailable()),
            ("remind_no_subcommand", remind_no_subcommand(&["add", "list"])),
            ("remind_bad_subcommand", remind_bad_subcommand("add")),
            ("remind_unknown_subcommand", remind_unknown_subcommand("wat", &["add"])),
            ("remind_field_missing", remind_field_missing("channel")),
            ("remind_id_missing", remind_id_missing()),
            ("remind_channel_not_found", remind_channel_not_found("#nope", "no such channel")),
            ("remind_needs_guild", remind_needs_guild()),
            ("remind_bad_channel_mention", remind_bad_channel_mention("<#abc>")),
            ("remind_bad_interval", remind_bad_interval(0, 1_440)),
            ("title_edit_session_expired", title_edit_session_expired()),
            ("title_edit_not_yours", title_edit_not_yours(Some(1))),
            ("title_file_unreadable", title_file_unreadable("static/bounty_title.json", "denied")),
            ("title_file_malformed", title_file_malformed("static/bounty_title.json", "bad json")),
            ("title_entry_missing", title_entry_missing(2, "0_21")),
            ("title_field_missing", title_field_missing("trigger")),
            ("title_bad_button", title_bad_button("bad prefix")),
            ("title_image_unavailable", title_image_unavailable("download failed")),
            ("party_leader_only", party_leader_only("edit", Some(1))),
            ("party_full", party_full(4, 4, Some(1))),
            ("party_busy", party_busy()),
            ("party_leader_cannot_leave", party_leader_cannot_leave(1)),
            ("party_not_a_member", party_not_a_member()),
            ("party_already_member", party_already_member(4, 4)),
            ("party_post_failed", party_post_failed()),
            ("party_edit_bad_format", party_edit_bad_format()),
            ("party_edit_timeout", party_edit_timeout(60)),
            ("party_request_handled", party_request_handled()),
            ("party_post_gone", party_post_gone()),
            ("party_requester_is_member", party_requester_is_member()),
            ("market_item_gone", market_item_gone()),
            ("bounty_submission_expired", bounty_submission_expired()),
            ("bounty_form_timed_out", bounty_form_timed_out()),
            ("bounty_review_cancelled", bounty_review_cancelled()),
            ("bounty_rejection_timed_out", bounty_rejection_timed_out()),
            ("bounty_distribution_cancelled", bounty_distribution_cancelled()),
            ("market_total_too_large", market_total_too_large(5_000, 2_147_483_647)),
            ("market_too_many_items", market_too_many_items(70_000, 65_535)),
            ("market_empty", market_empty()),
            ("trading_disabled", trading_disabled()),
            ("trading_section_closed", trading_section_closed("Casino")),
            ("trading_bad_quantity", trading_bad_quantity()),
            ("trading_no_guild", trading_no_guild(true)),
            ("guild_not_a_member", guild_not_a_member()),
            ("guild_withdraw_failed", guild_withdraw_failed()),
            ("guild_invalid_target", guild_invalid_target()),
            ("guild_target_not_registered", guild_target_not_registered()),
            ("guild_food_not_found", guild_food_not_found(9)),
            ("guild_bad_duration", guild_bad_duration(45)),
            ("guild_bad_button", guild_bad_button()),
            ("no_target_user", no_target_user()),
            ("target_not_registered", target_not_registered()),
            ("huntercard_title_too_long", huntercard_title_too_long(5, 3)),
            ("huntercard_storage_failed", huntercard_storage_failed("denied")),
            ("missing_subcommand", missing_subcommand()),
            ("bad_button_payload", bad_button_payload()),
            ("transfer_bad_user_id", transfer_bad_user_id("x")),
            ("transfer_user_fetch_failed", transfer_user_fetch_failed()),
        ];

        for (name, notice) in notices {
            let view = notice.view();
            // Every refusal must leave a recoverable route, except the ones whose
            // note is purely informational and already says what happened.
            let has_action = view.field("✅ What to do").is_some();
            assert!(has_action, "{name} carries no actionable step");
            // And none of them may smuggle a raw variant wrapper into the UI.
            assert!(!view.all_text().contains("Custom("), "{name} leaked a variant");
        }
    }

    #[test]
    fn no_builder_produces_an_untitled_or_empty_notice() {
        let view = market_insufficient_balance(0, 0, "items").view();
        assert!(!view.title.trim().is_empty());
        assert!(!view.description.trim().is_empty());
    }
}
