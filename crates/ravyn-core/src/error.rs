use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),
}
