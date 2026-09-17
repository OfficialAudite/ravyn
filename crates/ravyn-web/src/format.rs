pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    format!("{size:.1} {}", UNITS[unit])
}

/// Trims an RFC3339 timestamp down to just its date, e.g.
/// `2026-09-16T05:58:25Z` -> `2026-09-16`.
pub fn format_date(rfc3339: &str) -> &str {
    rfc3339.get(0..10).unwrap_or(rfc3339)
}

/// Mirrors `ExpiryPreset` in `ravyn-core` (key, label) — duplicated rather
/// than shared, since `ravyn-web` never depends on `ravyn-core` (it only
/// ever talks to `ravyn-api` over HTTP, see `server_fns::ssr`). Used by both
/// the admin default-expiry dropdown and each file's own expiry dropdown, so
/// the two stay in sync with each other even if not with the API crate.
pub const EXPIRY_PRESETS: &[(&str, &str)] = &[
    ("never", "never"),
    ("5m", "5 minutes"),
    ("10m", "10 minutes"),
    ("30m", "30 minutes"),
    ("1h", "1 hour"),
    ("6h", "6 hours"),
    ("12h", "12 hours"),
    ("1d", "1 day"),
    ("3d", "3 days"),
    ("1w", "1 week"),
    ("2w", "2 weeks"),
    ("1mo", "1 month"),
    ("3mo", "3 months"),
    ("6mo", "6 months"),
    ("1y", "1 year"),
];

/// The `128 images · 12 videos · ...` line shared by a user's own stats and
/// the instance-wide admin overview — plain string building here rather
/// than a shared view-returning helper, so this file stays free of any
/// dependency on Leptos or on the shape of either caller's stats struct.
pub fn format_type_breakdown(
    images: i64,
    videos: i64,
    audio: i64,
    documents: i64,
    other: i64,
) -> String {
    format!(
        "{images} images · {videos} videos · {audio} audio · {documents} documents · {other} other"
    )
}
