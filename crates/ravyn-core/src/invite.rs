use time::OffsetDateTime;
use uuid::Uuid;

use crate::UserId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InviteId(pub Uuid);

impl InviteId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for InviteId {
    fn default() -> Self {
        Self::new()
    }
}

/// A single-use registration code an admin hands out when the instance is
/// in invite-only mode. Only its hash is ever stored, same as a session or
/// API token — the raw code is shown once, at creation.
#[derive(Debug, Clone)]
pub struct Invite {
    pub id: InviteId,
    pub token_hash: String,
    pub created_by: UserId,
    pub created_at: OffsetDateTime,
    pub used_by: Option<UserId>,
    pub used_at: Option<OffsetDateTime>,
}
