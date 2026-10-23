/// Reduces whatever a client declared to a bare, lowercase `type/subtype`,
/// or `application/octet-stream` if it doesn't look like one. Parameters
/// such as `; charset=utf-8` are dropped.
pub fn normalize_content_type(raw: &str) -> String {
    let essence = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let valid_token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };

    match essence.split_once('/') {
        Some((kind, subtype)) if valid_token(kind) && valid_token(subtype) => essence,
        _ => "application/octet-stream".to_string(),
    }
}

/// Types a browser can display without running anything the uploader
/// wrote. Everything else is served as a download.
pub fn is_inline_safe(content_type: &str) -> bool {
    let essence = normalize_content_type(content_type);
    match essence.split_once('/') {
        Some(("image", subtype)) => subtype != "svg+xml",
        Some(("video" | "audio", _)) => true,
        _ => matches!(essence.as_str(), "application/pdf" | "text/plain"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_parameters_and_case() {
        assert_eq!(
            normalize_content_type("Text/HTML; charset=utf-8"),
            "text/html"
        );
        assert_eq!(normalize_content_type(" image/png "), "image/png");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(normalize_content_type("text/html\n"), "text/html");
        assert_eq!(
            normalize_content_type("text/ht ml"),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_content_type("nonsense"),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_content_type("a/b\r\nx: y"),
            "application/octet-stream"
        );
        assert_eq!(normalize_content_type(""), "application/octet-stream");
    }

    #[test]
    fn inline_allowlist() {
        assert!(is_inline_safe("image/png"));
        assert!(is_inline_safe("video/mp4"));
        assert!(is_inline_safe("text/plain; charset=utf-8"));
        assert!(!is_inline_safe("image/svg+xml"));
        assert!(!is_inline_safe("TEXT/HTML; charset=utf-8"));
        assert!(!is_inline_safe("application/xhtml+xml"));
        assert!(!is_inline_safe("application/octet-stream"));
    }
}
