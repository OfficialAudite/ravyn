use ravyn_core::{User, UserId};
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
    pub async fn create_user(&self, user: &User) -> Result<(), DbError> {
        sqlx::query(
            "insert into users (id, username, password_hash, created_at) values ($1, $2, $3, $4)",
        )
        .bind(user.id.0)
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(user.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
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
}
