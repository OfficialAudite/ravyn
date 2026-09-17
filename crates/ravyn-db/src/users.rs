use ravyn_core::{EmbedSettings, User, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    password_hash: String,
    is_admin: bool,
    created_at: OffsetDateTime,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: UserId(row.id),
            username: row.username,
            password_hash: row.password_hash,
            is_admin: row.is_admin,
            created_at: row.created_at,
        }
    }
}

impl Db {
    pub async fn create_user(&self, user: &User) -> Result<(), DbError> {
        sqlx::query(
            "insert into users (id, username, password_hash, is_admin, created_at)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(user.id.0)
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(user.is_admin)
        .bind(user.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Whether any account exists yet — an unclaimed instance always lets
    /// the very next registration through, regardless of registration mode,
    /// since otherwise nobody could ever become the first admin.
    pub async fn has_any_users(&self) -> Result<bool, DbError> {
        let count: i64 = sqlx::query_scalar("select count(*) from users")
            .fetch_one(&self.pool)
            .await?;

        Ok(count > 0)
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, UserRow>("select * from users where username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(User::from))
    }

    pub async fn get_user_by_id(&self, id: UserId) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, UserRow>("select * from users where id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(User::from))
    }

    pub async fn list_users(&self) -> Result<Vec<User>, DbError> {
        let rows = sqlx::query_as::<_, UserRow>("select * from users order by created_at asc")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(User::from).collect())
    }

    /// Bytes currently stored across every file this user owns — computed
    /// on the fly rather than kept as a running counter, since a self-hosted
    /// instance's file count stays small enough that summing is cheap and
    /// this way it can never drift out of sync with reality.
    pub async fn get_storage_usage(&self, user_id: UserId) -> Result<i64, DbError> {
        // `sum()` over a bigint column comes back as `numeric` in Postgres,
        // not `bigint` — without the cast this fails to decode as `i64` and
        // (since callers reasonably treat "couldn't check usage" as 0 rather
        // than failing the request) silently reads as "nothing stored yet"
        // every time, which quietly defeats the whole quota check.
        let total: i64 = sqlx::query_scalar(
            "select coalesce(sum(size_bytes), 0)::bigint from files where owner_id = $1",
        )
        .bind(user_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(total)
    }

    /// `None` means unlimited — the default for every user until an admin
    /// sets one.
    pub async fn get_max_storage_bytes(&self, user_id: UserId) -> Result<Option<i64>, DbError> {
        let value: Option<i64> =
            sqlx::query_scalar("select max_storage_bytes from users where id = $1")
                .bind(user_id.0)
                .fetch_one(&self.pool)
                .await?;

        Ok(value)
    }

    pub async fn set_max_storage_bytes(
        &self,
        user_id: UserId,
        max_bytes: Option<i64>,
    ) -> Result<(), DbError> {
        sqlx::query("update users set max_storage_bytes = $2 where id = $1")
            .bind(user_id.0)
            .bind(max_bytes)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_password_hash(
        &self,
        user_id: UserId,
        password_hash: String,
    ) -> Result<(), DbError> {
        sqlx::query("update users set password_hash = $2 where id = $1")
            .bind(user_id.0)
            .bind(password_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_embed_settings(&self, user_id: UserId) -> Result<EmbedSettings, DbError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            embed_enabled: bool,
            embed_title: Option<String>,
            embed_description: Option<String>,
            embed_color: Option<String>,
            embed_site_name: Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            "select embed_enabled, embed_title, embed_description, embed_color, embed_site_name
             from users where id = $1",
        )
        .bind(user_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(EmbedSettings {
            enabled: row.embed_enabled,
            title: row.embed_title,
            description: row.embed_description,
            color: row.embed_color,
            site_name: row.embed_site_name,
        })
    }

    pub async fn set_embed_settings(
        &self,
        user_id: UserId,
        settings: &EmbedSettings,
    ) -> Result<(), DbError> {
        sqlx::query(
            "update users set embed_enabled = $2, embed_title = $3, embed_description = $4,
             embed_color = $5, embed_site_name = $6 where id = $1",
        )
        .bind(user_id.0)
        .bind(settings.enabled)
        .bind(&settings.title)
        .bind(&settings.description)
        .bind(&settings.color)
        .bind(&settings.site_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
