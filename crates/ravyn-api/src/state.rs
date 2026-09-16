use ravyn_db::Db;
use ravyn_storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub storage: Storage,
}
