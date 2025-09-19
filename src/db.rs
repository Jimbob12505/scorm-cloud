use anyhow::Result;
use sqlx::{Pool, Postgres};
use std::env;

pub type Db = Pool<Postgres>;

pub async fn connect() -> Result<Db> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL not set");
    Ok(Pool::<Postgres>::connect(&url).await?)
}

