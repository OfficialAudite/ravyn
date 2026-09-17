use time::OffsetDateTime;

use crate::UserId;

/// The gap between "username and password checked out" and "session
/// issued" for an account with 2FA enabled — created by `POST /login`
/// instead of a session, redeemed by `POST /login/totp` once the code
/// checks out. Short-lived and single-use, the same shape as `Session` and
/// `Invite`: only the token's hash is ever stored, and it's deleted the
/// moment it's redeemed (or expires).
#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub token_hash: String,
    pub user_id: UserId,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}
