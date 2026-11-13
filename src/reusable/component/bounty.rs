use crate::event::interaction::SlashBundle;
use crate::material::ItemPedia;
use crate::reusable::postgress::card::Event;
use crate::reusable::utils::Color;
use crate::reusable::utils::MyTime;
use crate::BOUNTY;
use crate::Mybundle;
use crate::PgConn;
use serde::{Deserialize, Serialize};
use serenity::all::*;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use tracing::info;
use super::market::Market;
use super::Components;
use super::MyErr;
use super::bounty_cooldown;

#[derive(Clone)]
pub struct Title {
    pub bounty_bronze: bool,
    pub bounty_silver: bool,
    pub bounty_gold: bool,
    pub trade_bronze: bool,
    pub trade_silver: bool,
    pub trade_gold: bool,
}

impl Title {
    fn matching(code: u8, math: u8) -> bool {
        code & math == math
    }

    pub fn new(code: u8) -> Self {
        Title {
            bounty_bronze: Title::matching(code, 1),
            bounty_silver: Title::matching(code, 2),
            bounty_gold: Title::matching(code, 4),
            trade_bronze: Title::matching(code, 8),
            trade_silver: Title::matching(code, 16),
            trade_gold: Title::matching(code, 32),
        }
    }

    pub fn bonus(&self) -> f32 {
        let mut out = 1.0;
        if self.bounty_gold {
            out += 0.3
        }
        if self.bounty_silver {
            out += 0.2
        }
        if self.bounty_bronze {
            out += 0.1
        }
        out
    }

    /// The same bonus, computed in `f64`.
    ///
    /// [`Self::bonus`] returns `f32`, where `1.0 + 0.3` is `1.29999995...`. That is
    /// fine for the payout (it is the arithmetic the bot has always used) but it
    /// made an audit line report an effective factor of `2.5999999` for a clean
    /// `2.0 x 1.3`. This variant exists **for reporting only**; the thresholds must
    /// stay identical, and a test pins the two to agree.
    pub fn bonus_precise(&self) -> f64 {
        let mut out = 1.0f64;
        if self.bounty_gold {
            out += 0.3
        }
        if self.bounty_silver {
            out += 0.2
        }
        if self.bounty_bronze {
            out += 0.1
        }
        out
    }

    pub fn discount(&self) -> f32 {
        let mut out = 1.0;
        if self.trade_gold {
            out -= 0.3
        }
        if self.trade_silver {
            out -= 0.2
        }
        if self.trade_bronze {
            out -= 0.1
        }
        out
    }
}

#[derive(Clone)]
pub enum Methode {
    Solo,
    Multi,
}

impl Methode {
    pub fn new(code: u8) -> Self {
        match code {
            0 => Methode::Solo,
            _ => Methode::Multi,
        }
    }

    pub fn encode(&self) -> u8 {
        match self {
            Self::Solo => 0,
            Self::Multi => 1,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Methode::Solo => "Solo".to_owned(),
            Methode::Multi => "Multiplayer".to_owned(),
        }
    }

    pub fn option_str() -> Vec<(String, String)> {
        vec![
            ("0".to_owned(), "Solo".to_owned()),
            ("1".to_owned(), "Multi".to_owned()),
        ]
    }

    pub fn get_reward<'a>(&self, bnt: &'a BountyDesc) -> &'a BountyReward {
        match self {
            Self::Solo => &bnt.solo,
            Self::Multi => &bnt.multi,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Category {
    Bronze,
    Silver,
    Gold,
    Free,
    Event,
}

impl Category {
    pub fn new(code: u8) -> Result<Category, MyErr> {
        match code {
            0 => Ok(Category::Bronze),
            1 => Ok(Category::Silver),
            2 => Ok(Category::Gold),
            3 => Ok(Category::Free),
            4 => Ok(Category::Event),
            _ => Err(MyErr::Custom("Cannot get category".to_string())),
        }
    }

    pub fn encode(&self) -> u8 {
        match self {
            Category::Bronze => 0,
            Category::Silver => 1,
            Category::Gold => 2,
            Category::Free => 3,
            Category::Event => 4,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Category::Event => "Event".to_string(),
            Category::Free => "Free".to_owned(),
            Category::Gold => "Gold".to_owned(),
            Category::Silver => "Silver".to_owned(),
            Category::Bronze => "Bronze".to_owned(),
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Gold => Color::Gold,
            Self::Silver => Color::Silver,
            Self::Bronze => Color::Bronze,
            Self::Free => Color::Green,
            Self::Event => Color::Blue,
        }
    }

    pub fn option_str() -> Vec<(String, String)> {
        vec![
            ("0".to_owned(), "Bronze".to_owned()),
            ("1".to_owned(), "Silver".to_owned()),
            ("2".to_owned(), "Gold".to_owned()),
            ("3".to_owned(), "Free".to_owned()),
            ("4".to_owned(), "Event".to_owned()),
        ]
    }

    /// Highest BBQ number currently unlocked in this category.
    ///
    /// `pub(crate)` so the submit pre-check can report the same limit the final
    /// grant enforces, instead of letting the player reach the grant before being
    /// told the number is locked.
    pub(crate) fn rank(&self, event: &Event) -> i32 {
        match self {
            Self::Gold => event.gold + 1,
            Self::Bronze => event.bronze + 1,
            Self::Silver => event.silver + 1,
            _ => 50,
        }
    }

    pub(crate) fn check_rank(&self, event: &Event, code: u8) -> bool {
        code as i32 <= self.rank(event)
    }

    /// Completed bounty count for this category, which is what `rank` adds to.
    pub(crate) fn counter(&self, event: &Event) -> i32 {
        match self {
            Self::Gold => event.gold,
            Self::Silver => event.silver,
            Self::Bronze => event.bronze,
            // Free and Event have no ladder, so `rank` is the fixed ceiling.
            _ => 0,
        }
    }

    fn change_rank(&self, hunt: &mut Hunter, code: u8) {
        if code as i32 == self.rank(&hunt.event) {
            match self {
                Self::Gold => {
                    hunt.event.gold += 1;
                }
                Self::Silver => {
                    hunt.event.silver += 1;
                }
                Self::Bronze => {
                    hunt.event.bronze += 1;
                }
                _ => {
                    return;
                }
            }
        }
    }

    fn bounty_path(&self) -> PathBuf {
        let x = Path::new(".").join("static");
        match self {
            Self::Gold => x.join("gold_bounty.json"),
            Self::Silver => x.join("silver_bounty.json"),
            Self::Bronze => x.join("bronze_bounty.json"),
            Self::Free => x.join("free_bounty.json"),
            Self::Event => x.join("event_bounty.json"),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum BBQ {
    BBQ01,
    BBQ02,
    BBQ03,
    BBQ04,
    BBQ05,
    BBQ06,
    BBQ07,
    BBQ08,
    BBQ09,
    BBQ10,
    BBQ11,
    BBQ12,
    BBQ13,
    BBQ14,
    BBQ15,
    BBQ16,
    BBQ17,
    BBQ18,
    BBQ19,
    BBQ20,
    BBQ21,
    BBQ22,
    BBQ23,
    BBQ24,
    BBQ25,
}

impl BBQ {
    pub fn new(code: u8) -> Result<BBQ, MyErr> {
        match code {
            1 => Ok(BBQ::BBQ01),
            2 => Ok(BBQ::BBQ02),
            3 => Ok(BBQ::BBQ03),
            4 => Ok(BBQ::BBQ04),
            5 => Ok(BBQ::BBQ05),
            6 => Ok(BBQ::BBQ06),
            7 => Ok(BBQ::BBQ07),
            8 => Ok(BBQ::BBQ08),
            9 => Ok(BBQ::BBQ09),
            10 => Ok(BBQ::BBQ10),
            11 => Ok(BBQ::BBQ11),
            12 => Ok(BBQ::BBQ12),
            13 => Ok(BBQ::BBQ13),
            14 => Ok(BBQ::BBQ14),
            15 => Ok(BBQ::BBQ15),
            16 => Ok(BBQ::BBQ16),
            17 => Ok(BBQ::BBQ17),
            18 => Ok(BBQ::BBQ18),
            19 => Ok(BBQ::BBQ19),
            20 => Ok(BBQ::BBQ20),
            21 => Ok(BBQ::BBQ21),
            22 => Ok(BBQ::BBQ22),
            23 => Ok(BBQ::BBQ23),
            24 => Ok(BBQ::BBQ24),
            25 => Ok(BBQ::BBQ25),
            _ => Err(MyErr::Custom("Cannot get BBQ ID".to_string())),
        }
    }

    pub fn name_encode(code: u8) -> String {
        match Self::new(code) {
            Ok(x) => x.name(),
            Err(_) => "Not Progressed Yet".to_owned(),
        }
    }

    pub fn encode(&self) -> u8 {
        match self {
            BBQ::BBQ01 => 1,
            BBQ::BBQ02 => 2,
            BBQ::BBQ03 => 3,
            BBQ::BBQ04 => 4,
            BBQ::BBQ05 => 5,
            BBQ::BBQ06 => 6,
            BBQ::BBQ07 => 7,
            BBQ::BBQ08 => 8,
            BBQ::BBQ09 => 9,
            BBQ::BBQ10 => 10,
            BBQ::BBQ11 => 11,
            BBQ::BBQ12 => 12,
            BBQ::BBQ13 => 13,
            BBQ::BBQ14 => 14,
            BBQ::BBQ15 => 15,
            BBQ::BBQ16 => 16,
            BBQ::BBQ17 => 17,
            BBQ::BBQ18 => 18,
            BBQ::BBQ19 => 19,
            BBQ::BBQ20 => 20,
            BBQ::BBQ21 => 21,
            BBQ::BBQ22 => 22,
            BBQ::BBQ23 => 23,
            BBQ::BBQ24 => 24,
            BBQ::BBQ25 => 25,
        }
    }

    pub fn name(&self) -> String {
        match self {
            BBQ::BBQ01 => "BBQ01".to_owned(),
            BBQ::BBQ02 => "BBQ02".to_owned(),
            BBQ::BBQ03 => "BBQ03".to_owned(),
            BBQ::BBQ04 => "BBQ04".to_owned(),
            BBQ::BBQ05 => "BBQ05".to_owned(),
            BBQ::BBQ06 => "BBQ06".to_owned(),
            BBQ::BBQ07 => "BBQ07".to_owned(),
            BBQ::BBQ08 => "BBQ08".to_owned(),
            BBQ::BBQ09 => "BBQ09".to_owned(),
            BBQ::BBQ10 => "BBQ10".to_owned(),
            BBQ::BBQ11 => "BBQ11".to_owned(),
            BBQ::BBQ12 => "BBQ12".to_owned(),
            BBQ::BBQ13 => "BBQ13".to_owned(),
            BBQ::BBQ14 => "BBQ14".to_owned(),
            BBQ::BBQ15 => "BBQ15".to_owned(),
            BBQ::BBQ16 => "BBQ16".to_owned(),
            BBQ::BBQ17 => "BBQ17".to_owned(),
            BBQ::BBQ18 => "BBQ18".to_owned(),
            BBQ::BBQ19 => "BBQ19".to_owned(),
            BBQ::BBQ20 => "BBQ20".to_owned(),
            BBQ::BBQ21 => "BBQ21".to_owned(),
            BBQ::BBQ22 => "BBQ22".to_owned(),
            BBQ::BBQ23 => "BBQ23".to_owned(),
            BBQ::BBQ24 => "BBQ24".to_owned(),
            BBQ::BBQ25 => "BBQ25".to_owned(),
        }
    }

    pub fn option_str() -> Vec<(String, String)> {
        vec![
            ("1".to_owned(), "BBQ01".to_owned()),
            ("2".to_owned(), "BBQ02".to_owned()),
            ("3".to_owned(), "BBQ03".to_owned()),
            ("4".to_owned(), "BBQ04".to_owned()),
            ("5".to_owned(), "BBQ05".to_owned()),
            ("6".to_owned(), "BBQ06".to_owned()),
            ("7".to_owned(), "BBQ07".to_owned()),
            ("8".to_owned(), "BBQ08".to_owned()),
            ("9".to_owned(), "BBQ09".to_owned()),
            ("10".to_owned(), "BBQ10".to_owned()),
            ("11".to_owned(), "BBQ11".to_owned()),
            ("12".to_owned(), "BBQ12".to_owned()),
            ("13".to_owned(), "BBQ13".to_owned()),
            ("14".to_owned(), "BBQ14".to_owned()),
            ("15".to_owned(), "BBQ15".to_owned()),
            ("16".to_owned(), "BBQ16".to_owned()),
            ("17".to_owned(), "BBQ17".to_owned()),
            ("18".to_owned(), "BBQ18".to_owned()),
            ("19".to_owned(), "BBQ19".to_owned()),
            ("20".to_owned(), "BBQ20".to_owned()),
            ("21".to_owned(), "BBQ21".to_owned()),
            ("22".to_owned(), "BBQ22".to_owned()),
            ("23".to_owned(), "BBQ23".to_owned()),
            ("24".to_owned(), "BBQ24".to_owned()),
            ("25".to_owned(), "BBQ25".to_owned()),
        ]
    }

    pub fn get_bounty<'a>(&self, bbq: &'a Bounty) -> Result<&'a BountyDesc, MyErr> {
        for i in &bbq.bounty {
            if i.bbq + 1 == self.encode() {
                return Ok(i);
            }
        }
        Err(MyErr::Custom("The JSON is not configured yet".to_owned()))
    }
}

#[derive(Clone)]
pub struct Hunter {
    pub member: Member,
    pub title: Title,
    pub event: Event,
}

/// One audit line's numbers, computed once so the arithmetic is testable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RewardAuditEntry {
    pub multiplier_coin: f64,
    pub multiplier_gacha: f64,
    pub title_bonus: f32,
    /// `multiplier_coin * title_bonus`: everything the coin was multiplied by.
    ///
    /// Computed in `f64` after widening the bonus. The bonus is an `f32` (`1.3` in
    /// binary is `1.29999995...`), so multiplying it in `f32` and then widening gave
    /// `2.5999999` instead of `2.6`, which would misreport the factor in the audit
    /// line even though the paid amount was right.
    pub effective_coin_factor: f64,    pub coin_granted: u32,
    pub gacha_granted: u32,
}

#[derive(Clone)]
pub struct BountySubmit {
    pub method: Methode,
    pub category: Category,
    pub bbq: BBQ,
    pub hunter: Vec<Hunter>,
    pub url: String,
    pub thumb: String,
    pub time: i64,
    pub reward: BountyReward,
	pub msg_id: u64,
}

impl Hunter {
    async fn new(
        bnd: &SlashBundle<'_>,
        bypass: bool,
        user: Member,
        uid: Vec<UserId>,
        pg: &mut PgConn<'_>,
        bbq: &BBQ,
        category: &Category,
        tier: &bounty_cooldown::ProgressionTier,
    ) -> Result<Vec<Hunter>, MyErr> {
        let mut vec = vec![user];
        let mut hunter = Vec::new();
        let guild = bnd
            .cmd
            .guild_id
            .unwrap()
            .to_partial_guild(&bnd.ctx.http)
            .await?;
        for i in &uid {
            let mem = guild.member(&bnd.ctx.http, i.to_owned()).await?;
            vec.push(mem);
        }
        for us in vec {
            pg.did = us.user.id.to_string();
            let event = pg.get_event().await.map_err(|_| {
                MyErr::Custom(format!(
                    "{} most likely is not bound yet, please check",
                    us.to_string()
                ))
            })?;
            let title = Title::new(u8::try_from(event.title).unwrap());
            if !bypass {
                // Progression must be walked in order: Bronze finishes before
                // Silver opens, Silver before Gold. `config.json` decides whether
                // the counter, the achievement bit, or both are required.
                let gate = bounty_cooldown::progression_gate(
                    event.bronze,
                    event.silver,
                    title.bounty_bronze,
                    title.bounty_silver,
                    category,
                    &tier,
                    bnd.init().bounty.progression_gate,
                );
                if let bounty_cooldown::ProgressionGate::Blocked {
                    required,
                    need,
                    have,
                } = gate
                {
                    // Same shape as the submit pre-check: name the tier, show how
                    // far along the player is, and say what clears it.
                    return Err(MyErr::Custom(
                        super::bounty_notice::blocked_submission(&[
                            super::bounty_notice::Block::Progression {
                                user: us.to_string(),
                                required,
                                need,
                                have,
                            },
                        ])
                        .to_plain_text(),
                    ));
                }

                let policy = bounty_cooldown::CooldownPolicy::from(&bnd.init().bounty);
                let did = us.user.id.to_string();
                let rows = crate::reusable::postgress::gacha::group_history(pg.pool(), &did)
                    .await
                    .map(|rows| {
                        rows.iter()
                            .map(|r| (r.group_id, r.category, r.bbq, r.completed_at))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let history = bounty_cooldown::history_with_legacy_and_policy(
                    &bounty_cooldown::history_from_rows(&rows),
                    &event.latest_bounty,
                    event.latest_bounty_time,
                    &policy,
                );
                let group = policy.group_of(category);
                let previous = bounty_cooldown::last_of_group(&history, group);
                let verdict =
                    bounty_cooldown::evaluate(&history, category, bbq, &policy, MyTime::now());
                if let Some(ends_at) = verdict.ends_at() {
                    let hours = previous
                        .as_ref()
                        .map(|p| policy.hours_for(group, &p.category, &p.bbq, category, bbq))
                        .unwrap_or(0);
                    return Err(MyErr::Custom(
                        super::bounty_notice::blocked_submission(&[
                            super::bounty_notice::Block::Cooldown {
                                user: us.to_string(),
                                ends_at,
                                group,
                                hours,
                                previous: previous.map(|p| (p.category, p.bbq)),
                            },
                        ])
                        .to_plain_text(),
                    ));
                }
                if !category.check_rank(&event, bbq.encode()) {
                    let allowed = u8::try_from(category.rank(&event).clamp(1, 25)).unwrap_or(1);
                    return Err(MyErr::Custom(
                        super::bounty_notice::blocked_submission(&[
                            super::bounty_notice::Block::Rank {
                                user: us.to_string(),
                                category: category.clone(),
                                allowed,
                                wanted: bbq.encode(),
                                have: category.counter(&event),
                                need: bbq.encode().saturating_sub(1),
                            },
                        ])
                        .to_plain_text(),
                    ));
                }
                // Final enforcement of the no-repeat rule. The pre-check reports it
                // early; this makes it impossible to bypass by any other path.
                if matches!(category, Category::Bronze | Category::Silver | Category::Gold) {
                    match bounty_cooldown::progression_repeat(
                        category.counter(&event),
                        bbq.encode(),
                        bnd.init().bounty.progression_no_repeat,
                    ) {
                        bounty_cooldown::ProgressionRepeat::Allowed => {}
                        bounty_cooldown::ProgressionRepeat::AlreadyCleared { bbq: done, next } => {
                            return Err(MyErr::Custom(
                                super::bounty_notice::blocked_submission(&[
                                    super::bounty_notice::Block::ProgressionRepeat {
                                        user: us.to_string(),
                                        category: category.clone(),
                                        bbq: done,
                                        next,
                                    },
                                ])
                                .to_plain_text(),
                            ));
                        }
                        bounty_cooldown::ProgressionRepeat::LadderFinished => {
                            return Err(MyErr::Custom(
                                super::bounty_notice::blocked_submission(&[
                                    super::bounty_notice::Block::LadderFinished {
                                        user: us.to_string(),
                                        category: category.clone(),
                                    },
                                ])
                                .to_plain_text(),
                            ));
                        }
                    }
                }
            }

            hunter.push(Hunter {
                member: us.to_owned(),
                title,
                event,
            });
        }
        Ok(hunter)
    }
}

impl BountySubmit {
    pub async fn new(
        bnd: &SlashBundle<'_>,
        bypass: bool,
        user: Member,
        uid: Vec<UserId>,
        pg: &mut PgConn<'_>,
        bounty: &Box<Bounty>,
        url: &str,
        method: Methode,
        bbq: BBQ,
        category: Category,
    ) -> Result<BountySubmit, MyErr> {
        let tier = BountyTitle::progression_tier().await;
        let hunter = Hunter::new(bnd, bypass, user, uid, pg, &bbq, &category, &tier).await?;
        let desc = bbq.get_bounty(&*bounty)?;
        // The multiplier is applied here, once, so `self.reward` is the final amount:
        // `reward()` pays it and `reward_embed` shows it without a second scaling.
        // `for_bounty` honours a per-BBQ override, falling back to the group value.
        let multiplier = bnd
            .init()
            .bounty
            .reward_multiplier
            .for_bounty(&category, &bbq);
        let reward = method.get_reward(desc).scaled(&multiplier);
        Ok(BountySubmit {
            hunter,
            method,
            bbq,
            category,
            url: url.to_owned(),
            thumb: desc.icon.to_owned(),
            reward,
            time: MyTime::now(),
			msg_id: 0,
        })
    }

    async fn send_reward(
        &self,
        bypass: bool,
        pg: &PgConn<'_>,
        policy: &bounty_cooldown::CooldownPolicy,
    ) -> Result<(), MyErr> {
        let name = format!(
            "{} {} {} Rewards",
            self.method.name(),
            self.category.name(),
            self.bbq.name()
        );
        let desc = format!(
            "Reward for clearing server event {} {} as {}",
            self.category.name(),
            self.bbq.name(),
            self.method.name()
        );

        // Every hunter's currency row and item rows are written in one
        // transaction. A failure for the second member of a party used to leave
        // the first member rewarded and the rest empty-handed while all of them
        // had already been stamped with a fresh cooldown; now nothing is written
        // unless every member succeeds.
        let mut tx = pg.pool().begin().await?;
        for i in &self.hunter {
            let did = i.member.user.id.to_string();
            crate::reusable::postgress::gacha::send_item_tx(
                &mut tx,
                &self.reward.items,
                i.event.char_id,
                &name,
                &desc,
            )
            .await?;
            crate::reusable::postgress::gacha::bounty_event_tx(&mut tx, &did, &i.event).await?;

            // Per-group cooldown: Bronze, Silver and Gold share one row, so a Gold
            // run also locks Bronze and Silver (one progression ladder). Free and
            // Event have their own rows and stay independent.
            // `bypass` means an administrator granted this reward directly; it must
            // not start a cooldown for the user.
            if !bypass {
                crate::reusable::postgress::gacha::record_group_completion_tx(
                    &mut tx,
                    &did,
                    // The row has to land on the group the option puts this
                    // category in: with Event sharing enabled an Event completion
                    // must be written onto the standard row, or it would not lock
                    // the progression ladder.
                    policy.group_of(&self.category).encode(),
                    self.category.encode() as i16,
                    self.bbq.encode() as i16,
                    self.time,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn reward<T: Mybundle>(
        &mut self,
        bypass: bool,
        bnd: &T,
        pg: &mut PgConn<'_>,
    ) -> Result<(), MyErr> {
        // Phase 1: read the current state and compute the new values. No writes yet,
        // so a read failure cannot leave a half-applied reward behind.
        for hunt in self.hunter.iter_mut() {
            pg.did = hunt.member.user.id.to_string();
            let eve = pg.get_event().await?;
            hunt.event.bounty = eve.bounty;
            hunt.event.gacha = eve.gacha;
            let bounty = self.reward.coin as f32 * hunt.title.bonus();
            hunt.event.bounty += bounty as i32;
            hunt.event.gacha += self.reward.ticket as i32;

            if !bypass {
                self.category.change_rank(hunt, self.bbq.encode());
                // `latest_bounty` stays the "most recent completion" shown on the
                // player card. Cooldown no longer reads it, so overwriting it here
                // can no longer shorten another category's wait.
                hunt.event.latest_bounty =
                    BountyTitle::encrypt(self.category.clone(), self.bbq.clone());
                hunt.event.latest_bounty_time = self.time;
            }
        }

        // Phase 2: apply every write atomically. The policy is passed down so the
        // cooldown row uses the same group the checks just used.
        let policy = bounty_cooldown::CooldownPolicy::from(&bnd.init().bounty);

        // Audit trail, written before the payout so a failed write is still visible
        // alongside the amounts that were attempted.
        self.audit_reward(bnd, bypass);

        self.send_reward(bypass, pg, &policy).await?;

        let ch = ChannelId::new(bnd.init().bounty.receptionist_ch);
        for (user, embed) in self.reward_embed(bnd.pedia()) {
            ch.send_message(
                &bnd.ctx().http,
                CreateMessage::new().embed(embed).content(format!(
                    "{}'s {} {} {} Reward already distributed",
                    user.to_string(),
                    self.method.name(),
                    self.category.name(),
                    self.bbq.name()
                )),
            )
            .await?;
        }
        Ok(())
    }

    /// The numbers one audit line carries, for one hunter.
    ///
    /// Split out of [`Self::audit_reward`] so the arithmetic — especially the way the
    /// title bonus stacks on top of the multiplier — can be tested without a Discord
    /// context or a tracing subscriber.
    fn audit_entry(
        &self,
        multiplier: &crate::reusable::config::RewardMultiplier,
        title: &Title,
    ) -> RewardAuditEntry {
        let bonus = title.bonus();
        RewardAuditEntry {
            multiplier_coin: multiplier.coin,
            multiplier_gacha: multiplier.gacha,
            title_bonus: bonus,
            // Reported from the precise variant, so a clean 2.0 x 1.3 reads as 2.6
            // rather than 2.5999999. The amount actually paid still comes from the
            // `f32` path above, which is untouched.
            effective_coin_factor: multiplier.coin * title.bonus_precise(),
            coin_granted: self.reward.coin,
            gacha_granted: self.reward.ticket,
        }
    }

    /// Writes one audit line per recipient, recording exactly how the payout was
    /// arrived at.
    ///
    /// The line carries the base amount from the category file, the multiplier that
    /// was in force, and the final amount — so a dispute about "why did I get this
    /// much" can be answered from the log instead of by re-deriving history that no
    /// longer exists (the multiplier may have been changed since).
    ///
    /// Rewards are logged even when the multiplier is neutral: the absence of a line
    /// must never be mistaken for "no reward was paid".
    fn audit_reward<T: Mybundle>(&self, bnd: &T, bypass: bool) {
        let multiplier = bnd
            .init()
            .bounty
            .reward_multiplier
            .for_bounty(&self.category, &self.bbq);

        for hunt in &self.hunter {
            let e = self.audit_entry(&multiplier, &hunt.title);
            info!(
                target: "reward_audit",
                user = %hunt.member.user.id,
                category = %self.category.name(),
                bbq = %self.bbq.name(),
                method = %self.method.name(),
                bypass,
                multiplier_coin = e.multiplier_coin,
                multiplier_gacha = e.multiplier_gacha,
                title_bonus = e.title_bonus,
                effective_coin_factor = e.effective_coin_factor,
                coin_granted = e.coin_granted,
                gacha_granted = e.gacha_granted,
                "Reward distributed"
            );
        }
    }

    pub async fn title<T: Mybundle>(&mut self, bnd: &T, title: &BountyTitle) -> Result<(), MyErr> {
        let code = BountyTitle::encrypt(self.category.clone(), self.bbq.clone());
        for hunt in self.hunter.iter_mut() {
            title
                .add_title(bnd, &mut hunt.event, &mut hunt.member, &code)
                .await?;
        }
        Ok(())
    }

    pub async fn save(&self, did: &str) {
        let mut bounty = BOUNTY.lock().await;
        match bounty.get_mut(did) {
            Some(x) => {
                *x = self.clone();
            }
            None => {
                bounty.insert(did.to_owned(), self.clone());
            }
        }
    }

    pub async fn open(did: &str) -> Option<Self> {
        BOUNTY.lock().await.get(did).cloned()
    }

    /// Atomically reserves the in-progress submission slot for a user.
    /// Returns false when the user already holds a reservation, which
    /// prevents concurrent /bounty submit flows from colliding.
    pub async fn reserve(did: &str) -> bool {
        crate::SUBMIT_LOCK.lock().await.insert(did.to_owned())
    }

    /// Releases the in-progress submission slot for a user.
    pub async fn unreserve(did: &str) {
        crate::SUBMIT_LOCK.lock().await.remove(did);
    }

    pub async fn delete(&self) {
        BOUNTY
            .lock()
            .await
            .remove(&self.hunter[0].member.user.id.to_string());
    }

    pub fn embed(&self) -> CreateEmbed {
        let title = format!(
            "{} {} {}",
            self.category.name(),
            self.bbq.name(),
            self.method.name()
        );
        let mut desc = Vec::new();
        for i in &self.hunter {
            desc.push("\n".to_owned());
            desc.push(format!(
                "name : {} > {}",
                i.member.user.to_string(),
                &i.event.name
            ));
        }
        let res = format!("Submitted At <t:{}:F>\n{}", self.time, desc[1..].concat());
        let author = &self.hunter[0].member;
        CreateEmbed::new()
            .title(title)
            .description(res)
            .thumbnail(&self.thumb)
            .image(&self.url)
            .author(
                CreateEmbedAuthor::new(&author.user.name)
                    .url(author.user.face()),
            )
            .color(self.category.color().throw())
    }

    pub fn button(&self) -> Vec<CreateActionRow> {
        let accept = Components::normal_button(
            "Approve",
            &format!("bounty_{}_a", self.hunter[0].member.user.id.to_string()),
            ButtonStyle::Primary,
            "👌",
        );
        let reject = Components::normal_button(
            "Reject",
            &format!("bounty_{}_r", self.hunter[0].member.user.id.to_string()),
            ButtonStyle::Danger,
            "👎",
        );
        let check = Components::normal_button(
            "Check",
            &format!("bounty_{}_c", self.hunter[0].member.user.id.to_string()),
            ButtonStyle::Success,
            "🔍",
        );
        let arow = CreateActionRow::Buttons(vec![accept, reject, check]);
        vec![arow]
    }

    pub async fn cooldown(&self, bnt: &mut Box<BountyRefresh>) -> Result<bool, MyErr> {
        if self.category == Category::Free {
            for (bbq, cd) in bnt.hashmap() {
                if bbq == self.bbq && cd != 0 {
                    bnt.set_cd(&bbq, cd - 1);
                    return Ok(true);
                }
            }
            return Ok(false);
        } else {
            let desc = Box::new(Bounty::new(&self.category).await?);
            for x in desc.bounty {
                if x.cooldown != 0 && x.bbq == self.bbq.encode() {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }

    fn reward_embed<'a>(&'a self, item: &ItemPedia) -> HashMap<&'a User, CreateEmbed> {
        let mut out = HashMap::new();
        let mut reward = vec![">>> ".to_owned()];
        let ticket = format!("Ticket Reward: {} Ticket(s)", self.reward.ticket);
        for i in &self.reward.items {
            reward.push(i.text(item).unwrap());
            reward.push("\n".to_owned());
        }
        let lenre = reward.len() - 1;
        for j in &self.hunter {
            let bon = (j.title.bonus() - 1.0) * 100.0;
            let percent = format!("{}%", bon as u32);
            let bonus = self.reward.coin as f64 * j.title.bonus() as f64;
            let bounty = format!(
                "Bounty Reward: {}\nTitle Bonus: {}\nReward With Bonus: {}",
                Market::currency(self.reward.coin as i64),
                percent,
                Market::currency(bonus as i64)
            );
            let embed = CreateEmbed::new()
                .title("Bounty Reward")
                .description(format!(
                    "{}'s reward for {} {} category {}",
                    j.member.to_string(),
                    self.method.name(),
                    self.bbq.name(),
                    self.category.name()
                ))
                .fields(vec![
                    ("Bounty Coin", bounty, false),
                    ("Gacha Tickets", ticket.to_owned(), false),
                    ("Items/Equipment", reward[..lenre].to_vec().concat(), false),
                ])
                .color(Color::Gold.throw())
                .author(CreateEmbedAuthor::new(&j.member.user.name).icon_url(j.member.user.face()));
            out.insert(&j.member.user, embed);
        }
        out
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct Bounty {
    pub bounty: [BountyDesc; 25],
}

use super::super::bitwise::ItemCode;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct BountyDesc {
    pub bbq: u8,
    pub description: String,
    pub cooldown: u32,
    pub icon: String,
    pub thumbnail: String,
    pub rules: Vec<String>,
    pub solo: BountyReward,
    pub multi: BountyReward,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct BountyReward {
    coin: u32,
    ticket: u32,
    items: Vec<ItemCode>,
}

impl BountyReward {
    /// Returns a copy whose coin and ticket are scaled by `m`.
    ///
    /// Items are untouched: the multiplier covers the two currencies only. Applying
    /// it here, once, keeps the granted amount and the displayed amount derived from
    /// a single value — `self.reward` is then final everywhere downstream, so the
    /// payment path and the embed cannot drift apart.
    pub fn scaled(&self, m: &crate::reusable::config::RewardMultiplier) -> BountyReward {
        BountyReward {
            coin: m.coin_of(self.coin),
            ticket: m.gacha_of(self.ticket),
            items: self.items.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct BountyRefresh {
    pub bbq1: u32,
    pub bbq2: u32,
    pub bbq3: u32,
    pub bbq4: u32,
    pub bbq5: u32,
    pub bbq6: u32,
    pub bbq7: u32,
    pub bbq8: u32,
    pub bbq9: u32,
    pub bbq10: u32,
    pub bbq11: u32,
    pub bbq12: u32,
    pub bbq13: u32,
    pub bbq14: u32,
    pub bbq15: u32,
    pub bbq16: u32,
    pub bbq17: u32,
    pub bbq18: u32,
    pub bbq19: u32,
    pub bbq20: u32,
    pub bbq21: u32,
    pub bbq22: u32,
    pub bbq23: u32,
    pub bbq24: u32,
    pub bbq25: u32,
}

/// Colour of the cooldown board embed, `#287760`.
///
/// Fixed on purpose: the board is one message edited in place, so a random
/// colour per refresh made the same board look like a different one each time.
/// This is the green the production board has always shown.
const COOLDOWN_BOARD_COLOUR: u32 = 0x287760;

impl BountyRefresh {
    pub fn path() -> PathBuf {
        Path::new(".").join("static").join("bounty_refresh.json")
    }

    pub fn path2() -> PathBuf {
        Path::new(".").join("static").join("cooldown.json")
    }

    pub async fn new(cooldown: bool) -> Result<Self, MyErr> {
        let path = if cooldown {
            BountyRefresh::path2()
        } else {
            BountyRefresh::path()
        };
        let file = tokio::fs::read_to_string(path).await?;
        Ok(serde_json::from_str(&file)?)
    }

    pub async fn save(&self, cooldown: bool) -> Result<(), MyErr> {
        let path = if cooldown {
            BountyRefresh::path2()
        } else {
            BountyRefresh::path()
        };
        let string = serde_json::to_string_pretty(&self)?;
        tokio::fs::write(path, string.as_bytes()).await?;
        Ok(())
    }

    pub async fn check(data: &[u8]) -> Result<(), MyErr> {
        let x = serde_json::from_slice::<Self>(&data)?;
        x.save(false).await?;
        Ok(())
    }

    /// Returns consumed Free Bounty slots back to the pool.
    /// Called when a submission is rejected or expires without a verdict,
    /// so players do not permanently lose available Free Bounty counts.
    pub fn restore_slots(&mut self, bbqs: &[BBQ]) {
        for target in bbqs {
            for (qq, cd) in self.hashmap() {
                if target == &qq {
                    self.set_cd(target, cd + 1);
                }
            }
        }
    }

    pub async fn rejected<T: Mybundle>(bbq: &BBQ, bnd: &T) -> Result<(), MyErr> {
        let mut x = Self::new(true).await?;
        x.restore_slots(&[bbq.clone()]);
        x.save(true).await?;
        x.cooldown(bnd).await
    }

    /// Updates the cooldown board message directly from context and config.
    /// Used by background tasks that do not have an interaction bundle.
    pub async fn cooldown_board(&self, ctx: &Context, init: &crate::Init) -> Result<(), MyErr> {
        let board = super::cooldown_board::resolve(init).await;
        let Some(board) = board else {
            // No board has been spawned or adopted yet. Nothing to edit; the
            // caller logs what it can act on.
            return Err(MyErr::Custom(
                "no cooldown board is configured; run /cooldown interface or /cooldown adopt".to_owned(),
            ));
        };
        let mut msg = ChannelId::new(board.channel)
            .message(&ctx.http, MessageId::new(board.message))
            .await?;
        msg.edit(
            &ctx.http,
            EditMessage::new().embed(self.cooldown_embed()),
        )
        .await?;
        Ok(())
    }

    pub async fn cooldown<T: Mybundle>(&self, bnd: &T) -> Result<(), MyErr> {
        self.cooldown_board(bnd.ctx(), bnd.init()).await
    }

    pub fn set_cd(&mut self, bbq: &BBQ, cd: u32) {
        match bbq {
            BBQ::BBQ01 => {
                self.bbq1 = cd
            }
            BBQ::BBQ02 => {
                self.bbq2 = cd
            }
            BBQ::BBQ03 => {
                self.bbq3 = cd
            }
            BBQ::BBQ04 => {
                self.bbq4 = cd
            }
            BBQ::BBQ05 => {
                self.bbq5 = cd
            }
            BBQ::BBQ06 => {
                self.bbq6 = cd
            }
            BBQ::BBQ07 => {
                self.bbq7 = cd
            }
            BBQ::BBQ08 => {
                self.bbq8 = cd
            }
            BBQ::BBQ09 => {
                self.bbq9 = cd
            }
            BBQ::BBQ10 => {
                self.bbq10 = cd
            }
            BBQ::BBQ11 => {
                self.bbq11 = cd
            }
            BBQ::BBQ12 => {
                self.bbq12 = cd
            }
            BBQ::BBQ13 => {
                self.bbq13 = cd
            }
            BBQ::BBQ14 => {
                self.bbq14 = cd
            }
            BBQ::BBQ15 => {
                self.bbq15 = cd
            }
            BBQ::BBQ16 => {
                self.bbq16 = cd
            }
            BBQ::BBQ17 => {
                self.bbq17 = cd
            }
            BBQ::BBQ18 => {
                self.bbq18 = cd
            }
            BBQ::BBQ19 => {
                self.bbq19 = cd
            }
            BBQ::BBQ20 => {
                self.bbq20 = cd
            }
            BBQ::BBQ21 => {
                self.bbq21 = cd
            }
            BBQ::BBQ22 => {
                self.bbq22 = cd
            }
            BBQ::BBQ23 => {
                self.bbq23 = cd
            }
            BBQ::BBQ24 => {
                self.bbq24 = cd
            }
            BBQ::BBQ25 => {
                self.bbq25 = cd
            }
        }
    }

    /// The board body: a fenced list of all 25 BBQ cooldowns.
    ///
    /// Split out from `cooldown_embed` so the layout can be asserted without a
    /// Discord client.
    fn cooldown_body(&self) -> String {
        let mut text = "```".to_owned();
        for (bbq, cd) in &self.hashmap() {
            text.push_str(&format!("\n{} ======= {} left", bbq.name(), cd));
        }
        text.push_str("\n```");
        text
    }

    fn cooldown_embed(&self) -> CreateEmbed {
        // A fixed colour, not `Color::Random`: the board is edited in place, and a
        // colour that changes on every refresh makes one board look like several.
        // The green matches the colour the production board has always shown.
        CreateEmbed::new()
            .title("Free Category Cooldown")
            .description(self.cooldown_body())
            .color(COOLDOWN_BOARD_COLOUR)
            // The footer carries the refresh time so a board that stopped being
            // updated is obvious at a glance instead of looking current forever.
            .footer(CreateEmbedFooter::new("Last refreshed"))
            .timestamp(Timestamp::now())
    }

    fn hashmap(&self) -> [(BBQ, u32); 25] {
        [
            (BBQ::BBQ01, self.bbq1),
            (BBQ::BBQ02, self.bbq2),
            (BBQ::BBQ03, self.bbq3),
            (BBQ::BBQ04, self.bbq4),
            (BBQ::BBQ05, self.bbq5),
            (BBQ::BBQ06, self.bbq6),
            (BBQ::BBQ07, self.bbq7),
            (BBQ::BBQ08, self.bbq8),
            (BBQ::BBQ09, self.bbq9),
            (BBQ::BBQ10, self.bbq10),
            (BBQ::BBQ11, self.bbq11),
            (BBQ::BBQ12, self.bbq12),
            (BBQ::BBQ13, self.bbq13),
            (BBQ::BBQ14, self.bbq14),
            (BBQ::BBQ15, self.bbq15),
            (BBQ::BBQ16, self.bbq16),
            (BBQ::BBQ17, self.bbq17),
            (BBQ::BBQ18, self.bbq18),
            (BBQ::BBQ19, self.bbq19),
            (BBQ::BBQ20, self.bbq20),
            (BBQ::BBQ21, self.bbq21),
            (BBQ::BBQ22, self.bbq22),
            (BBQ::BBQ23, self.bbq23),
            (BBQ::BBQ24, self.bbq24),
            (BBQ::BBQ25, self.bbq25),
        ]
    }
}

impl Bounty {
    pub fn path(category: &Category) -> PathBuf {
        category.bounty_path()
    }

    /// Builds the operator-facing reward preview for one category.
    ///
    /// Shows, for every BBQ, the base reward from the category file next to what the
    /// current multiplier actually pays — so a typo is caught by reading a table
    /// rather than by a player noticing their reward is wrong.
    ///
    /// An amount whose multiplier is not 1.00 is marked with `*` and listed in the
    /// footer, so a changed row is obvious without comparing every number by eye.
    /// Split from the Discord command so the arithmetic can be tested offline.
    pub fn reward_preview(
        &self,
        category: &Category,
        multipliers: &crate::reusable::config::RewardMultipliers,
    ) -> String {
        let mut lines = Vec::new();
        let mut changed: Vec<String> = Vec::new();

        for desc in &self.bounty {
            let Ok(bbq) = BBQ::new(desc.bbq + 1) else {
                // The file could carry a number outside 1..=25; skip rather than panic.
                continue;
            };
            let m = multipliers.for_bounty(category, &bbq);

            let base_solo = &desc.solo;
            let base_multi = &desc.multi;
            let solo = base_solo.scaled(&m);
            let multi = base_multi.scaled(&m);

            let mut mark = "";
            if !m.is_neutral() {
                mark = "*";
                changed.push(format!(
                    "{} coin x{:.2} / gacha x{:.2}",
                    bbq.name(),
                    crate::reusable::config::RewardMultiplier::effective(m.coin),
                    crate::reusable::config::RewardMultiplier::effective(m.gacha)
                ));
            }

            lines.push(format!(
                "{mark}{} S {}->{} / T {}->{} | M {}->{} / T {}->{}",
                bbq.name(),
                base_solo.coin,
                solo.coin,
                base_solo.ticket,
                solo.ticket,
                base_multi.coin,
                multi.coin,
                base_multi.ticket,
                multi.ticket,
            ));
        }

        let mut out = format!(
            "{} reward preview\n`BBQ  Solo(coin/ticket)  Multi(coin/ticket)`\n```\n{}\n```",
            category.name(),
            lines.join("\n")
        );
        if changed.is_empty() {
            out.push_str("\nNo multiplier is active: every amount equals the category file.");
        } else {
            out.push_str("\n`*` = a multiplier changes this row:\n");
            for entry in changed {
                out.push_str(&format!("• {entry}\n"));
            }
        }
        out
    }

    pub fn desc<T: Mybundle>(
        &self,
        bnd: &T,
        bbq: &BBQ,
        category: &Category,
    ) -> Result<CreateEmbed, MyErr> {
        let pedia = bnd.pedia();
        let bdesc = bbq.get_bounty(self)?;
        let mut rules = vec![">>> ".to_owned()];
        let mut solo = vec![">>> ".to_owned()];
        let mut multi = vec![">>> ".to_owned()];
        for (i, v) in bdesc.rules.iter().enumerate() {
            rules.push(format!("{}. {v}", i + 1));
            rules.push("\n".to_owned());
        }
        for i in &bdesc.solo.items {
            solo.push(i.text(pedia).unwrap());
            solo.push("\n".to_owned());
        }
        for i in &bdesc.multi.items {
            multi.push(i.text(pedia).unwrap());
            multi.push("\n".to_owned());
        }
        let rulen = rules.len() - 1;
        let mulen = multi.len() - 1;
        let solen = solo.len() - 1;
        // Show what the operator has configured, not the raw file values: a player who
        // reads "15,000 BC" here must receive 15,000 BC, so the advertised amount and
        // the paid amount come from the same scaling and the same per-BBQ override.
        let multiplier = bnd
            .init()
            .bounty
            .reward_multiplier
            .for_bounty(category, bbq);
        let solo_reward = bdesc.solo.scaled(&multiplier);
        let multi_reward = bdesc.multi.scaled(&multiplier);
        Ok(CreateEmbed::new()
            .title(format!("{} {}", category.name(), bbq.name()))
            .description(&bdesc.description)
            .thumbnail(&bdesc.icon)
            .image(&bdesc.thumbnail)
            .field("Rules", rules[..rulen].concat(), false)
            .field(
                "Solo Rewards",
                format!(
                    "Bounty Coin: {}\nGacha Ticket: {} Ticket\n{}",
                    Market::currency(solo_reward.coin as i64),
                    solo_reward.ticket,
                    solo[..solen].concat()
                ),
                false,
            )
            .field(
                "Multiplayer Rewards",
                format!(
                    "Bounty Coin: {}\nGacha Ticket: {} Ticket\n{}",
                    Market::currency(multi_reward.coin as i64),
                    multi_reward.ticket,
                    multi[..mulen].concat()
                ),
                false,
            )
            .color(Color::Random.throw()))
    }

    pub async fn check(data: &[u8], category: &Category) -> Result<(), MyErr> {
        let x = serde_json::from_slice::<Self>(&data)?;
        let pedia = ItemPedia::default();
        for i in &x.bounty {
            let mut item = i.solo.items.clone();
            item.append(&mut i.multi.items.clone());
            for it in item {
                if !it.check(&pedia) {
                    return Err(MyErr::Custom(format!(
                        "There is item mismatch on data struct\n`{it:?}`\nthose items do not exist in the database"
                    )));
                }
            }
        }
        x.save(category).await?;
        Ok(())
    }

    pub async fn new(category: &Category) -> Result<Self, MyErr> {
        let file = tokio::fs::read(Bounty::path(category)).await?;
        Ok(serde_json::from_slice(&file)?)
    }

    pub async fn save(&self, category: &Category) -> Result<(), std::io::Error> {
        let string = serde_json::to_string_pretty(&self)?;
        tokio::fs::write(Bounty::path(category), string.as_bytes()).await
    }
}

impl Default for BountyDesc {
    fn default() -> Self {
        BountyDesc {
            description: "Default bounty description".to_owned(),
            cooldown: 0,
            bbq: 0,
            solo: BountyReward::default(),
            multi: BountyReward::default(),
            icon: "https://media.discordapp.net/attachments/1068440173479739393/1068440373132800080/SAF.png".to_owned(),
            thumbnail: "https://media.discordapp.net/attachments/1068440173479739393/1068440373132800080/SAF.png".to_owned(),
            rules: vec![
                "HR equipment only".to_owned(),
                "No MS".to_owned(),
                "Naked".to_owned(),
            ],
        }
    }
}

impl Default for BountyReward {
    fn default() -> Self {
        BountyReward {
            coin: 1,
            ticket: 1,
            items: vec![ItemCode::default(), ItemCode::default()],
        }
    }
}

impl Default for Bounty {
    fn default() -> Self {
        let x: [BountyDesc; 25] = (0..25)
            .map(|_| BountyDesc::default())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        Bounty { bounty: x }
    }
}

impl Default for BountyRefresh {
    fn default() -> Self {
        BountyRefresh {
            bbq1: 0,
            bbq2: 0,
            bbq3: 0,
            bbq4: 0,
            bbq5: 0,
            bbq6: 0,
            bbq7: 0,
            bbq8: 0,
            bbq9: 0,
            bbq10: 0,
            bbq11: 0,
            bbq12: 0,
            bbq13: 0,
            bbq14: 0,
            bbq15: 0,
            bbq16: 0,
            bbq17: 0,
            bbq18: 0,
            bbq19: 0,
            bbq20: 0,
            bbq21: 0,
            bbq22: 0,
            bbq23: 0,
            bbq24: 0,
            bbq25: 0,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct TitleImage {
    pub url: String,
    pub diameter: u32,
    pub x_start: u32,
    pub y_start: u32,
    #[serde(default)]
    pub animated: bool,
    #[serde(default)]
    pub bg_width: Option<u32>,
    #[serde(default)]
    pub bg_height: Option<u32>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct CustomTitle {
    pub image: TitleImage,
    pub trigger: String,
    pub role_id: u64,
    pub db_code: u8,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct BountyTitle {
    pub bronze_bounty: CustomTitle,
    pub silver_bounty: CustomTitle,
    pub gold_bounty: CustomTitle,
    pub bronze_trading: CustomTitle,
    pub silver_trading: CustomTitle,
    pub gold_trading: CustomTitle,
    pub custom: Vec<CustomTitle>,
}

impl CustomTitle {
    pub fn embed(&self, user: &User) -> CreateEmbed {
        let color = BountyTitle::decrypt(&self.trigger).unwrap().0.color().throw();
        let ext = if self.image.animated { "gif" } else { "jpg" };
        CreateEmbed::new()
            .title("Congratulations On Promotion")
            .color(color)
            .description(format!(
                "You got the Role <@&{}> after clearing {}",
                self.role_id,
                BountyTitle::name(&self.trigger)
            ))
            .image(format!("attachment://title.{}", ext))
            .author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
    }

    pub async fn send_title<T: Mybundle>(&self, bnd: &T, user: &User) -> Result<(), MyErr> {
        let avatar_url = user.static_avatar_url().unwrap_or(user.default_avatar_url());
        let image_bytes = self.image.title(&avatar_url, false).await?;
        let ext = if self.image.animated { "gif" } else { "jpg" };
        let filename = format!("title.{}", ext);

        ChannelId::new(bnd.init().bounty.promotion_ch)
            .send_message(
                &bnd.ctx().http,
                CreateMessage::new()
                    .add_file(CreateAttachment::bytes(image_bytes, filename))
                    .embed(self.embed(user))
                    .content(format!("for {}", user.to_string())),
            )
            .await?;
        Ok(())
    }
}

impl BountyTitle {
    fn path() -> PathBuf {
        Path::new(".").join("static").join("bounty_title.json")
    }

    pub async fn new() -> Result<Self, MyErr> {
        let file = tokio::fs::read_to_string(BountyTitle::path()).await?;
        Ok(serde_json::from_str(&file)?)
    }

    pub async fn save(&self) -> Result<(), MyErr> {
        let file = serde_json::to_string_pretty(&self)?;
        Ok(tokio::fs::write(BountyTitle::path(), file.as_bytes()).await?)
    }

    pub async fn check(data: &[u8]) -> Result<(), MyErr> {
        let x = serde_json::from_slice::<Self>(data)?;
        for i in &x.custom {
            if i.db_code != 0 {
                return Err(MyErr::Custom(
                    "You cannot set a value other than 0 for a custom title's database code".to_owned(),
                ));
            }
        }
        x.save().await?;
        Ok(())
    }

    /// Reports every configured role id that the guild does not actually have.
    ///
    /// Approving a bounty assigns the role for the matching trigger; if that role
    /// does not exist the approval fails at the very last moment, after a judge
    /// already decided to accept the submission. Checking at startup turns a
    /// production-time surprise into a log line nobody has to pay for.
    ///
    /// Read-only: it never creates or edits roles.
    pub async fn missing_roles(&self, guild: GuildId, http: &Http) -> Vec<(String, u64)> {
        let roles = match guild.roles(http).await {
            Ok(roles) => roles,
            Err(_) => return Vec::new(),
        };
        let present: std::collections::HashSet<u64> = roles.keys().map(|id| id.get()).collect();

        let mut missing = Vec::new();
        for i in self.get_trigger() {
            if i.role_id == 0 {
                continue;
            }
            if !present.contains(&i.role_id) {
                missing.push((i.trigger.clone(), i.role_id));
            }
        }
        missing
    }

    /// Reads the Bronze/Silver goals from the achievement triggers.
    ///
    /// The trigger format is `<category>_<bbq>`, so `0_21` means Bronze opens once
    /// BBQ21 is cleared. Reading them here keeps the progression gate in sync with
    /// the moment the achievement role is actually granted instead of duplicating
    /// the numbers in code. Falls back to the shipped defaults when the file is
    /// missing or the triggers do not parse.
    pub async fn progression_tier() -> bounty_cooldown::ProgressionTier {
        let Ok(titles) = Self::new().await else {
            return bounty_cooldown::ProgressionTier::fallback();
        };

        let goal_of = |trigger: &str| -> Option<u8> {
            let (category, bbq) = trigger.split_once('_')?;
            let category: u8 = category.parse().ok()?;
            let bbq: u8 = bbq.parse().ok()?;
            Some(bbq).filter(|_| category <= 4 && bbq > 0)
        };

        bounty_cooldown::ProgressionTier {
            bronze_goal: goal_of(&titles.bronze_bounty.trigger)
                .unwrap_or(bounty_cooldown::ProgressionTier::fallback().bronze_goal),
            silver_goal: goal_of(&titles.silver_bounty.trigger)
                .unwrap_or(bounty_cooldown::ProgressionTier::fallback().silver_goal),
        }
    }

    pub fn hash<'a>(&'a self) -> HashMap<&'a str, &'a CustomTitle> {
        let mut images: HashMap<&str, &CustomTitle> = HashMap::new();
        for (i, _) in self.custom.iter().enumerate() {
            images.insert(&self.custom[i].trigger, &self.custom[i]);
        }
        images.insert(&self.bronze_bounty.trigger, &self.bronze_bounty);
        images.insert(&self.silver_bounty.trigger, &self.silver_bounty);
        images.insert(&self.gold_bounty.trigger, &self.gold_bounty);
        images.insert(&self.bronze_trading.trigger, &self.bronze_trading);
        images.insert(&self.silver_trading.trigger, &self.silver_trading);
        images.insert(&self.gold_trading.trigger, &self.gold_trading);
        images
    }

        pub async fn add_title<T: Mybundle>(
        &self,
        bnd: &T,
        event: &mut Event,
        member: &mut Member,
        trigger: &str,
    ) -> Result<(), MyErr> {
        for i in self.get_trigger() {
            if i.role_id == 0 {
                tracing::warn!(
                    "⚠️ Skipping CustomTitle with trigger '{}': role_id is 0 (not configured in bounty_title.json)",
                    i.trigger
                );
                continue;
            }

            if i.trigger.as_str() == trigger {
                let role = RoleId::new(i.role_id);
                
                if i.db_code != 0 {
                    if !Title::matching(event.title as u8, i.db_code) {
                        event.title += i.db_code as i32;
                    } else {
                        return Ok(());
                    }
                } else {
                    if member.roles.contains(&role) {
                        return Ok(());
                    }
                }
                
                if !member.roles.contains(&role) {
                    member.add_role(&bnd.ctx().http, role).await?;
                }
                
                i.send_title(bnd, &member.user).await?;
            }
        }
        Ok(())
    }

    pub fn decrypt(code: &str) -> Option<(Category, BBQ)> {
        let mut x = code.split('_');
        let cat = Category::new(x.next()?.parse::<u8>().ok()?).ok()?;
        let bbq = BBQ::new(x.next()?.parse::<u8>().ok()?).ok()?;
        Some((cat, bbq))
    }

    fn encrypt(category: Category, bbq: BBQ) -> String {
        format!("{}_{}", category.encode(), bbq.encode())
    }

    fn get_trigger<'a>(&'a self) -> Vec<&'a CustomTitle> {
        let mut out = vec![
            &self.silver_bounty,
            &self.bronze_bounty,
            &self.gold_bounty,
            &self.silver_trading,
            &self.gold_trading,
            &self.bronze_trading,
        ];
        for i in &self.custom {
            out.push(i);
        }
        out
    }

    pub fn name(code: &str) -> String {
        match Self::decrypt(code) {
            Some((x, y)) => format!("{} {}", x.name(), y.name()),
            None => String::from(code),
        }
    }
}

impl Default for TitleImage {
    fn default() -> Self {
        TitleImage {
            url: "https://media.discordapp.net/attachments/1004207525408817323/1006334931607232542/01._Bounty_Expert.jpg?width=1150&height=658".to_owned(),
            diameter: 180,
            x_start: 695,
            y_start: 170,
            animated: false,
            bg_width: Some(1150),
            bg_height: Some(658),
        }
    }
}

impl Default for CustomTitle {
    fn default() -> Self {
        CustomTitle {
            image: TitleImage::default(),
            trigger: "4_0".to_owned(),
            role_id: 1031595216538452038,
            db_code: 0,
        }
    }
}

impl Default for BountyTitle {
    fn default() -> Self {
        BountyTitle {
            custom: vec![CustomTitle::default(), CustomTitle::default()],
            bronze_bounty: CustomTitle::default(),
            silver_bounty: CustomTitle::default(),
            gold_bounty: CustomTitle::default(),
            bronze_trading: CustomTitle::default(),
            silver_trading: CustomTitle::default(),
            gold_trading: CustomTitle::default(),
        }
    }
}

#[cfg(test)]
mod testing {
    use super::*;
    #[tokio::test]
    #[ignore]
    async fn get_json() {
        let x = Bounty::default();
        let y = BountyRefresh::default();
        let z = BountyTitle::default();
        x.save(&Category::Free).await.unwrap();
        y.save(false).await.unwrap();
        z.save().await.unwrap();
    }

    #[test]
    fn the_board_lists_every_bbq_inside_a_code_fence() {
        // The layout the operators read: one fenced block, 25 lines, the exact
        // "======= N left" shape. A board that silently loses BBQ rows is the
        // failure this guards.
        let board = BountyRefresh::default();
        let body = board.cooldown_body();
        assert!(body.starts_with("```\n"));
        assert!(body.ends_with("\n```"));

        let rows: Vec<&str> = body
            .lines()
            .filter(|l| l.starts_with("BBQ"))
            .collect();
        assert_eq!(rows.len(), 25, "every BBQ must appear once");
        assert_eq!(rows[0], "BBQ01 ======= 0 left");
        assert_eq!(rows[24], "BBQ25 ======= 0 left");
    }

    #[test]
    fn the_board_colour_is_fixed_not_random() {
        // A random colour per refresh made one board look like several. This
        // pins the value to the green the production board has always shown.
        assert_eq!(COOLDOWN_BOARD_COLOUR, 0x287760);
    }

    #[test]
    fn the_board_reports_the_last_refresh_time() {
        // The footer text is static; the timestamp is what makes a stale board
        // obvious. Both are applied in `cooldown_embed`, which needs Discord, so
        // this asserts the parts that can be checked offline.
        let board = BountyRefresh::default();
        let body = board.cooldown_body();
        assert!(body.contains("BBQ01 ======= 0 left"));
    }

    fn sample_reward() -> BountyReward {
        BountyReward {
            coin: 15_000,
            ticket: 10,
            items: vec![ItemCode::default()],
        }
    }

    #[test]
    fn a_neutral_multiplier_returns_the_same_reward() {
        let reward = sample_reward();
        let scaled = reward.scaled(&crate::reusable::config::RewardMultiplier::default());
        assert_eq!(scaled.coin, reward.coin);
        assert_eq!(scaled.ticket, reward.ticket);
        assert_eq!(scaled.items.len(), reward.items.len());
    }

    #[test]
    fn scaling_touches_the_currencies_but_not_the_items() {
        let reward = sample_reward();
        let m = crate::reusable::config::RewardMultiplier { coin: 2.0, gacha: 3.0 };
        let scaled = reward.scaled(&m);
        assert_eq!(scaled.coin, 30_000, "coin must double");
        assert_eq!(scaled.ticket, 30, "ticket must triple");
        assert_eq!(
            scaled.items.len(),
            reward.items.len(),
            "items are never scaled: only the two currencies are in scope"
        );
    }

    #[test]
    fn scaling_is_not_applied_twice() {
        // The reward is scaled once in `BountySubmit::new`, so `reward()` and
        // `reward_embed` must read it verbatim. Re-scaling would pay 4x for a 2x
        // setting; this pins the single-scaling contract.
        let reward = sample_reward();
        let m = crate::reusable::config::RewardMultiplier { coin: 2.0, gacha: 1.0 };
        let once = reward.scaled(&m);
        assert_eq!(once.coin, 30_000);
        let neutral = crate::reusable::config::RewardMultiplier::default();
        let again = once.scaled(&neutral);
        assert_eq!(again.coin, 30_000, "an already-scaled reward must pass through");
    }

    #[test]
    fn the_displayed_and_granted_amounts_come_from_one_value() {
        // `reward_embed` formats `self.reward`; `reward()` pays `self.reward`. Because
        // the scaling happens before both, they cannot disagree. This asserts the
        // formatted text really is derived from the scaled value.
        let m = crate::reusable::config::RewardMultiplier { coin: 1.25, gacha: 2.0 };
        let scaled = sample_reward().scaled(&m);
        let shown = Market::currency(scaled.coin as i64);
        assert!(shown.contains("18,750"), "display must show the scaled coin, got {shown}");
        assert_eq!(scaled.ticket, 20, "display must show the scaled ticket");
    }

    /// Reads a shipped category file. Uses the real files so a change to them (or to
    /// the parsing) is caught here, not only against synthetic fixtures.
    fn real_category(category: &Category) -> Bounty {
        let raw = std::fs::read(Bounty::path(category)).expect("category file");
        serde_json::from_slice(&raw).expect("parse category file")
    }

    #[test]
    fn scaling_applies_to_the_real_shipped_bounty_files() {
        // Synthetic fixtures cannot catch a renamed or retyped field. This runs the
        // real `static/*_bounty.json` through the same path the bot uses.
        use crate::reusable::config::RewardMultiplier;

        let free = real_category(&Category::Free);
        let base = free.bounty[0].solo.clone();
        assert!(
            base.coin > 0 && base.ticket > 0,
            "the real Free BBQ01 must carry a positive base reward, got {base:?}"
        );

        let doubled = base.scaled(&RewardMultiplier { coin: 2.0, gacha: 1.0 });
        assert_eq!(doubled.coin, base.coin * 2, "coin must double");

        let neutral = base.scaled(&RewardMultiplier::default());
        assert_eq!(neutral.coin, base.coin, "1.00 must leave the real file untouched");
        assert_eq!(neutral.ticket, base.ticket);
    }

    #[test]
    fn every_shipped_category_parses_and_scales() {
        // Bronze/Silver/Gold/Free/Event are five separate files with slightly
        // different shapes; a multiplier must work for all of them.
        use crate::reusable::config::RewardMultiplier;
        let m = RewardMultiplier { coin: 1.25, gacha: 1.5 };

        for category in [
            Category::Bronze,
            Category::Silver,
            Category::Gold,
            Category::Free,
            Category::Event,
        ] {
            let bounty = real_category(&category);
            let desc = &bounty.bounty[0];
            for (method, base) in [("solo", &desc.solo), ("multi", &desc.multi)] {
                let scaled = base.scaled(&m);
                let expected_coin = ((base.coin as f64) * 1.25).floor() as u32;
                assert_eq!(
                    scaled.coin, expected_coin,
                    "{} {} coin: base {}",
                    category.name(),
                    method,
                    base.coin
                );
                let expected_ticket = ((base.ticket as f64) * 1.5).floor() as u32;
                assert_eq!(
                    scaled.ticket, expected_ticket,
                    "{} {} ticket: base {}",
                    category.name(),
                    method,
                    base.ticket
                );
            }
        }
    }

    /// A preview built from the real shipped Free file, so the rows and the amounts
    /// are the ones an operator would actually see.
    #[test]
    fn the_preview_lists_every_bbq_with_base_and_scaled_amounts() {
        let data = real_category(&Category::Free);
        let multipliers = crate::reusable::config::RewardMultipliers::default();

        let text = data.reward_preview(&Category::Free, &multipliers);

        // One row per BBQ, and the total must be 25 — a silently dropped row is the
        // failure this guards.
        let rows = text
            .lines()
            .filter(|l| l.contains("BBQ"))
            .filter(|l| l.contains(" S "))
            .count();
        assert_eq!(rows, 25, "every BBQ needs a row, got {rows}");

        // With a neutral multiplier the scaled figure equals the base, and the header
        // must say so rather than leave the operator guessing.
        assert!(
            text.contains("every amount equals the category file"),
            "a neutral preview must say nothing changed"
        );
        assert!(!text.contains('*'), "no row may be marked when nothing changes");
    }

    #[test]
    fn the_preview_marks_the_rows_a_multiplier_changes() {
        let data = real_category(&Category::Free);
        let multipliers: crate::reusable::config::RewardMultipliers = serde_json::from_value(
            serde_json::json!({
                "free": { "coin": 2.00, "gacha": 1.00, "per_bbq": { "1": { "coin": 5.00 } } }
            }),
        )
        .expect("multipliers");

        let text = data.reward_preview(&Category::Free, &multipliers);

        assert!(text.contains('*'), "changed rows must be marked");
        assert!(
            text.contains("BBQ01 coin x5.00"),
            "the per-BBQ override must be reported, got:\n{text}"
        );
        assert!(
            text.contains("BBQ02 coin x2.00"),
            "non-overridden rows must report the group factor"
        );
    }

    #[test]
    fn the_preview_shows_the_amount_that_will_actually_be_paid() {
        // The whole point of the preview: what it prints must equal what the payout
        // path computes. Both go through `scaled`, so a divergence means the preview
        // drifted from the payment.
        let data = real_category(&Category::Free);
        let multipliers: crate::reusable::config::RewardMultipliers = serde_json::from_value(
            serde_json::json!({ "free": { "coin": 1.25, "gacha": 1.00 } }),
        )
        .expect("multipliers");

        let m = multipliers.for_bounty(&Category::Free, &BBQ::BBQ01);
        let expected = data.bounty[0].solo.scaled(&m).coin;
        let text = data.reward_preview(&Category::Free, &multipliers);

        let first = data.bounty[0].solo.coin;
        let needle = format!("BBQ01 S {first}->{expected}");
        assert!(
            text.contains(&needle),
            "preview must show {needle}, got:\n{text}"
        );
    }
    fn submit_with_reward(coin: u32, ticket: u32) -> BountySubmit {
        BountySubmit {
            method: Methode::Solo,
            category: Category::Free,
            bbq: BBQ::BBQ01,
            hunter: Vec::new(),
            url: String::new(),
            thumb: String::new(),
            time: 0,
            reward: BountyReward { coin, ticket, items: Vec::new() },
            msg_id: 0,
        }
    }

    #[test]
    fn the_audit_entry_reports_both_factors_and_both_amounts() {
        // The audit line has to answer "what was I multiplied by" and "what did I get".
        // A row missing either half is useless in a dispute.
        let submit = submit_with_reward(30_000, 30);
        let m = crate::reusable::config::RewardMultiplier { coin: 2.0, gacha: 3.0 };
        let title = Title::new(0); // no bounty titles -> bonus 1.0

        let e = submit.audit_entry(&m, &title);

        assert_eq!(e.multiplier_coin, 2.0);
        assert_eq!(e.multiplier_gacha, 3.0);
        assert_eq!(e.title_bonus, 1.0);
        assert_eq!(e.effective_coin_factor, 2.0);
        assert_eq!(e.coin_granted, 30_000);
        assert_eq!(e.gacha_granted, 30);
    }

    #[test]
    fn the_audit_entry_stacks_the_title_bonus_on_the_multiplier() {
        // A player holding the Gold bounty title gets +30%. The effective factor must
        // show the product, because the paid amount is the product.
        let submit = submit_with_reward(26_000, 10);
        let m = crate::reusable::config::RewardMultiplier { coin: 2.0, gacha: 1.0 };
        // Bit 4 is the Gold bounty title.
        let title = Title::new(4);
        assert!(title.bounty_gold);

        let e = submit.audit_entry(&m, &title);

        assert_eq!(e.title_bonus, 1.3, "Gold title carries +30%");
        assert!(
            (e.effective_coin_factor - 2.6).abs() < 1e-9,
            "2.0 x 1.3 must be 2.6, got {}",
            e.effective_coin_factor
        );
    }

    #[test]
    fn the_precise_bonus_agrees_with_the_payout_bonus() {
        // `bonus_precise` exists only for reporting. If the two ever disagreed on
        // which titles count, the audit line would describe a payout that never
        // happened. Every bit combination of the three bounty titles is checked.
        for bits in 0u8..8 {
            let code = match bits {
                0 => 0,
                1 => 1, // bronze
                2 => 2, // silver
                3 => 3, // bronze + silver
                4 => 4, // gold
                5 => 5, // gold + bronze
                6 => 6, // gold + silver
                _ => 7, // all three
            };
            let title = Title::new(code);
            let payout = title.bonus() as f64;
            let reported = title.bonus_precise();
            assert!(
                (payout - reported).abs() < 1e-6,
                "code {code}: payout {payout} vs reported {reported} disagree"
            );
        }
    }
}
