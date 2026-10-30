use crate::SlashBundle;
use super::{Components, MyErr};
use serenity::all::*;
use super::bounty::{Bounty, BountyRefresh, BountyTitle, Category};
use serenity::async_trait;
use super::gacha::Gacha;
use super::market::{Market, Meal, Trading, Tag};
use super::welcome::WelcomeData;

impl Components {
    pub fn get_att(cmd: &CommandInteraction) -> Result<Attachment, MyErr> {
        let resolved: Vec<Attachment> = cmd.data.resolved.attachments.iter().map(|(_, v)| v.clone()).collect();
        match resolved.first() {
            Some(x) => Ok(x.clone()),
            None => Err(MyErr::Custom("cant get the attachment attachment".to_owned()))
        }
    }

    pub async fn json_config<T: MyConfig>(bnd: &SlashBundle<'_>, _tip: T) -> Result<(), MyErr> {
        let att = Components::get_att(bnd.cmd)?;
        let byte = att.download().await?;
        T::check(&byte).await?;
        T::update(bnd).await?;
        Ok(())
    }

    pub async fn bounty_config(bnd: &SlashBundle<'_>, category: &Category) -> Result<(), MyErr> {
        let att = Components::get_att(bnd.cmd)?;
        let byte = att.download().await?;
        Bounty::check(&byte, category).await?;
        Ok(())
    }
}

#[async_trait]
pub trait MyConfig {
    async fn check(data: &[u8]) -> Result<(), MyErr>;
    async fn update(bnd: &SlashBundle<'_>) -> Result<(), MyErr>;
}

#[async_trait]
impl MyConfig for BountyRefresh {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(BountyRefresh::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for BountyTitle {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(BountyTitle::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for Gacha {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(Gacha::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for Market {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(Market::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        // Uploading `market.json` no longer republishes a public board: the
        // "Server Market Stall" message was removed by request. The new stock is
        // picked up by the next `/trading market` invocation, which reads the file.
        Ok(())
    }
}

#[async_trait]
impl MyConfig for Meal {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(Meal::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for Trading {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(Trading::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for Tag {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(Tag::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        Ok(())
    }
}

#[async_trait]
impl MyConfig for WelcomeData {
    async fn check(data: &[u8]) -> Result<(), MyErr> {
        Ok(WelcomeData::check(data).await?)
    }
    async fn update(_bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(())
    }
}