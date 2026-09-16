use ravyn_db::Db;
use ravyn_storage::Storage;

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
}
