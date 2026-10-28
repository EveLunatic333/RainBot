use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use crate::MyErr;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct WelcomeData {
    pub button_role_id: u64,
    pub guide_link: String,
    pub welcome_embed: EmbedData,
    pub dm_rules: Vec<EmbedData>,
    pub leave_embed: EmbedData,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct EmbedData {
    pub title: String,
    pub description: String,
    pub color: String,
    pub image_url: String,
    pub fields: Vec<EmbedField>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

impl Default for EmbedField {
    fn default() -> Self {
        EmbedField {
            name: "Field Name".to_string(),
            value: "Field Content".to_string(),
            inline: false,
        }
    }
}

impl Default for EmbedData {
    fn default() -> Self {
        EmbedData {
            title: "".to_string(),
            description: "".to_string(),
            color: "RANDOM".to_string(),
            image_url: "".to_string(),
            fields: vec![],
        }
    }
}

impl Default for WelcomeData {
    fn default() -> Self {
        WelcomeData {
            button_role_id: 0,
            guide_link: "".to_string(),
            welcome_embed: EmbedData::default(),
            dm_rules: vec![],
            leave_embed: EmbedData::default(),
        }
    }
}

impl WelcomeData {
    fn path() -> PathBuf {
        Path::new(".").join("static").join("welcome.json")
    }

    pub async fn new() -> Result<Self, MyErr> {
        let file = tokio::fs::read_to_string(Self::path()).await?;
        Ok(serde_json::from_str(&file)?)
    }

    pub async fn save(&self) -> Result<(), MyErr> {
        let file = serde_json::to_string_pretty(&self)?;
        Ok(tokio::fs::write(Self::path(), file.as_bytes()).await?)
    }

    pub async fn check(data: &[u8]) -> Result<(), MyErr> {
        let x = serde_json::from_slice::<Self>(data)?;
        x.save().await?;
        Ok(())
    }
}