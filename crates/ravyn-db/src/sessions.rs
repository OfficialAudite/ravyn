use ravyn_core::{Session, User, UserId};
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

impl Db {
    pub async fn create_session(&self, session: &Session) -> Result<(), DbError> {
        sqlx::query(
            "insert into sessions (token_hash, user_id, expires_at, created_at)
             values ($1, $2, $3, $4)",
        )
        .bind(&session.token_hash)
        .bind(session.user_id.0)
        .bind(session.expires_at)
        .bind(session.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Looks up the user for a live (non-expired) session token hash.
    pub async fn get_user_by_session_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<User>, DbError> {
        let row = sqlx::query_as::<_, UserRow>(
            "select users.* from sessions
             join users on users.id = sessions.user_id
             where sessions.token_hash = $1 and sessions.expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(User::from))
    }

    pub async fn delete_session(&self, token_hash: &str) -> Result<(), DbError> {
        sqlx::query("delete from sessions where token_hash = $1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
