use ravyn_core::{File, FileId, FolderId, UserId};
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
    folder_id: Option<Uuid>,
    password_hash: Option<String>,
    created_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
    tags: Vec<String>,
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
            folder_id: row.folder_id.map(FolderId),
            password_hash: row.password_hash,
            created_at: row.created_at,
            expires_at: row.expires_at,
            tags: row.tags,
        }
    }
}

impl Db {
    pub async fn insert_file(&self, file: &File) -> Result<(), DbError> {
        sqlx::query(
            "insert into files (id, owner_id, original_name, storage_key, content_type, size_bytes, sha256, folder_id, password_hash, created_at, expires_at, tags)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(file.id.0)
        .bind(file.owner_id.0)
        .bind(&file.original_name)
        .bind(&file.storage_key)
        .bind(&file.content_type)
        .bind(file.size_bytes as i64)
        .bind(&file.sha256)
        .bind(file.folder_id.map(|id| id.0))
        .bind(&file.password_hash)
        .bind(file.created_at)
        .bind(file.expires_at)
        .bind(&file.tags)
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

    pub async fn count_files_for_owner(&self, owner_id: UserId) -> Result<i64, DbError> {
        let count: i64 = sqlx::query_scalar("select count(*) from files where owner_id = $1")
            .bind(owner_id.0)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    pub async fn list_files_for_folder(&self, folder_id: FolderId) -> Result<Vec<File>, DbError> {
        let rows = sqlx::query_as::<_, FileRow>(
            "select * from files where folder_id = $1 order by created_at desc",
        )
        .bind(folder_id.0)
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

    pub async fn set_file_folder(
        &self,
        id: FileId,
        folder_id: Option<FolderId>,
    ) -> Result<(), DbError> {
        sqlx::query("update files set folder_id = $2 where id = $1")
            .bind(id.0)
            .bind(folder_id.map(|id| id.0))
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_file_password(
        &self,
        id: FileId,
        password_hash: Option<String>,
    ) -> Result<(), DbError> {
        sqlx::query("update files set password_hash = $2 where id = $1")
            .bind(id.0)
            .bind(password_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_file_name(&self, id: FileId, name: String) -> Result<(), DbError> {
        sqlx::query("update files set original_name = $2 where id = $1")
            .bind(id.0)
            .bind(name)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_file_expiry(
        &self,
        id: FileId,
        expires_at: Option<OffsetDateTime>,
    ) -> Result<(), DbError> {
        sqlx::query("update files set expires_at = $2 where id = $1")
            .bind(id.0)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_file_tags(&self, id: FileId, tags: Vec<String>) -> Result<(), DbError> {
        sqlx::query("update files set tags = $2 where id = $1")
            .bind(id.0)
            .bind(tags)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Feeds the background expiry sweep (`routes::files::run_expiry_sweep`)
    /// — every file whose timer has already run out, regardless of owner.
    pub async fn list_expired_files(&self) -> Result<Vec<File>, DbError> {
        let rows = sqlx::query_as::<_, FileRow>(
            "select * from files where expires_at is not null and expires_at < now()",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(File::from).collect())
    }
}
