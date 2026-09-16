use ravyn_core::{Invite, InviteId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct InviteRow {
    id: Uuid,
    token_hash: String,
    created_by: Uuid,
    created_at: OffsetDateTime,
    used_by: Option<Uuid>,
    used_at: Option<OffsetDateTime>,
}

impl From<InviteRow> for Invite {
    fn from(row: InviteRow) -> Self {
        Invite {
            id: InviteId(row.id),
            token_hash: row.token_hash,
            created_by: UserId(row.created_by),
            created_at: row.created_at,
            used_by: row.used_by.map(UserId),
            used_at: row.used_at,
        }
    }
}

impl Db {
    pub async fn create_invite(&self, invite: &Invite) -> Result<(), DbError> {
        sqlx::query(
            "insert into invites (id, token_hash, created_by, created_at)
             values ($1, $2, $3, $4)",
        )
        .bind(invite.id.0)
        .bind(&invite.token_hash)
        .bind(invite.created_by.0)
        .bind(invite.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_invites(&self) -> Result<Vec<Invite>, DbError> {
        let rows = sqlx::query_as::<_, InviteRow>("select * from invites order by created_at desc")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(Invite::from).collect())
    }

    /// Only matches an invite that hasn't been redeemed yet — an already-used
    /// token is treated the same as an unknown one.
    pub async fn get_unused_invite_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Invite>, DbError> {
        let row = sqlx::query_as::<_, InviteRow>(
            "select * from invites where token_hash = $1 and used_at is null",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Invite::from))
    }

    pub async fn mark_invite_used(&self, id: InviteId, used_by: UserId) -> Result<(), DbError> {
        sqlx::query("update invites set used_by = $2, used_at = now() where id = $1")
            .bind(id.0)
            .bind(used_by.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_invite(&self, id: InviteId) -> Result<(), DbError> {
        sqlx::query("delete from invites where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
