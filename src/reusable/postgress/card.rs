use sqlx::FromRow;
use super::{BitwiseError, PgConn};

#[derive(Debug, Clone, FromRow)]
pub struct Card {
    pub char_id: i32,
    pub user_id: i64,
    pub name: String,
    pub gr: i32,
    pub hrp: i32,
    pub login: i32,
    pub weapon_type: i32,
    pub username: String,
    pub guild_id: Option<i64>,
    pub guild_name: Option<String>,
    pub gsr: i32,
    pub playtime: i32,
}

impl Default for Card {
    fn default() -> Self {
        Self {
            char_id: 0,
            user_id: 0,
            username: "deleted".to_string(),
            name: "deleted".to_string(),
            login: 0,
            hrp: 0,
            gr: 0,
            guild_name: None,
            guild_id: None,
            weapon_type: 0,
            gsr: 0,
            playtime: 0,
        }
    }
}

#[derive(FromRow, Clone, Debug)]
pub struct Event {
    pub bounty: i32,
    pub gacha: i32,
    pub pity: i32,
    pub latest_bounty: String,
    pub latest_bounty_time: i64,
    pub title: i32,
    pub bronze: i32,
    pub silver: i32,
    pub gold: i32,
    pub name: String,
    pub char_id: i32,
    pub zeny: i32,
    pub gacha_premium: i32,
    pub meal_expiry: i64,
    pub meal_id: i32,
}

#[derive(Debug, FromRow)]
pub struct UserData {
    pub cid: i32,
    pub rid: i32,
}

impl<'a> PgConn<'a> {
    pub async fn get_event(&self) -> Result<Event, BitwiseError> {
        self.get_target_event(&self.did).await
    }

    pub async fn get_target_event(&self, target_did: &str) -> Result<Event, BitwiseError> {
        let query = r#"
            SELECT 
                ch.name AS name,
                ch.id AS char_id,
                d.bounty,
                d.gacha,
                d.pity,
                d.latest_bounty,
                d.latest_bounty_time,
                d.title,
                d.bronze,
                d.silver,
                d.gold,
                0 AS zeny,
                COALESCE(u.gacha_premium, 0) AS gacha_premium,
                COALESCE(gm.meal_expiry, 0) AS meal_expiry,
                COALESCE(gm.meal_id, 0) AS meal_id
            FROM discord d
            INNER JOIN characters ch ON d.char_id = ch.id
            INNER JOIN users u ON ch.user_id = u.id
            LEFT OUTER JOIN guild_characters gc ON ch.id = gc.character_id
            LEFT JOIN LATERAL (
                SELECT EXTRACT(EPOCH FROM expires_in)::BIGINT AS meal_expiry, meal_id
                FROM guild_meals
                WHERE guild_id = gc.guild_id
                ORDER BY expires_in DESC
                LIMIT 1
            ) gm ON TRUE
            WHERE d.discord_id = $1
        "#;
        Ok(sqlx::query_as::<_, Event>(query)
            .bind(target_did)
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn get_user(&self) -> Result<(i32, String), BitwiseError> {
        let query = r#"
            SELECT dr.user_id, COALESCE(u.username, 'unknown') AS username
            FROM discord_register dr
            LEFT OUTER JOIN users u ON dr.user_id = u.id
            WHERE dr.discord_id = $1
        "#;
        let row = sqlx::query_as::<_, (i32, String)>(query)
            .bind(&self.did)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.unwrap_or((0, String::new())))
    }

    pub(super) async fn get_char_id(&self) -> Result<i32, BitwiseError> {
        let query = "SELECT char_id FROM discord WHERE discord_id = $1";
        let row = sqlx::query_scalar::<_, i32>(query)
            .bind(&self.did)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.unwrap_or(0))
    }

    async fn already_create(&self) -> Result<i32, BitwiseError> {
        let query = "SELECT user_id FROM discord_register WHERE discord_id = $1";
        let row = sqlx::query_scalar::<_, i32>(query)
            .bind(&self.did)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.unwrap_or(0))
    }

    async fn get_all_cid(&self, uid: i32) -> Result<Vec<i32>, BitwiseError> {
        let query = "SELECT id FROM characters WHERE user_id = $1";
        let rows = sqlx::query_scalar::<_, i32>(query)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get_user_data(&self) -> Result<UserData, BitwiseError> {
        let cid = self.get_char_id().await?;
        if cid == 0 {
            return Ok(UserData { cid: 0, rid: self.already_create().await? });
        }
        Ok(UserData { cid, rid: 0 })
    }

    pub async fn get_user_data_long(&self) -> Result<UserData, BitwiseError> {
        let cid = self.get_char_id().await?;
        let rid = self.already_create().await?;
        if cid != 0 && rid == 0 {
            let uid_query = "SELECT user_id FROM characters WHERE id = $1";
            let uid = sqlx::query_scalar::<_, i32>(uid_query)
                .bind(cid)
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or(0);
            if uid != 0 {
                sqlx::query(
                    "INSERT INTO discord_register (discord_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(&self.did)
                .bind(uid)
                .execute(&self.pool)
                .await?;
                return Ok(UserData { cid, rid: uid });
            }
        }
        Ok(UserData { cid, rid })
    }

    pub async fn many_card(&self, user: i32) -> Result<Vec<Card>, BitwiseError> {
        let cid = self.get_all_cid(user).await?;
        if cid.is_empty() {
            return Ok(Vec::new());
        }
        let query = r#"
            SELECT 
                c.id AS char_id,
                c.user_id,
                c.name AS name,
                c.gr,
                c.hrp,
                c.weapon_type,
                c.last_login AS login,
                u.username,
                gc.guild_id,
                g.name AS guild_name,
                COALESCE(c.play_time, 0) AS playtime,
                0 AS gsr
            FROM characters c
            INNER JOIN users u ON c.user_id = u.id
            LEFT OUTER JOIN guild_characters gc ON c.id = gc.character_id
            LEFT OUTER JOIN guilds g ON gc.guild_id = g.id
            WHERE c.id = ANY($1)
        "#;
        Ok(sqlx::query_as::<_, Card>(query)
            .bind(&cid)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn get_card(&self, cid: i32) -> Result<Card, BitwiseError> {
        let query = r#"
            SELECT 
                c.id AS char_id,
                c.user_id,
                c.name AS name,
                c.gr,
                c.hrp,
                c.weapon_type,
                c.last_login AS login,
                u.username,
                gc.guild_id,
                g.name AS guild_name,
                COALESCE(c.play_time, 0) AS playtime,
                0 AS gsr
            FROM characters c
            INNER JOIN users u ON c.user_id = u.id
            LEFT OUTER JOIN guild_characters gc ON c.id = gc.character_id
            LEFT OUTER JOIN guilds g ON gc.guild_id = g.id
            WHERE c.id = $1
        "#;
        Ok(sqlx::query_as::<_, Card>(query)
            .bind(cid)
            .fetch_one(&self.pool)
            .await?)
    }
}