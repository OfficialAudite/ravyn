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

    /// Starts a multipart upload the caller feeds chunks into as they
    /// arrive, instead of handing over one `Bytes` already fully in memory.
    /// This is what actually makes a large upload bounded-memory end to
    /// end — `put` above still needs its whole file in RAM before this is
    /// ever called, this is for callers that don't.
    pub async fn start_upload(&self, key: &str) -> Result<StorageUpload, StorageError> {
        let path = ObjectPath::from(key);
        let upload = self.store.put_multipart(&path).await?;
        Ok(StorageUpload {
            writer: WriteMultipart::new(upload),
        })
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

/// An in-progress streaming upload, one chunk at a time. Deliberately
/// knows nothing about file ownership, hashing, or quotas — a caller
/// streaming a request body decides when to write a chunk and when to
/// give up, this just gets the bytes to the backend without ever needing
/// the whole file in memory at once.
pub struct StorageUpload {
    writer: WriteMultipart,
}

impl StorageUpload {
    /// Waits for at most `max_concurrency` parts to still be in flight
    /// before queuing another — without this, a fast producer (a chunk
    /// arriving off the network) could queue parts faster than the
    /// backend accepts them, defeating the point of streaming by piling
    /// up unbounded memory anyway.
    pub async fn write_chunk(&mut self, chunk: Bytes) -> Result<(), StorageError> {
        self.writer.wait_for_capacity(4).await?;
        self.writer.put(chunk);
        Ok(())
    }

    pub async fn finish(self) -> Result<(), StorageError> {
        self.writer.finish().await?;
        Ok(())
    }

    /// Cleans up the parts already uploaded — used when a caller decides
    /// partway through (e.g. a quota check) that this upload shouldn't be
    /// completed after all, so it doesn't leave orphaned parts billed
    /// against the bucket forever.
    pub async fn abort(self) -> Result<(), StorageError> {
        self.writer.abort().await?;
        Ok(())
    }
}
