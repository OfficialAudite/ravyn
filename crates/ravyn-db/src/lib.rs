mod api_tokens;
mod chunked_uploads;
mod error;
mod files;
mod folders;
mod invites;
mod pending_logins;
mod sessions;
mod settings;
mod short_urls;
mod users;

pub use error::DbError;

use sqlx::PgPool;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), DbError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}
