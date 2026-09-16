use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    ObjectStore(#[from] object_store::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StorageError {
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            StorageError::ObjectStore(object_store::Error::NotFound { .. })
        )
    }
}
