use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Base32, in plaintext — unlike a password, verifying a TOTP code means
    /// computing the current code from this, not comparing a hash. Set as
    /// soon as setup starts, before `totp_enabled` is true; see
    /// `routes::account::setup_totp`/`confirm_totp` for why the two aren't
    /// set together.
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    /// Hashed with `auth::hash_token`, not `hash_password` — high-entropy
    /// already, so Argon2's slowness buys nothing here and each login's
    /// worth checking several of these against isn't worth the extra cost.
    /// Consumed one at a time: redeeming a code removes it from this list.
    pub totp_recovery_codes: Vec<String>,
}
