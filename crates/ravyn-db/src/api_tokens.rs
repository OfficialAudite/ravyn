use ravyn_core::{ApiToken, ApiTokenId, User, UserId};
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
    totp_secret: Option<String>,
    totp_enabled: bool,
    totp_recovery_codes: Vec<String>,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: UserId(row.id),
            username: row.username,
            password_hash: row.password_hash,
            is_admin: row.is_admin,
            created_at: row.created_at,
            totp_secret: row.totp_secret,
            totp_enabled: row.totp_enabled,
            totp_recovery_codes: row.totp_recovery_codes,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ApiTokenRow {
    id: Uuid,
    token_hash: String,
    user_id: Uuid,
    name: String,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

impl From<ApiTokenRow> for ApiToken {
    fn from(row: ApiTokenRow) -> Self {
        ApiToken {
            id: ApiTokenId(row.id),
            token_hash: row.token_hash,
            user_id: UserId(row.user_id),
            name: row.name,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

impl Db {
    pub async fn create_api_token(&self, token: &ApiToken) -> Result<(), DbError> {
        sqlx::query(
            "insert into api_tokens (id, token_hash, user_id, name, created_at, last_used_at)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(token.id.0)
        .bind(&token.token_hash)
        .bind(token.user_id.0)
        .bind(&token.name)
        .bind(token.created_at)
        .bind(token.last_used_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_api_tokens_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<ApiToken>, DbError> {
        let rows = sqlx::query_as::<_, ApiTokenRow>(
            "select * from api_tokens where user_id = $1 order by created_at desc",
        )
        .bind(user_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(ApiToken::from).collect())
    }

    pub async fn get_api_token(&self, id: ApiTokenId) -> Result<Option<ApiToken>, DbError> {
        let row = sqlx::query_as::<_, ApiTokenRow>("select * from api_tokens where id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(ApiToken::from))
    }

    pub async fn delete_api_token(&self, id: ApiTokenId) -> Result<(), DbError> {
        sqlx::query("delete from api_tokens where id = $1")
            .bind(id.0)
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
