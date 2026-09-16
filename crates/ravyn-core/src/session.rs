use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::UserId;

/// The plaintext session token. Only its hash is ever persisted; this value
/// exists just long enough to hand to the client as a cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub token_hash: String,
    pub user_id: UserId,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
