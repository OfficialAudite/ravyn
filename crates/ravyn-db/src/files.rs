use ravyn_core::{File, FileId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct FileRow {
    id: Uuid,
    owner_id: Uuid,
    original_name: String,
    storage_key: String,
    content_type: String,
    size_bytes: i64,
    sha256: String,
    created_at: OffsetDateTime,
}

impl From<FileRow> for File {
    fn from(row: FileRow) -> Self {
        File {
            id: FileId(row.id),
            owner_id: UserId(row.owner_id),
            original_name: row.original_name,
            storage_key: row.storage_key,
            content_type: row.content_type,
            size_bytes: row.size_bytes as u64,
            sha256: row.sha256,
            created_at: row.created_at,
        }
    }
}

impl Db {
    pub async fn insert_file(&self, file: &File) -> Result<(), DbError> {
        sqlx::query(
            "insert into files (id, owner_id, original_name, storage_key, content_type, size_bytes, sha256, created_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(file.id.0)
        .bind(file.owner_id.0)
        .bind(&file.original_name)
        .bind(&file.storage_key)
        .bind(&file.content_type)
        .bind(file.size_bytes as i64)
        .bind(&file.sha256)
        .bind(file.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_file(&self, id: FileId) -> Result<Option<File>, DbError> {
        let row = sqlx::query_as::<_, FileRow>("select * from files where id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(File::from))
    }

    pub async fn list_files_for_owner(&self, owner_id: UserId) -> Result<Vec<File>, DbError> {
        let rows = sqlx::query_as::<_, FileRow>(
            "select * from files where owner_id = $1 order by created_at desc",
        )
        .bind(owner_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(File::from).collect())
    }

    pub async fn delete_file(&self, id: FileId) -> Result<(), DbError> {
        sqlx::query("delete from files where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
