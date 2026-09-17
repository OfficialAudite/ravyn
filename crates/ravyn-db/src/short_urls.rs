use ravyn_core::{ShortUrl, ShortUrlId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct ShortUrlRow {
    id: Uuid,
    owner_id: Uuid,
    slug: String,
    destination: String,
    clicks: i64,
    created_at: OffsetDateTime,
}

impl From<ShortUrlRow> for ShortUrl {
    fn from(row: ShortUrlRow) -> Self {
        ShortUrl {
            id: ShortUrlId(row.id),
            owner_id: UserId(row.owner_id),
            slug: row.slug,
            destination: row.destination,
            clicks: row.clicks,
            created_at: row.created_at,
        }
    }
}

impl Db {
    pub async fn create_short_url(&self, short_url: &ShortUrl) -> Result<(), DbError> {
        sqlx::query(
            "insert into short_urls (id, owner_id, slug, destination, clicks, created_at)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(short_url.id.0)
        .bind(short_url.owner_id.0)
        .bind(&short_url.slug)
        .bind(&short_url.destination)
        .bind(short_url.clicks)
        .bind(short_url.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_short_url(&self, id: ShortUrlId) -> Result<Option<ShortUrl>, DbError> {
        let row = sqlx::query_as::<_, ShortUrlRow>("select * from short_urls where id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(ShortUrl::from))
    }

    pub async fn get_short_url_by_slug(&self, slug: &str) -> Result<Option<ShortUrl>, DbError> {
        let row = sqlx::query_as::<_, ShortUrlRow>("select * from short_urls where slug = $1")
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(ShortUrl::from))
    }

    pub async fn list_short_urls_for_owner(
        &self,
        owner_id: UserId,
    ) -> Result<Vec<ShortUrl>, DbError> {
        let rows = sqlx::query_as::<_, ShortUrlRow>(
            "select * from short_urls where owner_id = $1 order by created_at desc",
        )
        .bind(owner_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(ShortUrl::from).collect())
    }

    pub async fn increment_short_url_clicks(&self, id: ShortUrlId) -> Result<(), DbError> {
        sqlx::query("update short_urls set clicks = clicks + 1 where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_short_url(&self, id: ShortUrlId) -> Result<(), DbError> {
        sqlx::query("delete from short_urls where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
