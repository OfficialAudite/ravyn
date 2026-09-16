use ravyn_core::{Folder, FolderId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, DbError};

#[derive(sqlx::FromRow)]
struct FolderRow {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    password_hash: Option<String>,
    created_at: OffsetDateTime,
}

impl From<FolderRow> for Folder {
    fn from(row: FolderRow) -> Self {
        Folder {
            id: FolderId(row.id),
            owner_id: UserId(row.owner_id),
            name: row.name,
            password_hash: row.password_hash,
            created_at: row.created_at,
        }
    }
}

impl Db {
    pub async fn create_folder(&self, folder: &Folder) -> Result<(), DbError> {
        sqlx::query(
            "insert into folders (id, owner_id, name, password_hash, created_at)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(folder.id.0)
        .bind(folder.owner_id.0)
        .bind(&folder.name)
        .bind(&folder.password_hash)
        .bind(folder.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_folder(&self, id: FolderId) -> Result<Option<Folder>, DbError> {
        let row = sqlx::query_as::<_, FolderRow>("select * from folders where id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(Folder::from))
    }

    pub async fn list_folders_for_owner(&self, owner_id: UserId) -> Result<Vec<Folder>, DbError> {
        let rows = sqlx::query_as::<_, FolderRow>(
            "select * from folders where owner_id = $1 order by created_at desc",
        )
        .bind(owner_id.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Folder::from).collect())
    }

    pub async fn delete_folder(&self, id: FolderId) -> Result<(), DbError> {
        sqlx::query("delete from folders where id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_folder_password(
        &self,
        id: FolderId,
        password_hash: Option<String>,
    ) -> Result<(), DbError> {
        sqlx::query("update folders set password_hash = $2 where id = $1")
            .bind(id.0)
            .bind(password_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
