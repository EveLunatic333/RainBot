use std::collections::HashMap;
use std::path::{PathBuf, Path};
use crate::reusable::config::Init;
use crate::reusable::postgress::PgConn;
use crate::{MyErr, CHAT, BOUNTY, PARTIES};
use crate::commands::misc::party::PartyRecord;
use super::bounty::{BountySubmit, Bounty, Category, Title, Hunter, BBQ, Methode};
use super::super::gpt::chat::Preserve;
use serde::{Serialize, Deserialize};
use serenity::all::{GuildId, UserId};
use serenity::prelude::Context;

#[derive(Serialize, Deserialize)]
struct BountySub {
    method: u8,
    category: u8,
    bbq: u8,
    hunter: Vec<u64>,
    url: String,
    time: i64,
    gid: u64,
}

#[derive(Serialize, Deserialize)]
struct BountyPre {
    id: String,
    sub: BountySub,
}

#[derive(Serialize, Deserialize)]
pub struct Shutdown {
    gpt: Vec<Preserve>,
    bounty: Vec<BountyPre>,
    // Defaults to an empty list so CACHE files written before the party
    // feature still load correctly.
    #[serde(default)]
    parties: Vec<PartyRecord>,
}

impl Shutdown {
    fn path() -> PathBuf {
        Path::new(".").join("CACHE")
    }

    pub async fn save() -> Result<(), MyErr> {
        let bont = BOUNTY.lock().await;
        let mut bounty = Vec::new();

        for (s, b) in bont.iter() {
            let hunt: Vec<u64> = b.hunter.iter().map(|x| x.member.user.id.get()).collect();
            let sub = BountySub {
                method: b.method.encode(),
                category: b.category.encode(),
                bbq: b.bbq.encode(),
                hunter: hunt,
                url: b.url.to_owned(),
                time: b.time,
                gid: b.hunter[0].member.guild_id.get(),
            };
            bounty.push(BountyPre { id: s.to_owned(), sub });
        }

        let gpt: Vec<Preserve> = CHAT
            .lock()
            .await
            .iter()
            .map(|(id, comp)| Preserve {
                id: id.to_owned(),
                comp: comp.clone(),
            })
            .collect();

        let parties: Vec<PartyRecord> = PARTIES.lock().await.values().cloned().collect();

        let sel = Self { gpt, bounty, parties };
        tokio::fs::write(Self::path(), &serde_json::to_string(&sel)?.as_bytes()).await?;
        Ok(())
    }

    pub async fn load(ctx: &Context, init: &Init) -> Result<(), MyErr> {
        if Self::path().exists() {
            let file: Self = serde_json::from_slice(&tokio::fs::read(Self::path()).await?)?;
            let mut chat = CHAT.lock().await;
            let mut bounty = BOUNTY.lock().await;

            *chat = file
                .gpt
                .iter()
                .map(|x| (x.id.to_string(), x.comp.clone()))
                .collect();

            let mut container = HashMap::new();

            for i in &file.bounty {
                let category = Category::new(i.sub.category)?;
                let bbq = BBQ::new(i.sub.bbq)?;
                let method = Methode::new(i.sub.method);
                let boxed = Box::new(Bounty::new(&category).await?);
                let desc = bbq.get_bounty(&boxed)?;
                let reward = method.get_reward(desc).clone();
                let mut hunter = Vec::new();

                let guild = GuildId::new(i.sub.gid)
                    .to_partial_guild(&ctx.http)
                    .await?;

                let mut pg = PgConn::create(init, String::new()).await?;

                for x in &i.sub.hunter {
                    let mem = guild.member(&ctx.http, UserId::new(*x)).await?;
                    pg.did = x.to_string();
                    let event = pg.get_event().await?;
                    let title_code = u8::try_from(event.title).unwrap_or(0);
                    let title = Title::new(title_code);
                    hunter.push(Hunter {
                        member: mem,
                        event,
                        title,
                    });
                }

                let submit = BountySubmit {
                    hunter,
                    method,
                    category,
                    bbq,
                    url: i.sub.url.to_owned(),
                    thumb: desc.thumbnail.to_owned(),
                    time: i.sub.time,
                    reward,
					msg_id: 0,
                };
                container.insert(i.id.to_owned(), submit);
            }
            *bounty = container;

            let mut parties = PARTIES.lock().await;
            *parties = file
                .parties
                .iter()
                .map(|p| (p.message_id.to_string(), p.clone()))
                .collect();
        }
        Ok(())
    }
}