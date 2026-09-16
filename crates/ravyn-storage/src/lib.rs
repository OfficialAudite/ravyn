mod config;
mod error;

pub use config::StorageConfig;
pub use error::StorageError;

use std::sync::Arc;

use bytes::Bytes;
use object_store::{path::Path as ObjectPath, ObjectStore, WriteMultipart};

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

    /// Always uploads via `object_store`'s multipart API, even for small
    /// files (a one-part multipart upload is valid — the 5MiB-per-part
    /// minimum only applies to parts before the last one). Every S3-
    /// compatible provider has its own hard limit on a single non-multipart
    /// PUT (5GiB on AWS itself, often less elsewhere); going through
    /// multipart uniformly means uploads of any size work the same way
    /// regardless of backend, without ravyn needing to know each
    /// provider's specific limits.
    pub async fn put(&self, key: &str, bytes: Bytes) -> Result<(), StorageError> {
        let path = ObjectPath::from(key);
        let upload = self.store.put_multipart(&path).await?;
        let mut writer = WriteMultipart::new(upload);
        writer.put(bytes);
        writer.finish().await?;
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
