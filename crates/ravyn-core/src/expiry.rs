use serde::{Deserialize, Serialize};
use time::Duration;

/// How long a file lives before it's auto-deleted, instance-wide or per-file
/// — same shape as [`crate::NamingScheme`]. A fixed set of presets rather
/// than a free-form date: harder to fat-finger into an absurd value, and
/// matches how Zipline offers this.
///
/// "1 month" and "1 year" are calendar-approximate (30 and 365 days) rather
/// than calendar-exact — good enough for an auto-delete timer, and avoids
/// pulling in calendar-arithmetic just for this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpiryPreset {
    Never,
    Minutes5,
    Minutes10,
    Minutes30,
    Hours1,
    Hours6,
    Hours12,
    Day1,
    Days3,
    Week1,
    Weeks2,
    Month1,
    Months3,
    Months6,
    Year1,
}

impl ExpiryPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            ExpiryPreset::Never => "never",
            ExpiryPreset::Minutes5 => "5m",
            ExpiryPreset::Minutes10 => "10m",
            ExpiryPreset::Minutes30 => "30m",
            ExpiryPreset::Hours1 => "1h",
            ExpiryPreset::Hours6 => "6h",
            ExpiryPreset::Hours12 => "12h",
            ExpiryPreset::Day1 => "1d",
            ExpiryPreset::Days3 => "3d",
            ExpiryPreset::Week1 => "1w",
            ExpiryPreset::Weeks2 => "2w",
            ExpiryPreset::Month1 => "1mo",
            ExpiryPreset::Months3 => "3mo",
            ExpiryPreset::Months6 => "6mo",
            ExpiryPreset::Year1 => "1y",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "never" => Some(ExpiryPreset::Never),
            "5m" => Some(ExpiryPreset::Minutes5),
            "10m" => Some(ExpiryPreset::Minutes10),
            "30m" => Some(ExpiryPreset::Minutes30),
            "1h" => Some(ExpiryPreset::Hours1),
            "6h" => Some(ExpiryPreset::Hours6),
            "12h" => Some(ExpiryPreset::Hours12),
            "1d" => Some(ExpiryPreset::Day1),
            "3d" => Some(ExpiryPreset::Days3),
            "1w" => Some(ExpiryPreset::Week1),
            "2w" => Some(ExpiryPreset::Weeks2),
            "1mo" => Some(ExpiryPreset::Month1),
            "3mo" => Some(ExpiryPreset::Months3),
            "6mo" => Some(ExpiryPreset::Months6),
            "1y" => Some(ExpiryPreset::Year1),
            _ => None,
        }
    }

    /// `None` for `Never` — the file has no expiry at all.
    pub fn to_duration(self) -> Option<Duration> {
        match self {
            ExpiryPreset::Never => None,
            ExpiryPreset::Minutes5 => Some(Duration::minutes(5)),
            ExpiryPreset::Minutes10 => Some(Duration::minutes(10)),
            ExpiryPreset::Minutes30 => Some(Duration::minutes(30)),
            ExpiryPreset::Hours1 => Some(Duration::hours(1)),
            ExpiryPreset::Hours6 => Some(Duration::hours(6)),
            ExpiryPreset::Hours12 => Some(Duration::hours(12)),
            ExpiryPreset::Day1 => Some(Duration::days(1)),
            ExpiryPreset::Days3 => Some(Duration::days(3)),
            ExpiryPreset::Week1 => Some(Duration::days(7)),
            ExpiryPreset::Weeks2 => Some(Duration::days(14)),
            ExpiryPreset::Month1 => Some(Duration::days(30)),
            ExpiryPreset::Months3 => Some(Duration::days(90)),
            ExpiryPreset::Months6 => Some(Duration::days(182)),
            ExpiryPreset::Year1 => Some(Duration::days(365)),
        }
    }
}
