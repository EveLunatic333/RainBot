use super::{BitwiseError, PgConn};
use sqlx::postgres::PgRow;
use sqlx::{Row, ValueRef, Decode, Postgres};
use sqlx::Column;

impl<'a> PgConn<'a> {
    pub async fn query(&self, qry: &str) -> Result<String, BitwiseError> {
        let fetch = sqlx::query(qry).fetch_all(&self.pool).await?;
        Ok(row_to_table(fetch)?)
    }

    pub async fn execute(&self, qry: &str) -> Result<(), BitwiseError> {
        sqlx::query(qry).execute(&self.pool).await?;
        Ok(())
    }
}

fn get_name_type(row: &PgRow) -> Result<String, BitwiseError> {
    let mut string = Vec::new();
    for i in row.columns() {
        let name = i.name();
        let typ = row.try_get_raw(i.ordinal())?;
        string.push("\t".to_string());
        string.push(format!("{}({})", name, typ.type_info().to_string()));
    }
    if string.is_empty() {
        return Ok(String::new());
    }
    Ok(string[1..].concat())
}

fn get_value(row: PgRow) -> Result<String, BitwiseError> {
    let mut string = Vec::new();
    for i in row.columns() {
        let value = row.try_get_raw(i.ordinal())?;
        let nam = value.type_info().to_string();
        let name = nam.as_str();
        let i32clust = vec!["INT", "SERIAL", "INT4"];
        let i64clust = vec!["BIGINT", "BIGSERIAL", "INT8"];
        let strclust = vec!["VARCHAR", "CHAR(N)", "TEXT", "NAME"];
        let boolclust = "BOOL";
        
        let val = if value.is_null() {
            "NULL".to_string()
        } else if i32clust.contains(&name) {
            format!("{}", <i32 as Decode<Postgres>>::decode(value).unwrap_or(0))
        } else if i64clust.contains(&name) {
            format!("{}", <i64 as Decode<Postgres>>::decode(value).unwrap_or(0))
        } else if strclust.contains(&name) {
            format!("{}", <&str as Decode<Postgres>>::decode(value).unwrap_or(""))
        } else if name == boolclust {
            format!("{}", <bool as Decode<Postgres>>::decode(value).unwrap_or(false))
        } else if name == "BYTEA" {
            "[bytea]".to_owned()
        } else {
            format!("{:?}", <&[u8] as Decode<Postgres>>::decode(value).unwrap_or(&[]))
        };
        
        string.push("\t".to_string());
        string.push(val);
    }
    if string.is_empty() {
        return Ok(String::new());
    }
    Ok(string[1..].concat())
}

fn row_to_table(row: Vec<PgRow>) -> Result<String, BitwiseError> {
    let name = match row.first() {
        Some(x) => get_name_type(x)?,
        None => return Err(BitwiseError::NoItem),
    };
    let mut res = vec!["`".to_string(), name];
    for pat in row {
        let data = get_value(pat)?;
        res.push("\n".to_string());
        res.push(data);
    }
    res.push("`".to_string());
    Ok(res.concat())
}