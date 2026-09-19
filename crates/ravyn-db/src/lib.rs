mod activity;
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

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// `sqlx`'s own defaults leave a connection idle for up to 10 minutes
    /// before recycling it - long enough that a NAT, conntrack table, or
    /// Docker's own network stack can silently drop it first without
    /// either side noticing. The next query on that connection then hangs
    /// waiting on a TCP-level timeout instead of failing fast, which is
    /// exactly what a self-hosted instance sitting quiet overnight and
    /// then "hanging" on the next visit looks like. Recycling well before
    /// any of those infra-level timeouts would fire (a few minutes,
    /// nothing this app's traffic pattern would ever notice) means a
    /// request only ever gets a connection sqlx knows for certain is
    /// fresh.
    pub async fn connect(database_url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .min_connections(1)
            .idle_timeout(Duration::from_secs(3 * 60))
            .max_lifetime(Duration::from_secs(30 * 60))
            .test_before_acquire(true)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), DbError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    /// A cheap round trip against a real connection, for `/health` to
    /// call - the difference between "the process is up" and "the
    /// process can actually still talk to Postgres", which a plain
    /// liveness check can't tell apart from the exact class of stale-pool
    /// hang `connect`'s own tuning above is guarding against.
    pub async fn ping(&self) -> Result<(), DbError> {
        sqlx::query("select 1").execute(&self.pool).await?;
        Ok(())
    }
}
