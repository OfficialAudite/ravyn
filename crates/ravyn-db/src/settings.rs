use ravyn_core::{NamingScheme, RegistrationMode};

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

    /// Falls back to `Original` if unrecognized, the same safe-default
    /// reasoning as `get_registration_mode` — and the one choice among the
    /// four that reproduces today's behavior, so a bad value never starts
    /// silently renaming people's uploads.
    pub async fn get_naming_scheme(&self) -> Result<NamingScheme, DbError> {
        let raw: String =
            sqlx::query_scalar("select naming_scheme from instance_settings where id = 1")
                .fetch_one(&self.pool)
                .await?;

        Ok(NamingScheme::parse(&raw).unwrap_or(NamingScheme::Original))
    }

    pub async fn set_naming_scheme(&self, scheme: NamingScheme) -> Result<(), DbError> {
        sqlx::query("update instance_settings set naming_scheme = $1 where id = 1")
            .bind(scheme.as_str())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_random_name_length(&self) -> Result<i64, DbError> {
        let length: i32 =
            sqlx::query_scalar("select random_name_length from instance_settings where id = 1")
                .fetch_one(&self.pool)
                .await?;

        Ok(length as i64)
    }

    pub async fn set_random_name_length(&self, length: i64) -> Result<(), DbError> {
        sqlx::query("update instance_settings set random_name_length = $1 where id = 1")
            .bind(length as i32)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
