use time::OffsetDateTime;
use uuid::Uuid;

use crate::UserId;

/// One row of the instance's activity log - who did what, when. `username`
/// is a snapshot taken at the moment of the action, not a live join
/// against `users`, so the log stays readable by name even if that
/// account is later renamed or deleted (`user_id` alone would go stale).
#[derive(Debug, Clone)]
pub struct ActivityLogEntry {
    pub id: Uuid,
    pub user_id: Option<UserId>,
    pub username: String,
    pub action: String,
    pub target: Option<String>,
    pub created_at: OffsetDateTime,
}
