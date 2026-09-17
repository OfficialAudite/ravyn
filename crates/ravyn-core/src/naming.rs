use rand::Rng;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// How a file's display name is chosen at upload time, instance-wide — an
/// admin setting, same shape as [`crate::RegistrationMode`]. Only affects the
/// name shown in the dashboard and offered on download; the shareable link
/// is always `/v/{uuid}` regardless, so this never touches URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NamingScheme {
    /// Keep whatever name the uploader's client sent.
    Original,
    /// A random lowercase alphanumeric string, `random_length` characters.
    Random,
    /// A fresh UUID.
    Uuid,
    /// The upload's UTC timestamp, `YYYYMMDD-HHMMSS`.
    Date,
}

impl NamingScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            NamingScheme::Original => "original",
            NamingScheme::Random => "random",
            NamingScheme::Uuid => "uuid",
            NamingScheme::Date => "date",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "original" => Some(NamingScheme::Original),
            "random" => Some(NamingScheme::Random),
            "uuid" => Some(NamingScheme::Uuid),
            "date" => Some(NamingScheme::Date),
            _ => None,
        }
    }

    /// Produces the display name to store for a freshly uploaded file,
    /// preserving `original_name`'s extension (if any) for every scheme but
    /// `Original`, so a generated name still opens in the right application.
    pub fn generate(self, original_name: &str, random_length: usize) -> String {
        if self == NamingScheme::Original {
            return original_name.to_string();
        }

        let extension = original_name
            .rsplit_once('.')
            .map(|(_, ext)| format!(".{ext}"))
            .unwrap_or_default();

        let stem = match self {
            NamingScheme::Original => unreachable!(),
            NamingScheme::Random => random_alphanumeric(random_length),
            NamingScheme::Uuid => Uuid::new_v4().to_string(),
            NamingScheme::Date => {
                let now = OffsetDateTime::now_utc();
                format!(
                    "{:04}{:02}{:02}-{:02}{:02}{:02}",
                    now.year(),
                    u8::from(now.month()),
                    now.day(),
                    now.hour(),
                    now.minute(),
                    now.second()
                )
            }
        };

        format!("{stem}{extension}")
    }
}

fn random_alphanumeric(length: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
