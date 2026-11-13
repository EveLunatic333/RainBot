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

    // The legacy table is only deleted from when it still exists, so this stays
    // safe after a rollback table is dropped. The existence probe is a separate
    // statement on purpose: a `DO` block runs as PL/pgSQL and cannot reference a
    // bind parameter, so `... WHERE discord_id = $1` inside it fails to parse with
    // "there is no parameter $1" before it ever runs.
    let legacy_exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.bounty_cooldown')::text")
            .fetch_one(&mut *conn)
            .await?;
    if legacy_exists.is_some() {
        sqlx::query("DELETE FROM bounty_cooldown WHERE discord_id = $1")
            .bind(did)
            .execute(&mut *conn)
            .await?;
    }
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

    let progression_group = CooldownGroup::Progression.encode();
    let standard_group = CooldownGroup::Standard.encode();
    let event_group = CooldownGroup::Event.encode();
    let bronze = i16::from(Category::Bronze.encode());
    let gold = i16::from(Category::Gold.encode());
    let event_category = i16::from(Category::Event.encode());

    // One row, one group: the legacy column is a single timestamp, so it can only
    // seed the group its own category belongs to. This used to CROSS JOIN every
    // group, which was right when there were two groups (one shared timer plus
    // Event). With three groups that would seed both the Free and the progression
    // timer from the same run, inventing a cooldown the player never earned.
    let result = sqlx::query(&format!(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT d.discord_id,
                CASE WHEN d.c BETWEEN {bronze} AND {gold} THEN $1::smallint
                     WHEN d.c = {event_category} THEN $2::smallint
                     ELSE $3::smallint END,
                d.c, d.bbq, d.latest_bounty_time
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
         ON CONFLICT (discord_id, group_id) DO NOTHING"
    ))
    .bind(progression_group)
    .bind(event_group)
    .bind(standard_group)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Moves ladder rows off the old shared group key and onto the progression key.
///
/// Before the ladder had a timer of its own, its rows were written with the group
/// key Free still uses (`0`). The ladder now reads `3`, so a row left on `0` would
/// stop gating anything and every player would look freshly unlocked.
///
/// This is idempotent: it only matches rows whose category belongs to the ladder,
/// and `ON CONFLICT DO UPDATE ... WHERE EXCLUDED.completed_at > ...` keeps the
/// newer of the two timestamps. Running it on every start is therefore safe, and
/// running it twice changes nothing the second time.
///
/// It is **not** wired into startup yet: the production table still mixes Free and
/// ladder rows from the previous build, and moving them during an unattended
/// upgrade would change live cooldowns without the operator seeing the result
/// first. Call it explicitly after taking a backup.
pub async fn regroup_ladder_rows_into_progression(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    use crate::reusable::component::bounty::Category;
    use crate::reusable::component::bounty_cooldown::CooldownGroup;

    let standard = CooldownGroup::Standard.encode();
    let progression = CooldownGroup::Progression.encode();
    let bronze = i16::from(Category::Bronze.encode());
    let gold = i16::from(Category::Gold.encode());

    let result = sqlx::query(&format!(
        "INSERT INTO bounty_cooldown_group (discord_id, group_id, category, bbq, completed_at)
         SELECT discord_id, {progression}::smallint, category, bbq, completed_at
         FROM bounty_cooldown_group
         WHERE group_id = $1 AND category BETWEEN {bronze} AND {gold}
         ON CONFLICT (discord_id, group_id) DO UPDATE
         SET category = EXCLUDED.category,
             bbq = EXCLUDED.bbq,
             completed_at = EXCLUDED.completed_at
         WHERE EXCLUDED.completed_at > bounty_cooldown_group.completed_at"
    ))
    .bind(standard)
    .execute(pool)
    .await?;
    let moved = result.rows_affected();

    // The source rows are removed so the ladder cannot be gated by a stale copy.
    // Only ladder categories are touched, so a genuine Free row survives.
    let deleted = sqlx::query(&format!(
        "DELETE FROM bounty_cooldown_group
         WHERE group_id = $1 AND category BETWEEN {bronze} AND {gold}"
    ))
    .bind(standard)
    .execute(pool)
    .await?;
    Ok(moved.max(deleted.rows_affected()))
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
                    CASE WHEN category = 4 THEN 2
                          WHEN category IN (0, 1, 2) THEN 3
                          ELSE 0 END AS group_id,
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
#[cfg(test)]
mod event_sharing_db_tests {
    //! End-to-end check of `event_shares_standard` against a real database.
    //!
    //! These are `#[ignore]`d because they need a live PostgreSQL with the bounty
    //! tables, which the normal unit-test run does not have. Run them on the dev
    //! host with:
    //!
    //! ```text
    //! RAINBOT_TEST_DATABASE_URL=postgres://user:pass@host:port/db \
    //!     cargo test event_sharing_db -- --ignored --nocapture
    //! ```
    //!
    //! Each test writes rows for a throwaway discord id and deletes them again, so
    //! real player data is never touched.

    use super::*;
    use crate::reusable::component::bounty::{BBQ, Category};
    use crate::reusable::component::bounty_cooldown::{
        self, CooldownPolicy, CooldownVerdict,
    };

    /// Throwaway ids. `999...` cannot collide with a real Discord snowflake.
    const USER_A: &str = "999000000000009001";
    const USER_B: &str = "999000000000009002";

    async fn pool() -> sqlx::PgPool {
        let url = std::env::var("RAINBOT_TEST_DATABASE_URL").expect(
            "set RAINBOT_TEST_DATABASE_URL to run the database-backed tests",
        );
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("cannot connect to the test database")
    }

    async fn cleanup(pool: &sqlx::PgPool) {
        for user in [USER_A, USER_B] {
            let _ = sqlx::query("DELETE FROM bounty_cooldown_group WHERE discord_id = $1")
                .bind(user)
                .execute(pool)
                .await;
        }
    }

    async fn rows_of(pool: &sqlx::PgPool, user: &str) -> Vec<(i16, i16, i16, i64)> {
        group_history(pool, user)
            .await
            .expect("group_history must work")
            .into_iter()
            .map(|r| (r.group_id, r.category, r.bbq, r.completed_at))
            .collect()
    }

    /// What the submit pre-check does, in the same order.
    ///
    /// `legacy` is the `(latest_bounty, latest_bounty_time)` pair the reward flow
    /// writes to the `discord` row. Passing it explicitly matters: a zero time
    /// means "the operator cleared the timer", which deliberately drops the
    /// standard rows, so a test that passed 0 would measure something else than a
    /// real completion.
    async fn verdict_for(
        pool: &sqlx::PgPool,
        user: &str,
        category: Category,
        bbq: BBQ,
        policy: &CooldownPolicy,
        legacy: (&str, i64),
        now: i64,
    ) -> CooldownVerdict {
        let history = bounty_cooldown::history_with_legacy_and_policy(
            &bounty_cooldown::history_from_rows(&rows_of(pool, user).await),
            legacy.0,
            legacy.1,
            policy,
        );
        bounty_cooldown::evaluate(&history, &category, &bbq, policy, now)
    }

    /// The `latest_bounty` string the reward flow stores for a completion.
    ///
    /// `BountyTitle::encrypt` is the private `<category>_<bbq>` formatter, so this
    /// mirrors it. `BountyTitle::decrypt` is the public reader, and the round trip
    /// is asserted below so the mirror cannot drift unnoticed.
    fn legacy_code(category: Category, bbq: BBQ) -> String {
        format!("{}_{}", category.encode(), bbq.encode())
    }

    fn now() -> i64 {
        crate::reusable::utils::MyTime::now()
    }

    #[tokio::test]
    #[ignore]
    async fn an_event_completion_is_written_to_the_shared_row_when_enabled() {
        let pool = pool().await;
        cleanup(&pool).await;

        // The exact expression `send_reward` uses for the database key.
        let on = CooldownPolicy::default().with_event_sharing(true);
        let off = CooldownPolicy::default().with_event_sharing(false);
        assert_eq!(on.group_of(&Category::Event).encode(), 0);
        assert_eq!(off.group_of(&Category::Event).encode(), 2);

        // Write an Event completion the way the reward flow would, with sharing on.
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_A,
            on.group_of(&Category::Event).encode(),
            Category::Event.encode() as i16,
            BBQ::BBQ01.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        let rows = rows_of(&pool, USER_A).await;
        assert_eq!(rows.len(), 1, "one row expected: {rows:?}");
        assert_eq!(rows[0].0, 0, "the row must be the standard group key");
        assert_eq!(rows[0].1, Category::Event.encode() as i16);

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_event_row_blocks_free_only_when_enabled() {
        let pool = pool().await;
        cleanup(&pool).await;

        // An Event completion stored under the Event key, as the current build
        // writes it. This is the data that already exists on a live server.
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_A,
            2,
            Category::Event.encode() as i16,
            BBQ::BBQ01.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        // The legacy column as the reward flow leaves it after an Event run. An
        // Event value reports `Unusable`, so it neither adds nor removes rows.
        let legacy = (legacy_code(Category::Event, BBQ::BBQ01), now());

        // Off: Free is untouched, which is the behaviour that must survive.
        assert_eq!(
            verdict_for(
                &pool,
                USER_A,
                Category::Free,
                BBQ::BBQ02,
                &CooldownPolicy::default(),
                (&legacy.0, legacy.1),
                now(),
            )
            .await,
            CooldownVerdict::Allowed,
            "with sharing off an Event row must not lock Free"
        );

        // On: the same stored row now gates Free.
        let on = CooldownPolicy::default().with_event_sharing(true);
        match verdict_for(&pool, USER_A, Category::Free, BBQ::BBQ02, &on, (&legacy.0, legacy.1), now()).await {
            CooldownVerdict::Blocked { ends_at } => {
                assert!(ends_at > now(), "ends_at must be in the future");
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // The ladder has its own timer, so the Event run must not reach it.
        assert_eq!(
            verdict_for(&pool, USER_A, Category::Bronze, BBQ::BBQ02, &on, (&legacy.0, legacy.1), now()).await,
            CooldownVerdict::Allowed,
            "the ladder is out of reach of `event_shares_standard`"
        );

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_free_row_blocks_event_only_when_enabled() {
        let pool = pool().await;
        cleanup(&pool).await;

        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_B,
            0,
            Category::Free.encode() as i16,
            BBQ::BBQ03.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        // A real Free completion stamps the legacy column with the Free run.
        let legacy = (legacy_code(Category::Free, BBQ::BBQ03), now());

        assert_eq!(
            verdict_for(
                &pool,
                USER_B,
                Category::Event,
                BBQ::BBQ01,
                &CooldownPolicy::default(),
                (&legacy.0, legacy.1),
                now(),
            )
            .await,
            CooldownVerdict::Allowed,
            "with sharing off a Free row must not lock Event"
        );

        let on = CooldownPolicy::default().with_event_sharing(true);
        match verdict_for(&pool, USER_B, Category::Event, BBQ::BBQ01, &on, (&legacy.0, legacy.1), now()).await {
            CooldownVerdict::Blocked { .. } => {}
            other => panic!("expected Blocked, got {other:?}"),
        }

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn turning_the_option_off_recovers_the_event_timer_from_storage() {
        let pool = pool().await;
        cleanup(&pool).await;

        // Stored the way the shipped build writes it.
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_A,
            2,
            Category::Event.encode() as i16,
            BBQ::BBQ01.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        // Flip the option on and back off without writing anything in between:
        // the stored key is never destroyed, so the Event timer must return.
        let legacy = (legacy_code(Category::Event, BBQ::BBQ01), now());

        let on = CooldownPolicy::default().with_event_sharing(true);
        let _ = verdict_for(&pool, USER_A, Category::Event, BBQ::BBQ01, &on, (&legacy.0, legacy.1), now()).await;

        let off = CooldownPolicy::default().with_event_sharing(false);
        match verdict_for(&pool, USER_A, Category::Event, BBQ::BBQ01, &off, (&legacy.0, legacy.1), now()).await {
            CooldownVerdict::Blocked { ends_at } => {
                // Same category and BBQ is a repeat, so it waits `same`.
                let expected = now() + bounty_cooldown::DEFAULT_SAME_HOURS * 3600;
                assert!(
                    (ends_at - expected).abs() <= 5,
                    "expected about {expected}, got {ends_at}"
                );
            }
            other => panic!("expected Blocked after turning the option off, got {other:?}"),
        }

        cleanup(&pool).await;
    }

    #[test]
    fn the_legacy_code_matches_what_the_reader_expects() {
        // Cheap guard against the mirrored formatter drifting from the real one.
        assert_eq!(
            crate::reusable::component::bounty::BountyTitle::decrypt(
                &legacy_code(Category::Event, BBQ::BBQ01)
            ),
            Some((Category::Event, BBQ::BBQ01))
        );
        assert_eq!(
            crate::reusable::component::bounty::BountyTitle::decrypt(
                &legacy_code(Category::Gold, BBQ::BBQ03)
            ),
            Some((Category::Gold, BBQ::BBQ03))
        );
    }

    #[tokio::test]
    #[ignore]
    async fn both_modes_leave_no_residue_behind() {
        let pool = pool().await;
        cleanup(&pool).await;

        // Nothing is left for either throwaway user, so the tests above cannot
        // have polluted the table.
        assert!(rows_of(&pool, USER_A).await.is_empty());
        assert!(rows_of(&pool, USER_B).await.is_empty());
    }
    /// Records a ladder completion on whichever key the policy writes, then reads it
    /// back through the same policy. This is the round trip `send_reward` and
    /// `Hunter::new` actually perform.
    async fn ladder_verdict_after_write(
        pool: &sqlx::PgPool,
        user: &str,
        policy: &CooldownPolicy,
        legacy: (&str, i64),
    ) -> CooldownVerdict {
        let key = policy.group_of(&Category::Bronze).encode();
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            user,
            key,
            Category::Bronze.encode() as i16,
            BBQ::BBQ01.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        verdict_for(
            pool,
            user,
            Category::Bronze,
            BBQ::BBQ02,
            policy,
            legacy,
            now(),
        )
        .await
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_ladder_row_lands_on_its_own_key() {
        let pool = pool().await;
        cleanup(&pool).await;

        let off = CooldownPolicy::default();
        // `latest_bounty_time > 0` keeps the legacy column from clearing the ladder.
        let legacy = (legacy_code(Category::Bronze, BBQ::BBQ01), now());
        let verdict = ladder_verdict_after_write(&pool, USER_A, &off, (&legacy.0, legacy.1)).await;
        match verdict {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("expected Blocked, got {other:?}"),
        }

        // The row must be readable under the progression key, not the shared one.
        let rows = rows_of(&pool, USER_A).await;
        assert!(
            rows.iter().any(|(g, _, _, _)| *g == 3),
            "the ladder must write key 3: {rows:?}"
        );

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_ladder_row_shares_the_key_when_enabled() {
        let pool = pool().await;
        cleanup(&pool).await;

        let on = CooldownPolicy::default().with_progression_sharing(true);
        let legacy = (legacy_code(Category::Bronze, BBQ::BBQ01), now());
        let verdict = ladder_verdict_after_write(&pool, USER_A, &on, (&legacy.0, legacy.1)).await;
        match verdict {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("expected Blocked, got {other:?}"),
        }

        let rows = rows_of(&pool, USER_A).await;
        assert!(
            rows.iter().any(|(g, _, _, _)| *g == 0),
            "while sharing the ladder must write the shared key: {rows:?}"
        );

        // A ladder run on the shared key must now gate Free too.
        let free = verdict_for(
            &pool,
            USER_A,
            Category::Free,
            BBQ::BBQ02,
            &on,
            (&legacy.0, legacy.1),
            now(),
        )
        .await;
        match free {
            CooldownVerdict::Blocked { .. } => {}
            other => panic!("expected Blocked for Free, got {other:?}"),
        }

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn turning_ladder_sharing_off_recovers_the_ladder_timer() {
        let pool = pool().await;
        cleanup(&pool).await;

        // Written while sharing was on, so the row sits on the shared key.
        let on = CooldownPolicy::default().with_progression_sharing(true);
        let legacy = (legacy_code(Category::Bronze, BBQ::BBQ01), now());
        ladder_verdict_after_write(&pool, USER_A, &on, (&legacy.0, legacy.1)).await;

        // Read back with sharing off: the ladder must recover its own timer and must
        // stop gating Free. This is the defect a one-way `normalize` had.
        let off = CooldownPolicy::default();
        let ladder = verdict_for(
            &pool,
            USER_A,
            Category::Bronze,
            BBQ::BBQ02,
            &off,
            (&legacy.0, legacy.1),
            now(),
        )
        .await;
        match ladder {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("expected the ladder timer back, got {other:?}"),
        }

        let free = verdict_for(
            &pool,
            USER_A,
            Category::Free,
            BBQ::BBQ02,
            &off,
            (&legacy.0, legacy.1),
            now(),
        )
        .await;
        assert_eq!(
            free,
            CooldownVerdict::Allowed,
            "the recovered ladder row must stop gating Free"
        );

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn a_pre_split_ladder_row_on_the_shared_key_still_gates_the_ladder() {
        let pool = pool().await;
        cleanup(&pool).await;

        // Rows written by the build before the split carried the ladder on key 0.
        // `normalize` derives the group from the category, so they must still gate
        // the ladder even if `regroup` never runs.
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_B,
            0,
            Category::Gold.encode() as i16,
            BBQ::BBQ07.encode() as i16,
            now(),
        )
        .await
        .expect("the completion must be recorded");

        let legacy = (legacy_code(Category::Gold, BBQ::BBQ07), now());
        let off = CooldownPolicy::default();
        let verdict = verdict_for(
            &pool,
            USER_B,
            Category::Bronze,
            BBQ::BBQ01,
            &off,
            (&legacy.0, legacy.1),
            now(),
        )
        .await;
        match verdict {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("expected Blocked, got {other:?}"),
        }

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn an_operator_can_still_patch_a_ladder_cooldown_with_sql() {
        // The reported regression, end to end against a real database: an operator
        // "corrects" a cooldown with a plain
        // `UPDATE discord SET latest_bounty_time = ...` on a player whose last run
        // was a ladder step, while the ladder shares the standard row.
        //
        // Before the fix the entry was placed on the static group (3) while
        // evaluation read group 0, so the edit was parsed, written to a row nothing
        // consulted, and silently ignored. On the live server that was 1623 of 3621
        // readable players whose last run was a ladder step.
        let pool = pool().await;
        cleanup(&pool).await;
        let on = CooldownPolicy::default().with_progression_sharing(true);

        // The column says "a Gold run happened just now", and there is no stored row
        // at all — exactly a player whose cooldown an operator is extending by hand.
        let legacy = (legacy_code(Category::Gold, BBQ::BBQ07), now());

        // The ladder must be gated by it.
        match verdict_for(&pool, USER_A, Category::Bronze, BBQ::BBQ01, &on, (&legacy.0, legacy.1), now()).await {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("a ladder row in the column must still gate the ladder, got {other:?}"),
        }

        // Free shares the same row, so it must be gated too.
        match verdict_for(&pool, USER_A, Category::Free, BBQ::BBQ01, &on, (&legacy.0, legacy.1), now()).await {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("the shared row must gate Free as well, got {other:?}"),
        }

        // Clearing the column must release both. This is the other half of the
        // operator habit (`SET latest_bounty_time = 0`) and has to keep working.
        assert_eq!(
            verdict_for(&pool, USER_A, Category::Bronze, BBQ::BBQ01, &on, (&legacy.0, 0), now()).await,
            CooldownVerdict::Allowed,
            "a zeroed column must release the ladder"
        );
        assert_eq!(
            verdict_for(&pool, USER_A, Category::Free, BBQ::BBQ01, &on, (&legacy.0, 0), now()).await,
            CooldownVerdict::Allowed,
            "a zeroed column must release Free"
        );

        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore]
    async fn an_operator_patch_reaches_a_player_with_a_stale_stored_row() {
        // The case that filled the live table: a real completion already left a row
        // behind, and the operator then edits the column. The stored row is older, so
        // the column must win — or the correction looks applied and does nothing.
        let pool = pool().await;
        cleanup(&pool).await;
        let on = CooldownPolicy::default().with_progression_sharing(true);

        // A ladder completion from two days ago: expired either way.
        record_group_completion_tx(
            &mut pool.acquire().await.unwrap(),
            USER_B,
            on.group_of(&Category::Bronze).encode(),
            Category::Bronze.encode() as i16,
            BBQ::BBQ01.encode() as i16,
            now() - 48 * 3600,
        )
        .await
        .expect("the old completion must be recorded");

        // Unpatched, the ladder is open again.
        let old = (legacy_code(Category::Bronze, BBQ::BBQ01), now() - 48 * 3600);
        assert_eq!(
            verdict_for(&pool, USER_B, Category::Bronze, BBQ::BBQ02, &on, (&old.0, old.1), now()).await,
            CooldownVerdict::Allowed,
            "a two-day-old run must not still be blocking"
        );

        // The operator pushes the cooldown forward via the column alone.
        let patched = (legacy_code(Category::Bronze, BBQ::BBQ01), now());
        match verdict_for(&pool, USER_B, Category::Bronze, BBQ::BBQ02, &on, (&patched.0, patched.1), now()).await {
            CooldownVerdict::Blocked { ends_at } => assert!(ends_at > now()),
            other => panic!("the operator patch must block the ladder, got {other:?}"),
        }

        cleanup(&pool).await;
    }
}