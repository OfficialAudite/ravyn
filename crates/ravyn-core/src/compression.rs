use serde::{Deserialize, Serialize};

/// A target format for optional, lossy image recompression at upload
/// time, off by default (`None` wherever this is stored), since it
/// trades image quality for size and that's not a call this app should
/// make for someone without asking. Deliberately just these two: `image`
/// (the crate this is built on) only supports lossless WebP encoding,
/// not quality-based, so a "WebP" option here would silently ignore the
/// quality setting; AVIF fills that role instead, and generally
/// compresses better than WebP at the same visual quality anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageCompressionFormat {
    Jpeg,
    Avif,
}

impl ImageCompressionFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageCompressionFormat::Jpeg => "jpeg",
            ImageCompressionFormat::Avif => "avif",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "jpeg" | "jpg" => Some(ImageCompressionFormat::Jpeg),
            "avif" => Some(ImageCompressionFormat::Avif),
            _ => None,
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            ImageCompressionFormat::Jpeg => "image/jpeg",
            ImageCompressionFormat::Avif => "image/avif",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageCompressionFormat::Jpeg => "jpg",
            ImageCompressionFormat::Avif => "avif",
        }
    }
}

/// A 1-100 quality target, clamped rather than rejected - same reasoning
/// as `random_name_length`'s own clamp: a stray out-of-range value from a
/// hand-edited request is a nuisance, not something worth failing an
/// otherwise-valid upload over.
pub fn clamp_quality(quality: i64) -> u8 {
    quality.clamp(1, 100) as u8
}
