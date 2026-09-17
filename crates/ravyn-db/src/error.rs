use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl DbError {
    /// Whether this is a unique-constraint violation — the caller's signal
    /// to retry with a fresh value rather than treat it as a real failure.
    /// Currently only meaningful use: a randomly generated short-URL slug
    /// colliding with an existing one.
    pub fn is_unique_violation(&self) -> bool {
        match self {
            DbError::Sqlx(sqlx::Error::Database(err)) => err.is_unique_violation(),
            _ => false,
        }
    }
}
