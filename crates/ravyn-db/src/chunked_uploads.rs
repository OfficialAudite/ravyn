use ravyn_core::{ChunkedUpload, ChunkedUploadId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct ChunkedUploadRow {
    id: Uuid,
    owner_id: Uuid,
    original_name: String,
    content_type: String,
    total_size: i64,
    created_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
    storage_key: Option<String>,
    multipart_id: Option<String>,
}

impl From<ChunkedUploadRow> for ChunkedUpload {
    fn from(row: ChunkedUploadRow) -> Self {
        ChunkedUpload {
            id: ChunkedUploadId(row.id),
            owner_id: UserId(row.owner_id),
            original_name: row.original_name,
            content_type: row.content_type,
            total_size: row.total_size,
            created_at: row.created_at,
            expires_at: row.expires_at,
            storage_key: row.storage_key,
            multipart_id: row.multipart_id,
        }
    }
}

/// One received chunk, as recorded in `chunked_upload_parts`. Has a
/// `storage_key` when stored as its own object, a `part_id` when it went
/// into the upload's multipart upload instead.
pub struct ChunkedUploadPart {
    pub part_number: i32,
    pub storage_key: Option<String>,
    pub part_id: Option<String>,
    pub size_bytes: i64,
}

impl Db {
    pub async fn create_chunked_upload(&self, upload: &ChunkedUpload) -> Result<(), DbError> {
        sqlx::query(
            "insert into chunked_uploads (id, owner_id, original_name, content_type, total_size, created_at, expires_at, storage_key, multipart_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(upload.id.0)
        .bind(upload.owner_id.0)
        .bind(&upload.original_name)
        .bind(&upload.content_type)
        .bind(upload.total_size)
        .bind(upload.created_at)
        .bind(upload.expires_at)
        .bind(&upload.storage_key)
        .bind(&upload.multipart_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_chunked_upload(
        &self,
        id: ChunkedUploadId,
    ) -> Result<Option<ChunkedUpload>, DbError> {
        let row =
            sqlx::query_as::<_, ChunkedUploadRow>("select * from chunked_uploads where id = $1")
                .bind(id.0)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(ChunkedUpload::from))
    }

    pub async fn delete_chunked_upload(&self, id: ChunkedUploadId) -> Result<(), DbError> {
        sqlx::query("delete from chunked_uploads where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// One row per part, replacing any previous attempt at the same
    /// `part_number` under this upload, so retrying a chunk with the same
    /// number after a partial write never leaves a stale duplicate.
    pub async fn upsert_chunked_upload_part(
        &self,
        upload_id: ChunkedUploadId,
        part_number: i32,
        storage_key: Option<&str>,
        part_id: Option<&str>,
        size_bytes: i64,
    ) -> Result<(), DbError> {
        sqlx::query(
            "insert into chunked_upload_parts (upload_id, part_number, storage_key, part_id, size_bytes)
             values ($1, $2, $3, $4, $5)
             on conflict (upload_id, part_number)
             do update set storage_key = excluded.storage_key, part_id = excluded.part_id,
                 size_bytes = excluded.size_bytes",
        )
        .bind(upload_id.0)
        .bind(part_number)
        .bind(storage_key)
        .bind(part_id)
        .bind(size_bytes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_chunked_upload_parts(
        &self,
        upload_id: ChunkedUploadId,
    ) -> Result<Vec<ChunkedUploadPart>, DbError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            part_number: i32,
            storage_key: Option<String>,
            part_id: Option<String>,
            size_bytes: i64,
        }

        let rows = sqlx::query_as::<_, Row>(
            "select part_number, storage_key, part_id, size_bytes from chunked_upload_parts
             where upload_id = $1 order by part_number asc",
        )
        .bind(upload_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ChunkedUploadPart {
                part_number: row.part_number,
                storage_key: row.storage_key,
                part_id: row.part_id,
                size_bytes: row.size_bytes,
            })
            .collect())
    }

    pub async fn list_chunked_uploads_for_owner(
        &self,
        owner_id: UserId,
    ) -> Result<Vec<ChunkedUpload>, DbError> {
        let rows = sqlx::query_as::<_, ChunkedUploadRow>(
            "select * from chunked_uploads where owner_id = $1",
        )
        .bind(owner_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(ChunkedUpload::from).collect())
    }

    /// Feeds the abandoned-upload sweep, same shape as
    /// `list_expired_files`: anything older than `cutoff`, regardless of
    /// owner, gets cleaned up rather than left taking up storage forever.
    pub async fn list_abandoned_chunked_uploads(
        &self,
        cutoff: OffsetDateTime,
    ) -> Result<Vec<ChunkedUpload>, DbError> {
        let rows = sqlx::query_as::<_, ChunkedUploadRow>(
            "select * from chunked_uploads where created_at < $1",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(ChunkedUpload::from).collect())
    }
}
