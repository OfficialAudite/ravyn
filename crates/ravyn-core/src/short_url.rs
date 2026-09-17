use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::UserId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShortUrlId(pub Uuid);

impl ShortUrlId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ShortUrlId {
    fn default() -> Self {
        Self::new()
    }
}

/// A shortened link to some other URL — same "give it a slug, own it,
/// share it" shape as a `File`, but pointing at a destination out on the
/// web instead of stored bytes. Deliberately minimal (no password, no
/// expiry, no tags): Zipline's own baseline shortener is this simple, and
/// the file side already covers those needs for anything hosted here.
#[derive(Debug, Clone)]
pub struct ShortUrl {
    pub id: ShortUrlId,
    pub owner_id: UserId,
    pub slug: String,
    pub destination: String,
    pub clicks: i64,
    pub created_at: OffsetDateTime,
}
