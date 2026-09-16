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
