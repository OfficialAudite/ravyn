use std::sync::Arc;

use object_store::{aws::AmazonS3Builder, local::LocalFileSystem, ObjectStore};

use crate::StorageError;

pub enum StorageConfig {
    Local {
        root: String,
    },
    S3 {
        bucket: String,
        region: String,
        endpoint: Option<String>,
        access_key_id: String,
        secret_access_key: String,
    },
}

impl StorageConfig {
    pub(crate) fn build(self) -> Result<Arc<dyn ObjectStore>, StorageError> {
        match self {
            StorageConfig::Local { root } => {
                std::fs::create_dir_all(&root)?;
                Ok(Arc::new(LocalFileSystem::new_with_prefix(root)?))
            }
            StorageConfig::S3 {
                bucket,
                region,
                endpoint,
                access_key_id,
                secret_access_key,
            } => {
                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(bucket)
                    .with_region(region)
                    .with_access_key_id(access_key_id)
                    .with_secret_access_key(secret_access_key);

                if let Some(endpoint) = endpoint {
                    builder = builder.with_endpoint(endpoint).with_allow_http(true);
                }

                Ok(Arc::new(builder.build()?))
            }
        }
    }
}
