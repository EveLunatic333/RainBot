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
    /// Id of the judge message that carries the Approve/Reject buttons.
    ///
    /// Without it a restored submission reports `0`, which the submit pre-check
    /// reads as "stuck" and silently deletes it, so a submission that survived a
    /// restart would still be dropped, and the player could submit the same
    /// bounty again while the old judge message was still live.
    ///
    /// `default` keeps CACHE files written before this field existed readable.
    #[serde(default)]
    msg_id: u64,
}

#[derive(Serialize, Deserialize)]
struct BountyPre {
    id: String,
    sub: BountySub,
}

/// One submission that could not be rebuilt, for the startup log.
struct SkippedBounty {
    user: String,
    reason: String,
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
            // A party submission always has at least one hunter, but the index
            // below would panic on a malformed entry. Skipping is safer than
            // taking the whole save down.
            let Some(first) = b.hunter.first() else {
                tracing::warn!(user = %s, "Skipping a submission with no hunter while saving CACHE");
                continue;
            };
            let hunt: Vec<u64> = b.hunter.iter().map(|x| x.member.user.id.get()).collect();
            let sub = BountySub {
                method: b.method.encode(),
                category: b.category.encode(),
                bbq: b.bbq.encode(),
                hunter: hunt,
                url: b.url.to_owned(),
                time: b.time,
                gid: first.member.guild_id.get(),
                msg_id: b.msg_id,
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

    /// Rebuilds the in-memory state from `CACHE`.
    ///
    /// Every submission is restored **independently**. A player who left the
    /// guild, never bound their account, or whose category no longer matches a
    /// config file used to abort the whole loop: one bad entry threw away every
    /// other pending submission, while their judge messages kept live buttons
    /// that no longer worked. Now a bad entry is logged and skipped, and the rest
    /// are restored.
    ///
    /// An unreadable CACHE file is still reported to the caller, because nothing
    /// can be salvaged in that case.
    pub async fn load(ctx: &Context, init: &Init) -> Result<(), MyErr> {
        if !Self::path().exists() {
            return Ok(());
        }

        let file: Self = serde_json::from_slice(&tokio::fs::read(Self::path()).await?)?;

        let mut container = HashMap::new();
        let mut skipped: Vec<SkippedBounty> = Vec::new();

        for i in &file.bounty {
            match Self::rebuild(ctx, init, i).await {
                Ok(submit) => {
                    container.insert(i.id.to_owned(), submit);
                }
                Err(reason) => skipped.push(SkippedBounty {
                    user: i.id.to_owned(),
                    reason,
                }),
            }
        }

        let restored = container.len();
        {
            let mut chat = CHAT.lock().await;
            *chat = file
                .gpt
                .iter()
                .map(|x| (x.id.to_string(), x.comp.clone()))
                .collect();
        }
        {
            let mut bounty = BOUNTY.lock().await;
            *bounty = container;
        }
        {
            let mut parties = PARTIES.lock().await;
            *parties = file
                .parties
                .iter()
                .map(|p| (p.message_id.to_string(), p.clone()))
                .collect();
        }

        if restored > 0 {
            tracing::info!(restored = restored, "Restored pending bounty submissions from CACHE");
        }
        for s in &skipped {
            tracing::warn!(
                user = %s.user,
                reason = %s.reason,
                "Could not restore a pending bounty submission; skipping it"
            );
        }
        if !skipped.is_empty() {
            tracing::warn!(
                skipped = skipped.len(),
                restored = restored,
                "Some pending submissions could not be restored and were dropped"
            );
        }
        Ok(())
    }

    /// Rebuilds one cached submission, or explains why it cannot be rebuilt.
    async fn rebuild(ctx: &Context, init: &Init, i: &BountyPre) -> Result<BountySubmit, String> {
        let category = Category::new(i.sub.category)
            .map_err(|e| format!("unknown category {}: {}", i.sub.category, e))?;
        let bbq = BBQ::new(i.sub.bbq)
            .map_err(|e| format!("unknown BBQ number {}: {}", i.sub.bbq, e))?;
        let method = Methode::new(i.sub.method);

        let boxed = Box::new(Bounty::new(&category).await.map_err(|e| {
            format!("cannot load the {} bounty config: {}", category.name(), e)
        })?);
        let desc = bbq.get_bounty(&boxed).map_err(|e| {
            format!("{} is missing from the {} config: {}", bbq.name(), category.name(), e)
        })?;
        // Same scaling as `BountySubmit::new`; a restored submission must be paid the
        // amount the operator has configured now, not the amount frozen at submit time.
        let multiplier = init.bounty.reward_multiplier.for_bounty(&category, &bbq);
        let reward = method.get_reward(desc).scaled(&multiplier);

        let guild = GuildId::new(i.sub.gid)
            .to_partial_guild(&ctx.http)
            .await
            .map_err(|e| format!("cannot resolve guild {}: {}", i.sub.gid, e))?;

        let mut pg = PgConn::create(init, String::new())
            .await
            .map_err(|e| format!("cannot create a database connection: {}", e))?;

        let mut hunter = Vec::new();
        for x in &i.sub.hunter {
            let mem = guild
                .member(&ctx.http, UserId::new(*x))
                .await
                .map_err(|e| format!("user {} is not a member of the guild: {}", x, e))?;
            pg.did = x.to_string();
            let event = pg
                .get_event()
                .await
                .map_err(|e| format!("user {} has no bound account: {}", x, e))?;
            let title_code = u8::try_from(event.title).unwrap_or(0);
            let title = Title::new(title_code);
            hunter.push(Hunter {
                member: mem,
                event,
                title,
            });
        }

        if hunter.is_empty() {
            return Err("the cached submission has no hunters".to_owned());
        }

        Ok(BountySubmit {
            hunter,
            method,
            category,
            bbq,
            url: i.sub.url.to_owned(),
            thumb: desc.thumbnail.to_owned(),
            time: i.sub.time,
            reward,
            // Keep the real judge message id: the submit pre-check uses it to tell
            // a live submission from a stale one. Restoring `0` made every
            // recovered submission look stuck and get deleted.
            msg_id: i.sub.msg_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CACHE body shaped exactly like the one `save()` writes.
    fn cache_json(msg_id_field: &str) -> String {
        format!(
            r#"{{"gpt":[],"bounty":[{{"id":"111","sub":{{"method":0,"category":0,"bbq":1,"hunter":[111],"url":"https://example.invalid/a.png","time":1700000000,"gid":222{msg_id_field}}}}}],"parties":[]}}"#
        )
    }

    #[test]
    fn the_judge_message_id_is_read_back_from_the_cache_file() {
        // Guards the schema half of the fix: `save()` must write the real id and
        // `load()` must read it back. The wiring inside `rebuild()` needs Discord,
        // so it is proved by the restart test on dev instead.
        //
        // Regression: `load` used to hard-code `msg_id: 0`, which made every
        // recovered submission look stuck and get deleted.
        let file: Shutdown =
            serde_json::from_str(&cache_json(r#","msg_id":987654321"#)).expect("CACHE should parse");
        assert_eq!(file.bounty.len(), 1);
        assert_eq!(file.bounty[0].sub.msg_id, 987654321);
    }

    #[test]
    fn a_cache_written_before_msg_id_existed_still_loads() {
        // Older CACHE files have no `msg_id` key at all. They must not fail to
        // parse: `#[serde(default)]` gives them 0, which the submit check already
        // treats as "stale", so behaviour for those is unchanged.
        let file: Shutdown =
            serde_json::from_str(&cache_json("")).expect("legacy CACHE should still parse");
        assert_eq!(file.bounty.len(), 1);
        assert_eq!(file.bounty[0].sub.msg_id, 0);
    }

    #[test]
    fn a_bad_entry_does_not_prevent_the_others_from_parsing() {
        // Parsing never rejects per-entry data: `rebuild` decides. Two entries,
        // one with a nonsense category, must both reach the rebuild step.
        let json = r#"{"gpt":[],"bounty":[
            {"id":"1","sub":{"method":0,"category":99,"bbq":1,"hunter":[1],"url":"u","time":1,"gid":2}},
            {"id":"2","sub":{"method":0,"category":0,"bbq":1,"hunter":[2],"url":"u","time":2,"gid":2}}
        ],"parties":[]}"#;
        let file: Shutdown = serde_json::from_str(json).expect("CACHE should parse");
        assert_eq!(file.bounty.len(), 2, "both entries must reach the rebuild step");
        assert_eq!(file.bounty[0].sub.category, 99);
    }

    #[test]
    fn the_cached_url_is_preserved_verbatim() {
        let file: Shutdown = serde_json::from_str(&cache_json("")).expect("CACHE should parse");
        assert_eq!(file.bounty[0].sub.url, "https://example.invalid/a.png");
        assert_eq!(file.bounty[0].sub.time, 1_700_000_000);
        assert_eq!(file.bounty[0].sub.gid, 222);
    }
}
