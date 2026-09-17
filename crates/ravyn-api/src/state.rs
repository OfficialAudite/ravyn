use std::sync::Arc;

use ravyn_db::Db;
use ravyn_storage::Storage;

use crate::rate_limit::RateLimiters;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// Where uploaded files themselves live — local disk or S3, per
    /// `STORAGE_BACKEND`.
    pub storage: Storage,
    /// Always local disk, regardless of `storage`'s backend: thumbnails are
    /// small and read constantly (every gallery view), so there's no reason
    /// to round-trip them through S3.
    pub thumbnails: Storage,
    /// Reused across requests — `reqwest::Client` is `Arc`-backed internally,
    /// so cloning it is cheap and keeps connection pooling working. Used
    /// only for firing upload webhook notifications.
    pub http: reqwest::Client,
    /// `RAVYN_PUBLIC_API_URL`, if set — see `routes::resolve_public_base_url`
    /// for why a request's own headers aren't always enough to know this.
    pub public_url: Option<String>,
    pub rate_limiters: Arc<RateLimiters>,
}
