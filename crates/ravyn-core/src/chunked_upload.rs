use time::OffsetDateTime;
use uuid::Uuid;

use crate::UserId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkedUploadId(pub Uuid);

impl ChunkedUploadId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ChunkedUploadId {
    fn default() -> Self {
        Self::new()
    }
}

/// A large upload in progress, one chunk at a time, instead of one HTTP
/// request holding the whole file. Each chunk lands in storage under its
/// own temporary key as soon as it arrives, tracked in
/// `chunked_upload_parts`, so a chunk that fails only needs retrying
/// itself, not the whole file, and the server already knows how much has
/// arrived if the browser reloads mid-upload.
#[derive(Debug, Clone)]
pub struct ChunkedUpload {
    pub id: ChunkedUploadId,
    pub owner_id: UserId,
    pub original_name: String,
    pub content_type: String,
    pub total_size: i64,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
}
