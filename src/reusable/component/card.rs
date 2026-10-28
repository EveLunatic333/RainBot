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

    fn embed(&self, user: &User) -> Result<CreateEmbed, MyErr> {
        let time = self.latest_bounty_time + 20 * 60 * 60;
        let time2 = self.latest_bounty_time + 40 * 60 * 60;
        let now = MyTime::now();

        let cd = |t: i64| {
            if t >= now { format!("<t:{}:R>", t) } else { "You can do it now".to_string() }
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

        let event_desc = format!(
            "🕜 **Latest Bounty:** {latest}\n🕒 **Time Completed:** {time_completed}\n🍽️ **Restaurant Buff:** {}\n\n👨‍🌾 **Different Bounty CD:** {}\n👩‍🌾 **Same Bounty CD:** {}\n\n🥉 **Bronze:** {}\n🥈 **Silver:** {}\n🥇 **Gold:** {}",
            meal_status, cd(time), cd(time2),
            BBQ::name_encode(self.bronze as u8),
            BBQ::name_encode(self.silver as u8),
            BBQ::name_encode(self.gold as u8)
        );

        Ok(CreateEmbed::new()
            .author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
            .title("Event & Logistics Card")
            .field("Economy Status", eco_desc, false)
            .field("Activity & Buffs", event_desc, false)
            .color(Color::Green.throw()))
    }

    pub async fn response<T: Mybundle>(&self, user: &User, bnd: &T) -> Result<(), MyErr> {
        Ok(Components::response_adv(
            bnd,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(self.embed(user)?),
            ),
        ).await?)
    }
}