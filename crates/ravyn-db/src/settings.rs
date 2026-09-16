use ravyn_core::RegistrationMode;

use crate::{Db, DbError};

impl Db {
    /// Falls back to `Closed` if the mode stored in the database is somehow
    /// unrecognized (e.g. rolled back to an older schema) rather than
    /// failing the request — the safer of the two failure directions.
    pub async fn get_registration_mode(&self) -> Result<RegistrationMode, DbError> {
        let raw: String =
            sqlx::query_scalar("select registration_mode from instance_settings where id = 1")
                .fetch_one(&self.pool)
                .await?;

        Ok(RegistrationMode::parse(&raw).unwrap_or(RegistrationMode::Closed))
    }

    pub async fn set_registration_mode(&self, mode: RegistrationMode) -> Result<(), DbError> {
        sqlx::query("update instance_settings set registration_mode = $1 where id = 1")
            .bind(mode.as_str())
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
