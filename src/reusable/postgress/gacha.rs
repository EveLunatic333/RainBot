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
/// One row per (user, group), so a Progression run locks Bronze, Silver and Gold
/// together while Free and Event keep their own timers. `category` and `bbq` are
/// kept to tell a repeat (40 h) from a change (20 h).
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

/// Reads every stored per-group completion for one user (at most three rows).
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

/// Carries the legacy single-row cooldown over into the group table.
///
/// Before any cooldown table existed the last completion lived in
/// `discord.latest_bounty` / `latest_bounty_time`. Without a backfill every user
/// would look like they had never done a bounty, and every outstanding cooldown
/// would silently vanish on upgrade. `ON CONFLICT DO NOTHING` keeps newer rows
/// intact, so running this on every start is safe.
pub async fn backfill_cooldown_history(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    // Category -> group: Bronze/Silver/Gold (0,1,2) collapse to 0, Free is 1,
    // Event is 2. The CASE mirrors CooldownGroup::encode.
    let result = sqlx::query(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT d.discord_id,
                CASE WHEN c = 3 THEN 1 WHEN c = 4 THEN 2 ELSE 0 END,
                c,
                split_part(d.latest_bounty, '_', 2)::smallint,
                d.latest_bounty_time
         FROM (
             SELECT discord_id, latest_bounty, latest_bounty_time,
                    split_part(latest_bounty, '_', 1)::smallint AS c
             FROM discord
             WHERE latest_bounty_time > 0
               AND latest_bounty ~ '^[0-9]+_[0-9]+$'
               AND split_part(latest_bounty, '_', 1)::smallint BETWEEN 0 AND 4
         ) d
         ON CONFLICT (discord_id, group_id) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Migrates the superseded per-category table into the per-group table.
///
/// The previous design stored one row per category, which let a player chain
/// Bronze/Silver/Gold with no wait. For each (user, group) the most recent row is
/// kept, because an older one would already be expired. If the old table does not
/// exist this is a no-op, so it is safe to call on every start.
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
                    CASE WHEN category = 3 THEN 1 WHEN category = 4 THEN 2 ELSE 0 END AS group_id,
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
