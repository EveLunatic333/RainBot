use sqlx::FromRow;
use super::{PgConn, BitwiseError};

#[derive(Debug, FromRow)]
pub struct Servers {
    pub name: String,
    pub cp: i32,
    pub land: i32,
    pub description: String,
}

impl<'a> PgConn<'a> {
    pub async fn get_server(&self) -> Result<Vec<Servers>, BitwiseError> {
        Ok(sqlx::query_as::<_, Servers>(
            "SELECT current_players AS cp, land, world_name AS name, world_description AS description FROM servers",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn psn(&self, psn: &str, id: i32) -> Result<(), BitwiseError> {
        sqlx::query("UPDATE users SET psn_id = $1 WHERE id = $2")
            .bind(psn)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}