use std::sync::Arc;

use object_store::{
    aws::AmazonS3Builder, local::LocalFileSystem, multipart::MultipartStore, ClientOptions,
    ObjectStore,
};

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
    /// The second half is only there for backends that can take parts of
    /// one object over separate requests, which local disk can't.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build(
        self,
    ) -> Result<(Arc<dyn ObjectStore>, Option<Arc<dyn MultipartStore>>), StorageError> {
        match self {
            StorageConfig::Local { root } => {
                std::fs::create_dir_all(&root)?;
                Ok((Arc::new(LocalFileSystem::new_with_prefix(root)?), None))
            }
            StorageConfig::S3 {
                bucket,
                region,
                endpoint,
                access_key_id,
                secret_access_key,
            } => {
                // object_store's default 30s timeout covers the whole
                // request, body included, so any download slower than that
                // got cut off partway. The connect timeout still applies.
                let mut builder = AmazonS3Builder::new()
                    .with_client_options(ClientOptions::new().with_timeout_disabled())
                    .with_bucket_name(bucket)
                    .with_region(region)
                    .with_access_key_id(access_key_id)
                    .with_secret_access_key(secret_access_key);

                if let Some(endpoint) = endpoint {
                    builder = builder.with_endpoint(endpoint).with_allow_http(true);
                }

                let s3 = Arc::new(builder.build()?);
                Ok((s3.clone(), Some(s3)))
            }
        }
    }
}
