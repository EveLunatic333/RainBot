use serenity::all::{ButtonStyle, User};
use serenity::builder::{
    CreateActionRow, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};
use crate::reusable::{
    postgress::card::{Card, Event},
    utils::{color, Color, MyTime},
};
use crate::Components;
use crate::Mybundle;
use super::MyErr;
use super::market::Market;
use super::bounty::{BBQ, BountyTitle};

fn make_button() -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        Components::normal_button("use", "use", ButtonStyle::Primary, "👍"),
        Components::normal_button("next", "next", ButtonStyle::Success, "➡️"),
    ])
}

/// The cooldown block for one group, as shown on the player card.
///
/// The old build printed "Different Bounty CD" and "Same Bounty CD" so a player
/// knew when each kind of bounty opened again. The per-group card had replaced
/// that with a bare "Ready" or a single countdown, which lost the distinction.
/// Both waits are printed again, each only while it is actually running.
///
/// Kept free of Discord types so the exact wording can be unit-tested.
fn cooldown_lines(
    history: &[super::bounty_cooldown::GroupHistory],
    category: &super::bounty::Category,
    policy: &super::bounty_cooldown::CooldownPolicy,
    now: i64,
) -> String {
    use super::bounty_cooldown::{self, CooldownVerdict};

    let Some(windows) = bounty_cooldown::reopen_windows(history, category, policy, now) else {
        return "✅ Ready".to_string();
    };

    let mut parts: Vec<String> = Vec::new();
    // Only print a line whose wait is still running: a countdown the player
    // cannot act on reads as if that bounty were still locked.
    if let CooldownVerdict::Blocked { ends_at } = windows.change {
        parts.push(format!("Different Bounty CD: ⏳ <t:{ends_at}:R>"));
    }
    if let CooldownVerdict::Blocked { ends_at } = windows.repeat {
        parts.push(format!("Same Bounty CD: ⏳ <t:{ends_at}:R>"));
    }

    if parts.is_empty() {
        "✅ Ready".to_string()
    } else {
        parts.join("\n")
    }
}

impl Card {
    fn w_name(&self) -> String {
        let names = vec![
            "Great Sword", "Heavy Bowgun", "Hammer", "Lance", "Sword & Shield",
            "Light Bowgun", "Dual Swords", "Long Sword", "Hunting Horn", "Gunlance",
            "Bow", "Tonfa", "Switch Axe F", "Magnet Spike",
        ];
        names.get(self.weapon_type as usize).cloned().unwrap_or("Unknown Weapon").to_string()
    }

    pub fn get_path(&self) -> String {
        let iconlist = vec![
             "https://media.discordapp.net/attachments/1068440173479739393/1068440322977312868/GS.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440324617281626/HS.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440323501596792/H.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440324931862599/L.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440373548044348/SS.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440325309345822/LB.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440322260086794/DS.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440372474302464/LS.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440324088807466/HH.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440322633383946/GL.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440321907761162/B.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440373757743154/T.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440373132800080/SAF.png",
             "https://media.discordapp.net/attachments/1068440173479739393/1068440372709167174/MS.png",
        ];
        iconlist
            .get(self.weapon_type as usize)
            .unwrap_or(&" ")
            .to_string()
    }

    fn g_name(&self) -> String {
        self.guild_name.clone().unwrap_or_else(|| "No guild".to_string())
    }

    fn g_id(&self) -> String {
        self.guild_id.map(|id| id.to_string()).unwrap_or_else(|| "No id".to_string())
    }

    fn hrp(&self) -> u8 {
        match self.hrp {
            n if n >= 999 => 7,
            n if n > 299 => 6,
            n if n > 99 => 5,
            n if n > 50 => 4,
            n if n > 30 => 3,
            n if n > 1 => 2,
            _ => 1,
        }
    }

    fn last_login(&self) -> String {
        format!("<t:{}:R>", self.login)
    }

    fn crete_embed(&self, user: &User, is_admin: bool) -> CreateEmbed {
        let user_info = if is_admin {
            format!(
                "👤 **Username:** {}\n🆔 **User ID:** {}\n🆔 **Char ID:** {}\n🕒 **Last Login:** {}",
                &self.username, self.user_id, self.char_id, self.last_login()
            )
        } else {
            format!(
                "👤 **Username:** {}\n🕒 **Last Login:** {}",
                &self.username, self.last_login()
            )
        };

        let h = self.playtime / 3600;
        let m = (self.playtime % 3600) / 60;
        let s = self.playtime % 60;

        let char_info = format!(
            "🛡️ **Weapon:** {}\n⭐ **HR:** {}\n🔥 **GR:** {}\n🎖️ **GSR:** {}\n⌛ **Total Playtime:** {}H:{:02}M:{:02}S",
            self.w_name(), self.hrp(), self.gr, self.gsr, h, m, s
        );

        CreateEmbed::new()
            .title(self.name.as_str())
            .fields(vec![
                ("User Information", user_info, false),
                ("Character Progress", char_info, false),
                ("Guild Association", format!("🏰 **Name:** {}\n🆔 **Guild ID:** {}", self.g_name(), self.g_id()), false),
            ])
            .footer(CreateEmbedFooter::new(format!("owned by {}", user.name)).icon_url(user.face()))
            .colour(color("ff", "55", "00"))
            .thumbnail(&self.get_path())
    }

    pub fn card(&self, user: &User, is_admin: bool) -> CreateInteractionResponse {
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(self.crete_embed(user, is_admin))
                .ephemeral(true),
        )
    }

    pub fn bind(&self, user: &User) -> CreateInteractionResponse {
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(self.crete_embed(user, false))
                .components(vec![make_button()])
                .ephemeral(true)
                .content("Please Select Your Character before using other commands."),
        )
    }

    pub fn edit_bind(&self, user: &User) -> EditInteractionResponse {
        EditInteractionResponse::new()
            .embed(self.crete_embed(user, false))
            .components(vec![make_button()])
    }
}

impl Event {
    fn get_meal_name(&self) -> &'static str {
        match self.meal_id {
            1 => "Buchigire", 109 => "Unaffected+3", 163 => "Great Luck",
            253 => "Super HG Earplugs", 296 => "Caring+3", 332 => "Adrenaline+2",
            368 => "Dragon Wind Breaker", 395 => "Peerless", 422 => "Goddess Embrace",
            455 => "Hunger Negated", 500 => "Wide Area+3", 527 => "Hazard Res (Large)",
            551 => "Divine Whim", 596 => "Paralysis Negated", 614 => "Sleep Negated",
            632 => "Poison Negated", 650 => "Stun Negated", 668 => "All Res+20",
            704 => "Medical Sage", 708 => "Encourage+2", 712 => "Assistance",
            716 => "Incitement", _ => "Others",
        }
    }

    fn embed(
        &self,
        user: &User,
        history: &[super::bounty_cooldown::GroupHistory],
        policy: &super::bounty_cooldown::CooldownPolicy,
    ) -> Result<CreateEmbed, MyErr> {
        use super::bounty::Category;

        let now = MyTime::now();

        // Ask the same evaluator the submit flow uses, so the card can never
        // disagree with what the bot actually enforces.
        let status = |category: Category| -> String {
            cooldown_lines(history, &category, policy, now)
        };

        let latest = BountyTitle::name(&self.latest_bounty);

        let meal_status = if self.meal_expiry > now {
            format!("{}, <t:{}:R>", self.get_meal_name(), self.meal_expiry)
        } else {
            "No active buff".to_string()
        };

        let time_completed = if self.latest_bounty_time == 0 {
            "Not completed yet".to_string()
        } else {
            format!("<t:{}:R>", self.latest_bounty_time)
        };

        let eco_desc = format!(
            "💰 **Bounty Coin:** {}\n💵 **In-Game Zeny:** Encrypted in Savedata\n🎫 **Gacha Ticket:** {} Ticket\n💎 **Premium Gacha:** {} Point",
            Market::currency(self.bounty as i64), self.gacha, self.gacha_premium
        );

        // Cooldown is per group, so the card lists the two groups rather than the
        // old per-category split, which no longer maps to anything real.
        // `status(Bronze)` is the shared group: the evaluator maps every standard
        // category onto the same row, so asking about Free separately would always
        // print the same answer twice.
        //
        // Each group gets its own block so the two countdowns are never read as
        // belonging to the other group.
        let cooldown_desc = format!(
            "🎯 **Standard** (Bronze / Silver / Gold / Free)\n{}\n\n⭐ **Event**\n{}\n\n🥉 **Bronze:** {}\n🥈 **Silver:** {}\n🥇 **Gold:** {}",
            status(Category::Bronze),
            status(Category::Event),
            BBQ::name_encode(self.bronze as u8),
            BBQ::name_encode(self.silver as u8),
            BBQ::name_encode(self.gold as u8)
        );

        let event_desc = format!(
            "🕜 **Latest Bounty:** {latest}\n🕒 **Time Completed:** {time_completed}\n🍽️ **Restaurant Buff:** {}\n\n{}",
            meal_status, cooldown_desc
        );

        Ok(CreateEmbed::new()
            .author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
            .title("Event & Logistics Card")
            .field("Economy Status", eco_desc, false)
            .field("Activity & Buffs", event_desc, false)
            .color(Color::Green.throw()))
    }

    pub async fn response<T: Mybundle>(&self, user: &User, bnd: &T) -> Result<(), MyErr> {
        let policy = super::bounty_cooldown::CooldownPolicy::from(&bnd.init().bounty.cooldown_hours);

        // One query for at most two rows; no per-category round trips.
        let did = user.id.to_string();
        let stored = match crate::reusable::postgress::gacha::group_history(
            crate::reusable::postgress::get_pool(),
            &did,
        )
        .await
        {
            Ok(rows) => {
                let flat: Vec<_> = rows
                    .iter()
                    .map(|r| (r.group_id, r.category, r.bbq, r.completed_at))
                    .collect();
                super::bounty_cooldown::history_from_rows(&flat)
            }
            // The legacy column is folded in below either way, so a missing table
            // still leaves the standard timer readable.
            Err(_) => Vec::new(),
        };
        // Same fold the submit flow applies, so the card cannot promise a slot the
        // bot will then refuse (or hide one that is open).
        let history = super::bounty_cooldown::history_with_legacy(
            &stored,
            &self.latest_bounty,
            self.latest_bounty_time,
        );

        Ok(Components::response_adv(
            bnd,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(self.embed(user, &history, &policy)?),
            ),
        ).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::cooldown_lines;
    use crate::reusable::component::bounty::{BBQ, Category};
    use crate::reusable::component::bounty_cooldown::{
        CooldownPolicy, GroupHistory,
    };

    const T0: i64 = 1_700_000_000;
    const HOUR: i64 = 60 * 60;

    fn history(category: Category, bbq: BBQ, at: i64) -> Vec<GroupHistory> {
        vec![GroupHistory {
            group: crate::reusable::component::bounty_cooldown::CooldownGroup::from_category(
                &category,
            ),
            category,
            bbq,
            completed_at: at,
        }]
    }

    #[test]
    fn a_fresh_run_shows_both_bounty_cooldowns() {
        // The regression: the new card printed a bare "Ready" with no countdown.
        let hist = history(Category::Gold, BBQ::BBQ07, T0);
        let out = cooldown_lines(&hist, &Category::Bronze, &CooldownPolicy::default(), T0 + HOUR);
        assert!(
            out.contains("Different Bounty CD"),
            "missing the different-bounty countdown: {out}"
        );
        assert!(
            out.contains("Same Bounty CD"),
            "missing the same-bounty countdown: {out}"
        );
        // 20 h for a different BBQ, 40 h for the exact repeat.
        assert!(out.contains(&format!("<t:{}:R>", T0 + 20 * HOUR)), "got {out}");
        assert!(out.contains(&format!("<t:{}:R>", T0 + 40 * HOUR)), "got {out}");
    }

    #[test]
    fn the_different_line_comes_before_the_same_line() {
        let hist = history(Category::Gold, BBQ::BBQ07, T0);
        let out = cooldown_lines(&hist, &Category::Gold, &CooldownPolicy::default(), T0 + HOUR);
        let different = out.find("Different Bounty CD").expect("different line");
        let same = out.find("Same Bounty CD").expect("same line");
        assert!(different < same, "order should match the old build: {out}");
    }

    #[test]
    fn only_the_running_wait_is_shown() {
        // 25 hours in, any other bounty is open but the exact repeat is not.
        let hist = history(Category::Gold, BBQ::BBQ07, T0);
        let out = cooldown_lines(&hist, &Category::Gold, &CooldownPolicy::default(), T0 + 25 * HOUR);
        assert!(!out.contains("Different Bounty CD"), "should be open: {out}");
        assert!(out.contains("Same Bounty CD"), "should still be locked: {out}");
        assert!(out.contains(&format!("<t:{}:R>", T0 + 40 * HOUR)), "got {out}");
    }

    #[test]
    fn an_open_bounty_says_ready_without_a_countdown() {
        let hist = history(Category::Gold, BBQ::BBQ07, T0);
        let out = cooldown_lines(&hist, &Category::Gold, &CooldownPolicy::default(), T0 + 41 * HOUR);
        assert_eq!(out, "✅ Ready");
    }

    #[test]
    fn no_history_says_ready() {
        let out = cooldown_lines(&[], &Category::Event, &CooldownPolicy::default(), T0);
        assert_eq!(out, "✅ Ready");
    }

    #[test]
    fn a_corrupt_future_row_does_not_print_a_multi_year_countdown() {
        let hist = history(Category::Event, BBQ::BBQ06, 2_069_445_092);
        let out = cooldown_lines(&hist, &Category::Event, &CooldownPolicy::default(), T0);
        assert_eq!(out, "✅ Ready");
    }

    #[test]
    fn event_shows_the_configured_wait_on_both_lines() {
        // Event ignores `same`/`different`, so both lines carry the same time.
        let hist = history(Category::Event, BBQ::BBQ01, T0);
        let out = cooldown_lines(&hist, &Category::Event, &CooldownPolicy::default(), T0 + HOUR);
        let stamp = format!("<t:{}:R>", T0 + 40 * HOUR);
        assert_eq!(
            out.matches(&stamp).count(),
            2,
            "both lines should use the 40 hour default: {out}"
        );
    }

    #[test]
    fn the_event_wait_follows_config_not_the_default() {
        let configured = CooldownPolicy {
            event: 6,
            ..CooldownPolicy::default()
        };
        let hist = history(Category::Event, BBQ::BBQ01, T0);
        let out = cooldown_lines(&hist, &Category::Event, &configured, T0 + HOUR);
        assert!(out.contains(&format!("<t:{}:R>", T0 + 6 * HOUR)), "got {out}");
        assert!(!out.contains(&format!("<t:{}:R>", T0 + 40 * HOUR)), "got {out}");
    }

    #[test]
    fn a_sql_planted_cooldown_reaches_the_card() {
        // The card folds the legacy column in before rendering, so an operator's
        // UPDATE must be visible to the player.
        let hist = crate::reusable::component::bounty_cooldown::history_with_legacy(&[], "2_7", T0);
        let out = cooldown_lines(&hist, &Category::Bronze, &CooldownPolicy::default(), T0 + HOUR);
        assert!(out.contains("Different Bounty CD"), "got {out}");
        assert!(out.contains("Same Bounty CD"), "got {out}");
        assert!(out.contains(&format!("<t:{}:R>", T0 + 20 * HOUR)), "got {out}");
        assert!(out.contains(&format!("<t:{}:R>", T0 + 40 * HOUR)), "got {out}");
    }
}