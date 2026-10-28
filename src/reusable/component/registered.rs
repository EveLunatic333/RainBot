use std::time::Duration;
use serenity::all::{User, Message};
use serenity::futures::StreamExt;
use serenity::prelude::Context;
use crate::{Init, MyErr, Mytrait, PgConn, Components, Mybundle, reusable::postgress::card::UserData};
use crate::reusable::component::error::ErrorLog;

/// Ends the character picker quietly when the failure is just an expired
/// interaction (Discord codes 10062/40060); any real error still propagates.
async fn benign_exit(me: MyErr, msg: &Message, ctx: &Context) -> Result<(), MyErr> {
    if !crate::reusable::component::new_error::is_benign_interaction_error(&me) {
        return Err(me);
    }
    tracing::warn!("Character picker interaction expired (10062/40060); closing quietly.");
    let _ = msg.delete(&ctx.http).await;
    Ok(())
}

pub struct Reg<'a> {
    pub pg: PgConn<'a>,
    pub cid: i32,
}

impl<'a> Reg<'a> {
    async fn fire_error<T: Mytrait>(ctx: &Context, cmd: &T, init: &Init, user: &User, msg: &str) {
        let mut err_log = ErrorLog::new(ctx, init, user).await;
        let my_err = MyErr::Custom(msg.to_string());
        my_err.log(cmd, "Auth Check", true, &mut err_log).await;
    }

    async fn is_purged(pg: &PgConn<'_>, did: &str) -> bool {
        let result: Result<bool, _> = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM discord_register WHERE discord_id = $1)",
        )
        .bind(did)
        .fetch_one(&pg.pool)
        .await;

        match result {
            Ok(exists) => !exists,
            Err(why) => {
                tracing::warn!(error = ?why, "Auth check query failed");
                true
            }
        }
    }

    async fn select_card<T: Mytrait>(
        &self,
        data: UserData,
        ctx: &'a Context,
        cmd: &'a T,
        init: &'a Init,
    ) -> Result<(), MyErr> {
        let user = cmd.user();
        let card = self.pg.many_card(data.rid).await?;

        if card.is_empty() {
            let msg = "You don't have any character on your account, please make one on the game launcher and use it to enter town first before using this command again.";
            Self::fire_error(ctx, cmd, init, &user, msg).await;
            return Err(MyErr::Custom(msg.to_string()));
        }

        let mut index = 0;
        cmd.response(ctx, card[index].bind(&user)).await?;
        let msg = cmd.get_msg(ctx).await?;
        let mut reply = msg
            .await_component_interactions(ctx)
            .timeout(Duration::from_secs(60 * 3))
            .stream();

        while let Some(pat) = reply.next().await {
            let id = &pat.data.custom_id;
            if user != pat.user {
                continue;
            }
            if id == "next" {
                index += 1;
                if index == card.len() {
                    index = 0;
                }
                if let Err(e) = cmd.edit(ctx, card[index].edit_bind(&user)).await {
                    return benign_exit(e, &msg, ctx).await;
                }
                if let Err(e) = pat.defer(&ctx.http).await {
                    return benign_exit(MyErr::from(e), &msg, ctx).await;
                }
            } else if id == "use" {
                self.pg.switch(card[index].char_id).await?;
                if let Err(e) = pat
                    .create_response(
                        &ctx.http,
                        Components::interaction_response("Successfully switched main character!", true),
                    )
                    .await
                {
                    // The character switch already succeeded at this point;
                    // only the confirmation may be lost to an expired click.
                    return benign_exit(MyErr::from(e), &msg, ctx).await;
                }
                break;
            }
        }
        let _ = msg.delete(&ctx.http).await;

        Ok(())
    }

    pub async fn switch<T: Mytrait>(
        ctx: &'a Context,
        cmd: &'a T,
        init: &'a Init,
        bypass: bool,
        _ephemeral: bool,
    ) -> Result<Option<Reg<'a>>, MyErr> {
        let user = cmd.user();
        let pg = PgConn::create(init, user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let msg = format!(
                "Account for {} has been purged or not found. Please use `/bind` to register your account.",
                user.name
            );
            Self::fire_error(ctx, cmd, init, &user, &msg).await;
            return Err(MyErr::Custom(msg));
        }

        let data = pg.get_user_data_long().await?;

        if data.cid != 0 && !bypass {
            return Ok(Some(Reg { pg, cid: data.cid }));
        } else if data.rid == 0 && data.cid == 0 {
            let msg = "You don't have any account in this server. Please create an account with `/create` or bind an existing one with `/bind`.";
            Self::fire_error(ctx, cmd, init, &user, msg).await;
            return Err(MyErr::Custom(msg.to_string()));
        }

        let x = Self { pg, cid: data.cid };
        x.select_card(data, ctx, cmd, init).await?;
        Ok(None)
    }

    pub async fn check<T: Mybundle>(bnd: &'a T, user: &'a User) -> Result<Reg<'a>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let msg = format!(
                "Account for {} has been purged or not found. Please use `/bind` to register your account.",
                user.name
            );
            Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
            return Err(MyErr::Custom(msg));
        }

        let data = pg.get_user_data().await?;

        if data.cid == 0 {
            if data.rid == 0 {
                let msg = format!(
                    "{} doesn't have an account in this server yet, use `/create` to create a new account or bind an existing account with `/bind`.",
                    user.name
                );
                Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
                return Err(MyErr::Custom(msg));
            } else {
                let msg = format!(
                    "{} already has an account but hasn't selected a main character. Tell them to use `/switch`.",
                    user.name
                );
                Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
                return Err(MyErr::Custom(msg));
            }
        }
        Ok(Reg { pg, cid: data.cid })
    }

    pub async fn check_user<T: Mybundle>(bnd: &'a T, user: &'a User) -> Result<Reg<'a>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let msg = format!(
                "Account for {} has been purged or not found. Please use `/bind` to register your account.",
                user.name
            );
            Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
            return Err(MyErr::Custom(msg));
        }

        let data = pg.get_user_data_long().await?;

        if data.rid == 0 {
            let msg = format!(
                "{} doesn't have an account in this server yet, use `/create` to create a new account or bind an existing account with `/bind`.",
                user.name
            );
            Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
            return Err(MyErr::Custom(msg));
        }
        Ok(Reg { pg, cid: data.cid })
    }

    pub async fn reverse_check<T: Mybundle>(
        bnd: &'a T,
        user: &'a User,
    ) -> Result<Option<Reg<'a>>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let x = Reg { pg, cid: 0 };
            return Ok(Some(x));
        }

        let data = pg.get_user_data_long().await?;
        let x = Reg { pg, cid: data.cid };

        if data.rid != 0 {
            x.select_card(data, bnd.ctx(), bnd.cmd(), bnd.init()).await?;
            return Ok(None);
        }
        Ok(Some(x))
    }

    pub async fn reverse_check_alter<T: Mybundle>(
        bnd: &'a T,
        user: &'a User,
    ) -> Result<Option<Reg<'a>>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let x = Reg { pg, cid: 0 };
            return Ok(Some(x));
        }

        let data = pg.get_user_data_long().await?;
        let x = Reg { pg, cid: data.rid };

        if data.rid != 0 {
            x.select_card(data, bnd.ctx(), bnd.cmd(), bnd.init()).await?;
            return Ok(None);
        }
        Ok(Some(x))
    }

    pub async fn only_check_alter<T: Mybundle>(bnd: &'a T, user: &'a User) -> Result<Reg<'a>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;

        if Self::is_purged(&pg, &user.id.to_string()).await {
            let msg = format!(
                "Account for {} has been purged or not found. Please use `/bind` to register your account.",
                user.name
            );
            Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
            return Err(MyErr::Custom(msg));
        }

        let data = pg.get_user_data_long().await?;
        let x = Reg { pg, cid: data.rid };

        if data.rid == 0 {
            let msg = format!(
                "{} doesn't have an account in this server yet, use `/create` to create a new account or bind an existing account with `/bind`.",
                user.name
            );
            Self::fire_error(bnd.ctx(), bnd.cmd(), bnd.init(), user, &msg).await;
            return Err(MyErr::Custom(msg));
        }
        Ok(x)
    }

    pub async fn no_check<T: Mybundle>(bnd: &'a T, user: &'a User) -> Result<Reg<'a>, MyErr> {
        let pg = PgConn::create(bnd.init(), user.id.to_string()).await?;
        let x = Reg { pg, cid: 0 };
        Ok(x)
    }
}