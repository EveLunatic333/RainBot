use bcrypt::{hash, verify};
use chrono::NaiveDateTime;
use crate::commands::binded::transfer::FileSave;
use crate::reusable::utils::MyTime;
use std::fs::File;
use std::io::prelude::*;
use std::time::SystemTime;
use sqlx::{FromRow, Pool, Postgres};
use sqlx::Row;
use super::{BitwiseError, PgConn};

#[derive(Debug, FromRow)]
pub struct AccountData {
    pub id: i32,
    pub username: String,
}

#[derive(Debug, FromRow)]
pub struct SaveData {
    pub savedata: Option<Vec<u8>>,
    pub decomyset: Option<Vec<u8>>,
    pub hunternavi: Option<Vec<u8>>,
    pub otomoairou: Option<Vec<u8>>,
    pub partner: Option<Vec<u8>>,
    pub platedata: Option<Vec<u8>>,
    pub platebox: Option<Vec<u8>>,
    pub platemyset: Option<Vec<u8>>,
    pub rengokudata: Option<Vec<u8>>,
    pub savemercenary: Option<Vec<u8>>,
    pub skin_hist: Option<Vec<u8>>,
}

impl<'a> PgConn<'a> {
    pub async fn get_cid_from_discord(&self, discord_id: u64) -> Result<i32, sqlx::Error> {
        let did_str = discord_id.to_string();
        let row = sqlx::query("SELECT char_id FROM discord WHERE discord_id = $1")
            .bind(&did_str)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(r) = row {
            let cid: i32 = r.try_get("char_id")?;
            return Ok(cid);
        }
        let uid_row = sqlx::query("SELECT user_id FROM discord_register WHERE discord_id = $1")
            .bind(&did_str)
            .fetch_one(&self.pool)
            .await?;
        let uid: i32 = uid_row.try_get("user_id")?;
        let cid_row = sqlx::query("SELECT id FROM characters WHERE user_id = $1 ORDER BY id ASC LIMIT 1")
            .bind(uid)
            .fetch_one(&self.pool)
            .await?;
        let cid: i32 = cid_row.try_get("id")?;
        Ok(cid)
    }

    pub async fn check_user_password(&self, cid: i32, pass: &str) -> Result<bool, BitwiseError> {
        let row = sqlx::query("SELECT user_id FROM discord_register WHERE discord_id = $1")
            .bind(&self.did)
            .fetch_optional(&self.pool)
            .await?;
        let uid: i32 = match row {
            Some(r) => r.try_get("user_id")?,
            None => {
                let char_row = sqlx::query("SELECT user_id FROM characters WHERE id = $1")
                    .bind(cid)
                    .fetch_one(&self.pool)
                    .await?;
                char_row.try_get("user_id")?
            }
        };
        let hash_str: String = sqlx::query("SELECT password FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
            .try_get("password")?;
        Ok(verify(pass, &hash_str).unwrap_or_default())
    }

    pub async fn check_user_password_by_username(&self, username: &str, pass: &str) -> Result<bool, BitwiseError> {
        let hash_str: String = sqlx::query("SELECT password FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(&self.pool)
            .await?
            .try_get("password")?;
        Ok(verify(pass, &hash_str).unwrap_or_default())
    }

    pub async fn is_username_bound_to_other(&self, username: &str, current_discord_id: &str) -> Result<bool, BitwiseError> {
        let result: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM users u
                JOIN discord_register dr ON u.id = dr.user_id
                WHERE u.username = $1 AND dr.discord_id != $2
            )"
        )
        .bind(username)
        .bind(current_discord_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(result)
    }

    pub async fn get_password_hash(&self, account_id: i32) -> Result<String, BitwiseError> {
        let hash: String = sqlx::query_scalar(
            "SELECT password FROM users WHERE id = $1"
        )
        .bind(account_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(hash)
    }

    pub async fn is_account_bound_to_user(&self, account_id: i32, discord_id: &str) -> Result<bool, BitwiseError> {
        let result: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM discord_register WHERE user_id = $1 AND discord_id != $2)"
        )
        .bind(account_id)
        .bind(discord_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(result)
    }

    pub async fn change_user_password(&self, pass: &str, cid: i32) -> Result<(), BitwiseError> {
        let row = sqlx::query("SELECT user_id FROM discord_register WHERE discord_id = $1")
            .bind(&self.did)
            .fetch_optional(&self.pool)
            .await?;
        let uid: i32 = match row {
            Some(r) => r.try_get("user_id")?,
            None => {
                let char_row = sqlx::query("SELECT user_id FROM characters WHERE id = $1")
                    .bind(cid)
                    .fetch_one(&self.pool)
                    .await?;
                char_row.try_get("user_id")?
            }
        };
        let hashed = bcrypt::hash(pass, 10).unwrap_or_default();
        sqlx::query("UPDATE users SET password = $1 WHERE id = $2")
            .bind(&hashed)
            .bind(uid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn change_password_with_username(&self, pass: &str, username: &str) -> Result<(), BitwiseError> {
        let hashed = bcrypt::hash(pass, 10).unwrap_or_default();
        sqlx::query("UPDATE users SET password = $1 WHERE username = $2")
            .bind(hashed)
            .bind(username)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_account(&self, user: &str, pass: &str, reg: bool) -> Result<AccountData, BitwiseError> {
        let uid;
        if reg {
            uid = match create_account(&self.pool, user, pass).await {
                Ok(acc) => acc,
                Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505") => {
                    return Err(BitwiseError::DuplicateUser);
                }
                Err(e) => return Err(e.into()),
            };
        } else {
            uid = sqlx::query_as::<_, AccountData>("SELECT id, username FROM users WHERE username = $1")
                .bind(user)
                .fetch_one(&self.pool)
                .await?;

            let is_valid = self.check_user_password_by_username(user, pass).await?;
            if !is_valid {
                return Err(BitwiseError::InvalidPassword);
            }

            let is_bound = self.is_username_bound_to_other(user, &self.did).await?;
            if is_bound {
                return Err(BitwiseError::AlreadyBound);
            }
        }
        use_history(&self.pool, &self.did, uid.id as i64).await?;
        Ok(uid)
    }

    pub async fn switch(&self, cid: i32) -> Result<(), BitwiseError> {
        let is_owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM characters c
                JOIN users u ON c.user_id = u.id
                JOIN discord_register dr ON u.id = dr.user_id
                WHERE c.id = $1 AND dr.discord_id = $2
            )"
        )
        .bind(cid)
        .bind(&self.did)
        .fetch_one(&self.pool)
        .await?;
        
        if !is_owned {
            return Err(BitwiseError::CharacterNotOwned);
        }
        
        sqlx::query("INSERT INTO discord (discord_id, char_id, gacha) VALUES ($1, $2, 100) ON CONFLICT (discord_id) DO UPDATE SET char_id = $2")
            .bind(&self.did)
            .bind(cid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn reset_cd(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE discord SET transfercd = 0 WHERE discord_id = $1")
            .bind(&self.did)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn send_save(&self, cid: i32) -> Result<SaveData, BitwiseError> {
        Ok(sqlx::query_as("SELECT * FROM characters WHERE id = $1")
            .bind(cid)
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn transfer_cd(&self) -> Result<(bool, i64), sqlx::Error> {
        let cd: i64 = sqlx::query("SELECT transfercd FROM discord WHERE discord_id = $1")
            .bind(&self.did)
            .fetch_one(&self.pool)
            .await?
            .try_get("transfercd")?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if now > cd as u64 {
            let next_transfer = now + 24 * 60 * 60;
            sqlx::query("UPDATE discord SET transfercd = $2 WHERE discord_id = $1")
                .bind(&self.did)
                .bind(next_transfer as i64)
                .execute(&self.pool)
                .await?;
            return Ok((true, next_transfer as i64));
        }
        Ok((false, cd))
    }

    pub async fn transfer_file(&self, file: &FileSave, cid: i32) -> Result<(), BitwiseError> {
        sqlx::query(&format!("UPDATE characters SET {} = $1 WHERE id = $2", &file.name))
            .bind(file.bin.as_slice())
            .bind(cid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn purge(&self) -> Result<(), BitwiseError> {
        sqlx::query("DELETE FROM discord_register WHERE discord_id = $1")
            .bind(&self.did)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

impl SaveData {
    fn to_file(&self) -> Result<(), std::io::Error> {
        if let Some(x) = &self.savedata {
            let mut y = File::create("./save/savefile.bin")?;
            y.write_all(x.as_slice())?;
        }
        Ok(())
    }
}

fn get_naive() -> Option<NaiveDateTime> {
    let amonth = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() + 30 * 24 * 60 * 60;
    #[allow(deprecated)]
    NaiveDateTime::from_timestamp_opt(amonth as i64, 0)
}

async fn create_account(pool: &Pool<Postgres>, user: &str, pass: &str) -> Result<AccountData, sqlx::Error> {
    let hashed = hash(pass, 10).unwrap_or_default();
    let time = get_naive().unwrap_or_default();
    let account = sqlx::query_as::<_, AccountData>(
        "INSERT INTO users (username, password, return_expires) VALUES ($1, $2, $3) RETURNING id, username",
    )
    .bind(user)
    .bind(&hashed)
    .bind(time)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        "INSERT INTO characters (user_id, is_female, is_new_character, name, unk_desc_string, hrp, gr, weapon_type, last_login) VALUES($1, False, True, '', '', 0, 0, 0, $2)",
    )
    .bind(account.id)
    .bind(MyTime::now() as i32)
    .execute(pool)
    .await?;
    Ok(account)
}

async fn use_history(pool: &Pool<Postgres>, did: &str, uid: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO discord_register (discord_id, user_id) VALUES ($1, $2)")
        .bind(did)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(())
}