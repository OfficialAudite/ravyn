mod config;
mod error;

pub use config::StorageConfig;
pub use error::StorageError;

use std::sync::Arc;

use bytes::Bytes;
use object_store::{path::Path as ObjectPath, ObjectStore};

#[derive(Clone)]
pub struct Storage {
    store: Arc<dyn ObjectStore>,
}

impl Storage {
    pub fn new(config: StorageConfig) -> Result<Self, StorageError> {
        Ok(Self {
            store: config.build()?,
        })
    }

    pub async fn put(&self, key: &str, bytes: Bytes) -> Result<(), StorageError> {
        let path = ObjectPath::from(key);
        self.store.put(&path, bytes.into()).await?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = ObjectPath::from(key);
        let result = self.store.get(&path).await?;
        Ok(result.bytes().await?)
    }

    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = ObjectPath::from(key);
        self.store.delete(&path).await?;
        Ok(())
    }
}
