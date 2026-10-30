use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use super::config::Init;
use super::bitwise::BitwiseError;
use std::sync::OnceLock;

pub mod card;
pub mod account;
pub mod server;
pub mod gacha;
pub mod custom;
pub mod guild;

pub static DB_POOL: OnceLock<Pool<Postgres>> = OnceLock::new();

pub fn get_pool() -> &'static Pool<Postgres> {
    DB_POOL.get().expect("Critical: DB_POOL has not been initialized in main.rs!")
}

pub async fn init_pool(init: &Init) -> Result<(), sqlx::Error> {
    let missing = init.postgress.missing();
    if !missing.is_empty() {
        // Fail with the variable names rather than an opaque connection error.
        return Err(sqlx::Error::Configuration(
            format!(
                "Database settings missing from .env: {}. See .env.example.",
                missing.join(", ")
            )
            .into(),
        ));
    }

    let url = format!(
        "postgres://{}:{}@{}:{}/{}",
        init.postgress.user,
        urlencoding::encode(&init.postgress.password),
        init.postgress.host,
        init.postgress.port,
        init.postgress.database
    );
    
    let pool = PgPoolOptions::new()
        .max_connections(50)
        .min_connections(5)
        .connect(&url)
        .await?;
    
    DB_POOL.set(pool).expect("Critical: Failed to set global DB_POOL!");
    Ok(())
}

#[derive(Debug)]
pub struct PgConn<'a> {
    pub(crate) init: &'a Init,
    pub(crate) did: String,
    pub(crate) pool: Pool<Postgres>,
}

impl<'a> PgConn<'a> {
    pub async fn create(init: &'a Init, did: String) -> Result<PgConn<'a>, BitwiseError> {
        let global_pool = get_pool().clone();
        let pg = PgConn {
            init,
            did,
            pool: global_pool,
        };
        Ok(pg)
    }

    pub async fn close(&mut self) {}

    pub async fn re_connect(&mut self) -> Result<(), BitwiseError> {
        self.pool = get_pool().clone();
        Ok(())
    }

    pub async fn validate_character_exists(&self, char_id: i32) -> Result<bool, BitwiseError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM characters WHERE id = $1)"
        )
        .bind(char_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }
}