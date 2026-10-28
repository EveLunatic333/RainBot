use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::MyErr;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Reminder {
    pub id: i32,
    pub reminder_type: String,
    pub role_id: Option<i64>,
    pub channel_id: i64,
    pub message: String,
    pub schedule_cron: Option<String>,
    pub interval_minutes: Option<i64>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_sent: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug)]
pub struct Scheduler {
    pub pool: PgPool,
    pub reminders: Arc<RwLock<HashMap<i32, Reminder>>>,
}

impl Scheduler {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            reminders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn init(&self) -> Result<(), MyErr> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS rotation_reminders (
                id SERIAL PRIMARY KEY,
                reminder_type VARCHAR(50) NOT NULL,
                role_id BIGINT,
                channel_id BIGINT NOT NULL,
                message TEXT NOT NULL,
                schedule_cron VARCHAR(50),
                interval_minutes BIGINT,
                is_active BOOLEAN DEFAULT true,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                last_sent TIMESTAMPTZ
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        self.load_reminders().await?;
        Ok(())
    }

    async fn load_reminders(&self) -> Result<(), MyErr> {
        let rows = sqlx::query_as::<_, Reminder>(
            r#"SELECT * FROM rotation_reminders WHERE is_active = true"#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut cache = self.reminders.write().await;
        cache.clear();
        for row in rows {
            cache.insert(row.id, row);
        }
        Ok(())
    }

    pub async fn add_reminder(
        &self,
        reminder_type: &str,
        role_id: Option<u64>,
        channel_id: u64,
        message: &str,
        interval_minutes: Option<i64>,
    ) -> Result<i32, MyErr> {
        let row = sqlx::query_as::<_, Reminder>(
            r#"
            INSERT INTO rotation_reminders 
            (reminder_type, role_id, channel_id, message, interval_minutes, is_active)
            VALUES ($1, $2, $3, $4, $5, true)
            RETURNING *
            "#,
        )
        .bind(reminder_type)
        .bind(role_id.map(|id| id as i64))
        .bind(channel_id as i64)
        .bind(message)
        .bind(interval_minutes)
        .fetch_one(&self.pool)
        .await?;

        let mut cache = self.reminders.write().await;
        cache.insert(row.id, row.clone());
        Ok(row.id)
    }

    pub async fn list_reminders(&self) -> Vec<Reminder> {
        let cache = self.reminders.read().await;
        let mut reminders: Vec<_> = cache.values().cloned().collect();
        reminders.sort_by_key(|r| r.id);
        reminders
    }

    pub async fn edit_reminder(
        &self,
        id: i32,
        message: Option<&str>,
        interval_minutes: Option<i64>,
        is_active: Option<bool>,
    ) -> Result<(), MyErr> {
        sqlx::query(
            "UPDATE rotation_reminders SET message = COALESCE($1, message), interval_minutes = COALESCE($2, interval_minutes), is_active = COALESCE($3, is_active) WHERE id = $4"
        )
        .bind(message)
        .bind(interval_minutes)
        .bind(is_active)
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.load_reminders().await?;
        Ok(())
    }

    pub async fn remove_reminder(&self, id: i32) -> Result<(), MyErr> {
        sqlx::query("DELETE FROM rotation_reminders WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        let mut cache = self.reminders.write().await;
        cache.remove(&id);
        Ok(())
    }

    pub async fn get_due_reminders(&self) -> Vec<Reminder> {
        let cache = self.reminders.read().await;
        let now = chrono::Utc::now();
        
        cache
            .values()
            .filter(|r| {
                r.is_active && 
                r.interval_minutes.map_or(false, |minutes| {
                    r.last_sent.map_or(true, |last| {
                        (now - last).num_minutes() >= minutes
                    })
                })
            })
            .cloned()
            .collect()
    }

    pub async fn mark_sent(&self, id: i32) -> Result<(), MyErr> {
        sqlx::query("UPDATE rotation_reminders SET last_sent = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        
        if let Some(reminder) = self.reminders.write().await.get_mut(&id) {
            reminder.last_sent = Some(chrono::Utc::now());
        }
        Ok(())
    }

    pub fn get_pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn get_reminders_arc(&self) -> Arc<RwLock<HashMap<i32, Reminder>>> {
        self.reminders.clone()
    }
}