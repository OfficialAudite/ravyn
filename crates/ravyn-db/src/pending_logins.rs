use ravyn_core::{PendingLogin, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

impl Db {
    pub async fn create_pending_login(&self, login: &PendingLogin) -> Result<(), DbError> {
        sqlx::query(
            "insert into pending_logins (token_hash, user_id, created_at, expires_at)
             values ($1, $2, $3, $4)",
        )
        .bind(&login.token_hash)
        .bind(login.user_id.0)
        .bind(login.created_at)
        .bind(login.expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Only returns a still-live one — an expired row is treated the same
    /// as no row at all, same as an expired session.
    pub async fn get_pending_login(
        &self,
        token_hash: &str,
    ) -> Result<Option<PendingLogin>, DbError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            token_hash: String,
            user_id: Uuid,
            created_at: OffsetDateTime,
            expires_at: OffsetDateTime,
        }

        let row = sqlx::query_as::<_, Row>(
            "select * from pending_logins where token_hash = $1 and expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| PendingLogin {
            token_hash: row.token_hash,
            user_id: UserId(row.user_id),
            created_at: row.created_at,
            expires_at: row.expires_at,
        }))
    }

    pub async fn delete_pending_login(&self, token_hash: &str) -> Result<(), DbError> {
        sqlx::query("delete from pending_logins where token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
