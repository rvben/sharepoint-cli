//! Shared utilities used across multiple modules.

use std::time::Duration;

/// Maximum cap on `Retry-After` header value (seconds) — prevents a hostile
/// or misconfigured upstream from blocking the CLI for hours.
pub const MAX_RETRY_AFTER_SECS: u64 = 60;

/// Parse the value of a `Retry-After` header as a `Duration`.
///
/// Accepts both numeric seconds (e.g. `"30"`) and HTTP-date form
/// (e.g. `"Thu, 01 Jan 2026 00:00:30 GMT"`), per RFC 7231 §7.1.3.
/// The result is capped at [`MAX_RETRY_AFTER_SECS`].
pub fn parse_retry_after(header: &str) -> Option<Duration> {
    let s = header.trim();
    // Numeric seconds (primary form used by Graph and token endpoints).
    if let Ok(secs) = s.parse::<u64>() {
        return Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)));
    }
    // HTTP-date form (used by Graph for long-throttled accounts).
    if let Ok(when) = httpdate::parse_http_date(s) {
        let now = std::time::SystemTime::now();
        if let Ok(delta) = when.duration_since(now) {
            return Some(Duration::from_secs(
                delta.as_secs().min(MAX_RETRY_AFTER_SECS),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_retry_after_accepts_numeric_seconds() {
        let d = parse_retry_after("30").expect("numeric should parse");
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn parse_retry_after_clamps_to_max() {
        let d = parse_retry_after("9999").expect("numeric should parse");
        assert_eq!(d.as_secs(), MAX_RETRY_AFTER_SECS);
    }

    #[test]
    fn parse_retry_after_accepts_http_date_in_the_future() {
        let when = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let header = httpdate::fmt_http_date(when);
        let d = parse_retry_after(&header).expect("HTTP-date should parse");
        assert!(d.as_secs() <= 10, "got {d:?}");
    }

    #[test]
    fn parse_retry_after_rejects_garbage() {
        assert!(parse_retry_after("not-a-time").is_none());
    }

    #[test]
    fn parse_retry_after_rejects_past_http_date() {
        let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let header = httpdate::fmt_http_date(when);
        assert!(parse_retry_after(&header).is_none());
    }
}
