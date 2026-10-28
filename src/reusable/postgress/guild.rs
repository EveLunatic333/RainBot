use sqlx::{FromRow, Row};
use super::{BitwiseError, PgConn};
use crate::MyErr;
#[derive(FromRow, Debug, Clone)]
pub struct GuildList {
    pub id: i32,
    pub name: String,
}
#[derive(FromRow, Debug, Clone)]
pub struct Guild {
    pub id: i32,
    pub name: String,
    pub rank_rp: i32,
    pub leader_id: i32,
    pub created: chrono::DateTime<chrono::Utc>,
    pub lead_name: String,
    pub discord_id: Option<String>,
}
const MAX_GUILD_MEMBERS: i64 = 60;
impl PgConn<'_> {
    pub async fn guild_id(&self, cid: i32) -> Result<i64, BitwiseError> {
        let row = sqlx::query("SELECT guild_id FROM guild_characters WHERE character_id = $1")
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|d| d.get("guild_id")).unwrap_or(0))
    }
    
    async fn guild_count(&self, guild_id: i64) -> Result<i64, BitwiseError> {
        let row = sqlx::query("SELECT count(*) FROM guild_characters WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }

    pub async fn clear_guild_application(&self, char_id: i32) -> Result<(), BitwiseError> {
        sqlx::query("DELETE FROM guild_applications WHERE character_id = $1")
            .bind(char_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_char_id_by_discord(&self, discord_id: u64) -> Result<i32, BitwiseError> {
        let row = sqlx::query("SELECT char_id FROM discord WHERE discord_id = $1")
            .bind(discord_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("char_id")?)
    }

    pub async fn guild_food(
        &self,
        cid: i32,
        meal_id: i32,
        level: i32,
        duration_sec: i32,
    ) -> Result<bool, BitwiseError> {
        let gid = self.guild_id(cid).await?;
        if gid != 0 {
            sqlx::query(r#"
                INSERT INTO guild_meals (guild_id, meal_id, level, expires_in)
                VALUES ($1, $2, $3, CURRENT_TIMESTAMP + ($4::int * INTERVAL '1 second'))
                ON CONFLICT (guild_id, meal_id, level)
                DO UPDATE SET expires_in = GREATEST(guild_meals.expires_in, CURRENT_TIMESTAMP) + ($4::int * INTERVAL '1 second')
            "#)
            .bind(gid as i32)
            .bind(meal_id)
            .bind(level)
            .bind(duration_sec)
            .execute(&self.pool)
            .await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn guild_rp(&self, cid: i32, rp: i32) -> Result<bool, BitwiseError> {
        let gid = self.guild_id(cid).await?;
        if gid == 0 {
            return Ok(false);
        }
        sqlx::query("UPDATE guilds SET rank_rp = rank_rp + $1 WHERE id = $2")
            .bind(rp)
            .bind(gid as i32)
            .execute(&self.pool)
            .await?;
        Ok(true)
    }

    pub async fn guild_list(&self) -> Result<Vec<GuildList>, BitwiseError> {
        Ok(sqlx::query_as::<_, GuildList>("SELECT id, name FROM guilds")
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn guild_search(&self, guild_id: i64) -> Result<(Guild, i64), BitwiseError> {
        let guild = sqlx::query_as(
            "SELECT 
                guilds.name AS name, 
                characters.name AS lead_name, 
                guilds.id AS id, 
                rank_rp, 
                guilds.created_at AS created,   
                leader_id, 
                discord_id 
             FROM guilds 
             INNER JOIN characters ON characters.id = guilds.leader_id 
             LEFT OUTER JOIN discord ON discord.char_id = guilds.leader_id 
             WHERE guilds.id = $1",
        )
        .bind(guild_id)
        .fetch_one(&self.pool)
        .await?;
        
        let count = self.guild_count(guild_id).await?;
        Ok((guild, count))
    }

    pub async fn guild_join(&self, guild_id: i64, cid: i32) -> Result<(), MyErr> {
        if self.guild_id(cid).await? != 0 {
            return Err(MyErr::Custom(
                "You already belong to a guild. Leave your current guild to use this command. ".to_string(),
            ));
        }
        if self.guild_count(guild_id).await? >= MAX_GUILD_MEMBERS {
            return Err(MyErr::Custom("The guild you selected is already full. ".to_string()));
        }
        if let Err(why) = sqlx::query(
            "INSERT INTO guild_characters (guild_id, character_id, order_index) 
            VALUES ($1, $2, (SELECT COALESCE(max(order_index), 0) + 1 FROM guild_characters WHERE guild_id = $1))",
        )
        .bind(guild_id)
        .bind(cid)
        .execute(&self.pool)
        .await
        {
            return Err(BitwiseError::from(why).into());
        }
        Ok(())
    }

    pub async fn guild_leave(&self, cid: i32) -> Result<(), BitwiseError> {
        sqlx::query("DELETE FROM guild_characters WHERE character_id = $1")
            .bind(cid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}