use ravyn_core::{ExpiryPreset, ImageCompressionFormat, NamingScheme, RegistrationMode};

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

    /// Falls back to `Never` if unrecognized — the safe default, since it
    /// reproduces pre-auto-delete behavior rather than starting to silently
    /// delete files.
    pub async fn get_default_expiry_preset(&self) -> Result<ExpiryPreset, DbError> {
        let raw: String =
            sqlx::query_scalar("select default_expiry_preset from instance_settings where id = 1")
                .fetch_one(&self.pool)
                .await?;

        Ok(ExpiryPreset::parse(&raw).unwrap_or(ExpiryPreset::Never))
    }

    pub async fn set_default_expiry_preset(&self, preset: ExpiryPreset) -> Result<(), DbError> {
        sqlx::query("update instance_settings set default_expiry_preset = $1 where id = 1")
            .bind(preset.as_str())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_strip_exif(&self) -> Result<bool, DbError> {
        let value: bool =
            sqlx::query_scalar("select strip_exif from instance_settings where id = 1")
                .fetch_one(&self.pool)
                .await?;

        Ok(value)
    }

    pub async fn set_strip_exif(&self, strip_exif: bool) -> Result<(), DbError> {
        sqlx::query("update instance_settings set strip_exif = $1 where id = 1")
            .bind(strip_exif)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// `cost_per_gb_month` is `None` until an admin fills it in - there's no
    /// sensible default price to guess at, unlike every other instance
    /// setting here, so this one stays unconfigured (and the cost estimate
    /// hidden) rather than defaulting to some made-up number.
    pub async fn get_cost_settings(&self) -> Result<(Option<f64>, String), DbError> {
        let row: (Option<f64>, String) = sqlx::query_as(
            "select cost_per_gb_month, cost_currency from instance_settings where id = 1",
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    pub async fn set_cost_settings(
        &self,
        cost_per_gb_month: Option<f64>,
        cost_currency: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "update instance_settings set cost_per_gb_month = $1, cost_currency = $2 where id = 1",
        )
        .bind(cost_per_gb_month)
        .bind(cost_currency)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// `None` (both columns unset) means off, the same "no sensible
    /// default to guess at" reasoning as the cost estimate's own price
    /// field - unlike every naming/expiry/privacy default above, this one
    /// trades image quality for size, so it stays opt-in rather than
    /// silently recompressing uploads until an admin turns it on.
    pub async fn get_compression_settings(
        &self,
    ) -> Result<(Option<ImageCompressionFormat>, Option<i64>), DbError> {
        let row: (Option<String>, Option<i32>) = sqlx::query_as(
            "select default_compression_format, default_compression_quality from instance_settings where id = 1",
        )
        .fetch_one(&self.pool)
        .await?;

        let format = row.0.as_deref().and_then(ImageCompressionFormat::parse);
        Ok((format, row.1.map(|quality| quality as i64)))
    }

    pub async fn set_compression_settings(
        &self,
        format: Option<ImageCompressionFormat>,
        quality: Option<i64>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "update instance_settings set default_compression_format = $1, default_compression_quality = $2 where id = 1",
        )
        .bind(format.map(|f| f.as_str()))
        .bind(quality.map(|quality| quality as i32))
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
