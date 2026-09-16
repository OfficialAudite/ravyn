use ravyn_core::{ApiToken, User, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    password_hash: String,
    created_at: OffsetDateTime,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: UserId(row.id),
            username: row.username,
            password_hash: row.password_hash,
            created_at: row.created_at,
        }
    }
}

impl Db {
    pub async fn create_api_token(&self, token: &ApiToken) -> Result<(), DbError> {
        sqlx::query(
            "insert into api_tokens (token_hash, user_id, name, created_at, last_used_at)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(&token.token_hash)
        .bind(token.user_id.0)
        .bind(&token.name)
        .bind(token.created_at)
        .bind(token.last_used_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_user_by_api_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, UserRow>(
            "select users.* from api_tokens
             join users on users.id = api_tokens.user_id
             where api_tokens.token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(User::from))
    }

    pub async fn touch_api_token(&self, token_hash: &str) -> Result<(), DbError> {
        sqlx::query("update api_tokens set last_used_at = now() where token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
