use crate::material::ItemPedia;
use super::super::bitwise::{Bitwise, BitwiseError, ItemCode};
use super::card::Event;
use super::PgConn;
use sqlx::{FromRow, Row};

#[derive(FromRow, Debug)]
pub struct GachaPg {
    pub pity: i32,
    pub ticket: i32,
}

impl<'a> PgConn<'a> {
    pub async fn get_coin(&self) -> Result<i32, BitwiseError> {
        Ok(sqlx::query("SELECT bounty FROM discord WHERE discord_id = $1")
            .bind(&self.did)
            .fetch_one(&self.pool)
            .await?
            .get("bounty"))
    }

    pub async fn get_pity(&self) -> Result<GachaPg, BitwiseError> {
        Ok(sqlx::query_as::<_, GachaPg>(
            "SELECT gacha AS ticket, pity FROM discord WHERE discord_id = $1",
        )
        .bind(&self.did)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn buy_ticket(&self, ticket: i32) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET gacha = gacha + $1 WHERE discord_id = $2")
            .bind(ticket)
            .bind(&self.did)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn ticket_all(&self, ticket: i32) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET gacha = gacha + $1 WHERE gacha IS NOT NULL")
            .bind(ticket)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_distribution_items(
        &self,
        dist_id: i32,
        data: &[ItemCode],
    ) -> Result<(), BitwiseError> {
        let mut conn = self.pool.acquire().await?;
        insert_distribution_items_tx(&mut conn, dist_id, data).await
    }

    pub async fn send_item(
        &self,
        data: &[ItemCode],
        cid: i32,
        name: &str,
        desc: &str,
    ) -> Result<(), BitwiseError> {
        let mut conn = self.pool.acquire().await?;
        send_item_tx(&mut conn, data, cid, name, desc).await
    }

    pub async fn send_distrib(
        &self,
        pg: &GachaPg,
        data: &[ItemCode],
        cid: i32,
        pedia: &ItemPedia,
    ) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET gacha = $1, pity = $2 WHERE discord_id = $3")
            .bind(pg.ticket)
            .bind(pg.pity)
            .bind(&self.did)
            .execute(&self.pool)
            .await?;

        if data.len() == 1 {
            let text = match data.first().unwrap().text(pedia) {
                Some(x) => x,
                None => return Err(BitwiseError::NoItem),
            };
            self.send_item(data, cid, &text, &format!("Congratulations on getting {}", &text))
                .await?;
            return Ok(());
        }

        self.send_item(data, cid, "Multi Gacha Rewards", "Unable to list all rewards in description")
            .await?;
        Ok(())
    }

    pub async fn bounty_transaction(&self, price: i32) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET bounty = bounty - $1 WHERE discord_id = $2")
            .bind(price)
            .bind(&self.did)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn bounty_event(&self, event: &Event) -> Result<(), BitwiseError> {
        sqlx::query(
            "UPDATE discord SET bounty = $1, gacha = $2, gold = $3, silver = $4, bronze = $5, \
             latest_bounty = $6, latest_bounty_time = $7, title = $8 WHERE discord_id = $9",
        )
        .bind(event.bounty)
        .bind(event.gacha)
        .bind(event.gold)
        .bind(event.silver)
        .bind(event.bronze)
        .bind(&event.latest_bounty)
        .bind(event.latest_bounty_time)
        .bind(event.title)
        .bind(&self.did)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn bounty_all(&self, gift: i32) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE discord SET bounty = bounty + $1 WHERE bounty IS NOT NULL")
            .bind(gift)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn jelewelry(&self, cid: i32, bought: i32) -> Result<(), BitwiseError> {
        sqlx::query(
            "UPDATE users SET gacha_premium = COALESCE(gacha_premium, 0) + $1 \
             WHERE id = (SELECT user_id FROM characters WHERE id = $2)",
        )
        .bind(bought)
        .bind(cid)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn market(
        &self,
        data: &ItemCode,
        cid: i32,
        price: Option<i32>,
        pedia: &ItemPedia,
    ) -> Result<(), BitwiseError> {
        if let Some(x) = price {
            self.bounty_transaction(x).await?;
        }
        let array = [data.clone()];
        let byte = Bitwise::new(&array);

        if !self.validate_character_exists(cid).await? {
            return Err(BitwiseError::Custom(format!("Character ID {} is not registered", cid)));
        }

        let byte_data = byte.multiple_item()?;

        let dist_id: i32 = sqlx::query(
            "INSERT INTO distribution (character_id, data, type, bot, event_name, description) \
             VALUES ($1, $2, 1, true, $3, $4) RETURNING id",
        )
        .bind(cid)
        .bind(&byte_data)
        .bind(data.text(pedia).unwrap())
        .bind("~C05 Item distributed by administrator")
        .fetch_one(&self.pool)
        .await?
        .get("id");

        self.insert_distribution_items(dist_id, &array).await?;
        Ok(())
    }

    pub async fn market_user(
        &self,
        data: &ItemCode,
        cid: i32,
        price: u32,
        pedia: &ItemPedia,
    ) -> Result<(), BitwiseError> {
        self.bounty_transaction(price as i32).await?;
        let array = [data.clone()];
        let byte = Bitwise::new(&array);

        if !self.validate_character_exists(cid).await? {
            return Err(BitwiseError::Custom(format!("Character ID {} is not registered", cid)));
        }

        let byte_data = byte.multiple_item()?;

        let dist_id: i32 = sqlx::query(
            "INSERT INTO distribution (character_id, data, type, bot, event_name, description) \
             VALUES ($1, $2, 1, true, $3, $4) RETURNING id",
        )
        .bind(cid)
        .bind(&byte_data)
        .bind(data.text(pedia).unwrap())
        .bind("~C05 Market transaction delivery")
        .fetch_one(&self.pool)
        .await?
        .get("id");

        self.insert_distribution_items(dist_id, &array).await?;
        Ok(())
    }
}

/// Inserts the item rows of one distribution inside the caller's transaction.
pub async fn insert_distribution_items_tx(
    conn: &mut sqlx::PgConnection,
    dist_id: i32,
    data: &[ItemCode],
) -> Result<(), BitwiseError> {
    if data.is_empty() {
        return Ok(());
    }

    let mut dist_ids = Vec::with_capacity(data.len());
    let mut item_types = Vec::with_capacity(data.len());
    let mut item_ids = Vec::with_capacity(data.len());
    let mut quantities = Vec::with_capacity(data.len());

    for item in data {
        dist_ids.push(dist_id);
        item_types.push(item.types as i32);
        let reversed_key = item.reverse_key().unwrap_or_else(|_| item.key.clone());
        item_ids.push(i32::from_str_radix(&reversed_key, 16).unwrap_or(0));
        quantities.push(item.count as i32);
    }

    sqlx::query(
        "INSERT INTO distribution_items (distribution_id, item_type, item_id, quantity) \
         SELECT * FROM UNNEST($1::int[], $2::int[], $3::int[], $4::int[])",
    )
    .bind(&dist_ids)
    .bind(&item_types)
    .bind(&item_ids)
    .bind(&quantities)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Grants items to a character inside the caller's transaction.
pub async fn send_item_tx(
    conn: &mut sqlx::PgConnection,
    data: &[ItemCode],
    cid: i32,
    name: &str,
    desc: &str,
) -> Result<(), BitwiseError> {
    let exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM characters WHERE id = $1)")
        .bind(cid)
        .fetch_one(&mut *conn)
        .await?;
    if !exists {
        return Err(BitwiseError::Custom(format!("Character ID {} is not registered", cid)));
    }

    let byte = Bitwise::new(data);
    let byte_data = byte.multiple_item()?;

    let dist_id: i32 = sqlx::query(
        "INSERT INTO distribution (character_id, data, type, bot, event_name, description) \
         VALUES ($1, $2, 1, true, $3, $4) RETURNING id",
    )
    .bind(cid)
    .bind(&byte_data)
    .bind(name)
    .bind(&format!("~C05 {}", desc))
    .fetch_one(&mut *conn)
    .await?
    .get("id");

    insert_distribution_items_tx(conn, dist_id, data).await?;
    Ok(())
}

/// Writes the bounty row of one hunter inside the caller's transaction.
pub async fn bounty_event_tx(
    conn: &mut sqlx::PgConnection,
    did: &str,
    event: &Event,
) -> Result<(), BitwiseError> {
    sqlx::query(
        "UPDATE discord SET bounty = $1, gacha = $2, gold = $3, silver = $4, bronze = $5, \
         latest_bounty = $6, latest_bounty_time = $7, title = $8 WHERE discord_id = $9",
    )
    .bind(event.bounty)
    .bind(event.gacha)
    .bind(event.gold)
    .bind(event.silver)
    .bind(event.bronze)
    .bind(&event.latest_bounty)
    .bind(event.latest_bounty_time)
    .bind(event.title)
    .bind(did)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Creates the per-group cooldown table if it does not exist yet.
///
/// One row per (user, group). `category` and `bbq` are kept to tell a repeat
/// (40 h) from a change (20 h).
pub async fn ensure_bounty_cooldown_group_table(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS bounty_cooldown_group (
            discord_id VARCHAR(255) NOT NULL,
            group_id SMALLINT NOT NULL,
            category SMALLINT NOT NULL,
            bbq SMALLINT NOT NULL,
            completed_at BIGINT NOT NULL,
            PRIMARY KEY (discord_id, group_id)
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Records one completed bounty for a user and cooldown group.
///
/// Only the most recent completion of a group can ever start a cooldown, so one
/// row per (user, group) is enough; older data would already be expired.
pub async fn record_group_completion_tx(
    conn: &mut sqlx::PgConnection,
    did: &str,
    group: i16,
    category: i16,
    bbq: i16,
    completed_at: i64,
) -> Result<(), BitwiseError> {
    sqlx::query(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (discord_id, group_id) DO UPDATE \
         SET category = EXCLUDED.category, bbq = EXCLUDED.bbq, \
             completed_at = EXCLUDED.completed_at",
    )
    .bind(did)
    .bind(group)
    .bind(category)
    .bind(bbq)
    .bind(completed_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[derive(FromRow)]
pub struct GroupCooldownRow {
    pub group_id: i16,
    pub category: i16,
    pub bbq: i16,
    pub completed_at: i64,
}

/// Reads every stored per-group completion for one user (at most two rows).
pub async fn group_history(
    pool: &sqlx::PgPool,
    did: &str,
) -> Result<Vec<GroupCooldownRow>, BitwiseError> {
    Ok(sqlx::query_as::<_, GroupCooldownRow>(
        "SELECT group_id, category, bbq, completed_at \
         FROM bounty_cooldown_group WHERE discord_id = $1",
    )
    .bind(did)
    .fetch_all(pool)
    .await?)
}

/// Deletes the cooldown rows of one user, so an administrator reset is not
/// immediately undone by leftovers. Clears both the current group table and the
/// legacy per-category table kept for rollback.
pub async fn clear_cooldown_history(
    conn: &mut sqlx::PgConnection,
    did: &str,
) -> Result<(), BitwiseError> {
    sqlx::query("DELETE FROM bounty_cooldown_group WHERE discord_id = $1")
        .bind(did)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "DO $$ BEGIN \
             IF to_regclass('public.bounty_cooldown') IS NOT NULL THEN \
                 DELETE FROM bounty_cooldown WHERE discord_id = $1; \
             END IF; \
         END $$",
    )
    .bind(did)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Folds the retired Free cooldown row into the shared standard row.
///
/// Free used to have a group of its own (database key `1`), which let one player
/// take a Bronze/Silver/Gold bounty and a Free bounty in the same window. The two
/// are now one group, so the newest of the two rows is moved onto the standard key
/// (`0`) and the retired row is deleted.
///
/// Runs in one transaction, so an interrupted start cannot leave a player with
/// both the old and the new row. `ON CONFLICT ... WHERE EXCLUDED.completed_at >
/// bounty_cooldown_group.completed_at` only ever moves a newer timestamp onto the
/// standard row, so a real standard completion is never rolled back by a stale
/// Free row.
///
/// A row is kept if the migration has not run yet: `CooldownGroup::decode` maps
/// the retired key `1` onto `Standard`, so a leftover row still gates the timer
/// instead of unlocking it.
pub async fn merge_free_into_standard(pool: &sqlx::PgPool) -> Result<(u64, u64), sqlx::Error> {
    use crate::reusable::component::bounty_cooldown::CooldownGroup;

    let standard = CooldownGroup::Standard.encode();
    // The retired key, spelled out rather than decoded: `decode` now maps it onto
    // the standard group, so it cannot be recovered from the enum any more.
    const RETIRED_FREE_KEY: i16 = 1;

    let mut tx = pool.begin().await?;

    let folded = sqlx::query(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT DISTINCT ON (discord_id)
                discord_id, $1, category, bbq, completed_at
         FROM bounty_cooldown_group
         WHERE group_id IN ($1, $2)
         ORDER BY discord_id, completed_at DESC
         ON CONFLICT (discord_id, group_id) DO UPDATE
         SET category = EXCLUDED.category,
             bbq = EXCLUDED.bbq,
             completed_at = EXCLUDED.completed_at
         WHERE EXCLUDED.completed_at > bounty_cooldown_group.completed_at",
    )
    .bind(standard)
    .bind(RETIRED_FREE_KEY)
    .execute(&mut *tx)
    .await?;

    let deleted = sqlx::query("DELETE FROM bounty_cooldown_group WHERE group_id = $1")
        .bind(RETIRED_FREE_KEY)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok((folded.rows_affected(), deleted.rows_affected()))
}

/// Carries the legacy single-row cooldown over into the group table.
///
/// Before any cooldown table existed the last completion lived in
/// `discord.latest_bounty` / `latest_bounty_time`, and that **one** timestamp
/// gated the submission regardless of which category it was: the old check was
/// `time > now && category != Event`, with 40 h on an exact (category, BBQ)
/// repeat and 20 h otherwise. There was no per-group split at all.
///
/// So one legacy timestamp is evidence for **both** surviving groups:
///
/// * The standard group was gated by it, whatever the stored category was. A
///   player whose last run was an Event was still blocked from taking a Gold for
///   20 h. Writing only the stored category's group would therefore unlock the
///   ladder and let those players submit immediately.
/// * Event was exempt under the old rule, so a legacy Event timestamp is **not**
///   evidence that Event should be blocked. Event now has its own timer, and it
///   measures the wait between Events; seeding it from a Free or Bronze run would
///   invent a restriction the old build never applied.
///
/// The migration writes the timestamp into the standard group unconditionally,
/// and into Event only when the stored category already was Event.
///
/// Rows are skipped when the legacy value cannot be read — a malformed
/// `latest_bounty` or a BBQ outside 1..=25 — because `BountyTitle::decrypt`
/// returned `None` for those too, so the old build treated them as no history.
///
/// `ON CONFLICT DO NOTHING` keeps rows written by a real completion, so running
/// this on every start is safe.
pub async fn backfill_cooldown_history(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    use crate::reusable::component::bounty::Category;
    use crate::reusable::component::bounty_cooldown::CooldownGroup;

    // Built from the enums so the database keys can never drift from the code.
    let groups = CooldownGroup::ALL
        .iter()
        .map(|g| format!("({}::smallint)", g.encode()))
        .collect::<Vec<_>>()
        .join(", ");
    let event_group = CooldownGroup::Event.encode();
    let event_category = i16::from(Category::Event.encode());

    let result = sqlx::query(&format!(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT d.discord_id, g.group_id, d.c, d.bbq, d.latest_bounty_time
         FROM (
             SELECT discord_id,
                    split_part(latest_bounty, '_', 1)::smallint AS c,
                    split_part(latest_bounty, '_', 2)::smallint AS bbq,
                    latest_bounty_time
             FROM discord
             WHERE latest_bounty_time > 0
               AND latest_bounty ~ '^[0-9]+_[0-9]+$'
               AND split_part(latest_bounty, '_', 1)::smallint BETWEEN 0 AND 4
               AND split_part(latest_bounty, '_', 2)::smallint BETWEEN 1 AND 25
         ) d
         CROSS JOIN (VALUES {groups}) AS g(group_id)
         WHERE g.group_id <> $1 OR d.c = $2
         ON CONFLICT (discord_id, group_id) DO NOTHING"
    ))
    .bind(event_group)
    .bind(event_category)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Migrates the superseded per-category table into the per-group table.
///
/// The previous design stored one row per category, which let a player chain
/// Bronze/Silver/Gold with no wait. For each (user, group) the most recent row is
/// kept, because an older one would already be expired. Category `4` (Event) is
/// the only one that keeps a group of its own; everything else lands on the
/// shared standard row. If the old table does not exist this is a no-op, so it is
/// safe to call on every start.
pub async fn migrate_category_cooldown_to_group(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT to_regclass('public.bounty_cooldown')::text",
    )
    .fetch_one(pool)
    .await?;
    if exists.is_none() {
        return Ok(0);
    }

    let result = sqlx::query(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT DISTINCT ON (discord_id, group_id)
                discord_id, group_id, category, bbq, completed_at
         FROM (
             SELECT discord_id,
                    CASE WHEN category = 4 THEN 2 ELSE 0 END AS group_id,
                    category,
                    bbq,
                    completed_at
             FROM bounty_cooldown
             WHERE category BETWEEN 0 AND 4
         ) t
         ORDER BY discord_id, group_id, completed_at DESC
         ON CONFLICT (discord_id, group_id) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
