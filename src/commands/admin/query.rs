use serenity::all::*;
use crate::{Components, MsgBundle, MyErr, PgConn};

pub struct Code {
    pub code: String,
}

impl Default for Code {
    fn default() -> Self {
        Code { code: String::new() }
    }
}

impl Code {
    fn new(cont: String) -> Result<Code, MyErr> {
        let first_whitespace = cont.find(|c: char| c.is_whitespace());
        let payload = match first_whitespace {
            Some(idx) => cont[idx..].trim(),
            None => return Err(MyErr::Custom("Missing SQL query payload.".to_string())),
        };

        let mut clean_query = payload;
        if clean_query.starts_with('`') {
            clean_query = clean_query.trim_start_matches('`');
            clean_query = clean_query.strip_prefix("sql").unwrap_or(clean_query);
        }
        if clean_query.ends_with('`') {
            clean_query = clean_query.trim_end_matches('`');
        }
        clean_query = clean_query.trim();

        if clean_query.is_empty() {
            return Err(MyErr::Custom("Cannot detect any SQL code in your message.".to_string()));
        }

        Ok(Code { code: clean_query.to_string() })
    }
}

async fn verify_admin(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    let gid = bnd.msg.guild_id
        .ok_or_else(|| MyErr::Custom("🛑 ACCESS DENIED: Command must be used in a server.".to_string()))?;

    let member = bnd.ctx.http.get_member(gid, bnd.msg.author.id).await
        .map_err(|_| MyErr::Custom("🛑 **ACCESS DENIED:** Failed to verify guild membership.".to_string()))?;

    let roles = bnd.ctx.http.get_guild_roles(gid).await
        .map_err(|_| MyErr::Custom("🛑 **ACCESS DENIED:** Failed to fetch server roles.".to_string()))?;

    let is_admin = member.roles.iter().any(|role_id| {
        roles.iter().any(|r| r.id == *role_id && r.permissions.administrator())
    });

    if !is_admin {
        if let Ok(guild) = bnd.ctx.http.get_guild(gid).await {
            if guild.owner_id != bnd.msg.author.id {
                return Err(MyErr::Custom("🛑 **ACCESS DENIED:** This command is strictly restricted to Server Administrators.".to_string()));
            }
        } else {
            return Err(MyErr::Custom("🛑 **ACCESS DENIED:** Failed to resolve guild ownership.".to_string()));
        }
    }

    Ok(())
}

#[hertz::hertz_msg(true)]
async fn msg(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    verify_admin(bnd).await?;
    let code = Code::new(bnd.msg.content.clone())?;
    let pg = PgConn::create(bnd.init, bnd.msg.author.id.to_string()).await?;
    pg.execute(&code.code).await?;
    Components::msg(bnd, "✅ Query executed successfully.").await?;
    Ok(())
}

#[hertz::hertz_msg(true)]
async fn qry(bnd: &MsgBundle<'_>) -> Result<(), MyErr> {
    verify_admin(bnd).await?;
    let code = Code::new(bnd.msg.content.clone())?;
    let pg = PgConn::create(bnd.init, bnd.msg.author.id.to_string()).await?;
    let table = pg.query(&code.code).await?;
    Components::msg(bnd, &table).await?;
    Ok(())
}