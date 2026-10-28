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
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn send_item(
        &self,
        data: &[ItemCode],
        cid: i32,
        name: &str,
        desc: &str,
    ) -> Result<(), BitwiseError> {
        if !self.validate_character_exists(cid).await? {
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
        .fetch_one(&self.pool)
        .await?
        .get("id");

        self.insert_distribution_items(dist_id, data).await?;
        Ok(())
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