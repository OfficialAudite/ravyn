use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::UserId;

/// The plaintext API token, e.g. for a ShareX upload config. Only its hash is
/// ever persisted; this value is shown to the user once, at creation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTokenValue(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub token_hash: String,
    pub user_id: UserId,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}
