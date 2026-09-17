use ravyn_core::{ActivityLogEntry, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct ActivityLogRow {
    id: Uuid,
    user_id: Option<Uuid>,
    username: String,
    action: String,
    target: Option<String>,
    created_at: OffsetDateTime,
}

impl From<ActivityLogRow> for ActivityLogEntry {
    fn from(row: ActivityLogRow) -> Self {
        ActivityLogEntry {
            id: row.id,
            user_id: row.user_id.map(UserId),
            username: row.username,
            action: row.action,
            target: row.target,
            created_at: row.created_at,
        }
    }
}

impl Db {
    /// Best-effort by convention at every call site (never awaited into a
    /// request's success/failure) - a missed log entry is a much smaller
    /// problem than an upload or login failing because logging it didn't
    /// work. `username` is a snapshot, not a live lookup, so the entry
    /// stays legible even if that account is later renamed or deleted.
    pub async fn log_activity(
        &self,
        user_id: Option<UserId>,
        username: &str,
        action: &str,
        target: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "insert into activity_log (id, user_id, username, action, target, created_at)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id.map(|id| id.0))
        .bind(username)
        .bind(action)
        .bind(target)
        .bind(OffsetDateTime::now_utc())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_recent_activity(&self, limit: i64) -> Result<Vec<ActivityLogEntry>, DbError> {
        let rows = sqlx::query_as::<_, ActivityLogRow>(
            "select * from activity_log order by created_at desc limit $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(ActivityLogEntry::from).collect())
    }
}
