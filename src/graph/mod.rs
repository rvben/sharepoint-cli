//! Microsoft Graph client.
//!
//! Centralizes HTTP, auth header injection, error mapping, retry/backoff for
//! 429/5xx, and paging continuation. Sub-modules (sites, drives, search,
//! download) call `GraphClient::get_json` / `iterator` etc. — they don't
//! build their own clients.

use std::time::Duration;

use reqwest::{Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio::time::sleep;

use crate::auth::AuthContext;
use crate::error::{CliError, Result};

/// Maximum number of automatic retries on 429 / 5xx.
const MAX_RETRIES: u32 = 3;
/// Cap on `Retry-After` header value (seconds) — prevents a hostile or
/// misconfigured upstream from blocking the CLI for hours.
const MAX_RETRY_AFTER_SECS: u64 = 60;

#[derive(Clone)]
pub struct GraphClient {
    auth: AuthContext,
}

impl GraphClient {
    pub fn new(auth: AuthContext) -> Self {
        Self { auth }
    }

    pub fn auth(&self) -> &AuthContext {
        &self.auth
    }

    /// Build a fully-qualified URL from a path. Accepts both absolute URLs
    /// (used when following `@odata.nextLink`) and bare paths.
    pub async fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        let cfg = self.auth.config().await;
        let base = cfg.graph_endpoint.trim_end_matches('/');
        if let Some(rest) = path.strip_prefix('/') {
            format!("{base}/{rest}")
        } else {
            format!("{base}/{path}")
        }
    }

    /// Perform a GET and parse JSON, with retry/backoff and Graph error mapping.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.send(Method::GET, path, None).await?;
        let body = resp.text().await?;
        serde_json::from_str(&body)
            .map_err(|e| CliError::Other(format!("graph response not JSON: {e}; body={body}")))
    }

    /// Perform a request that returns a streaming body (e.g., download).
    /// `body` is cloned per retry attempt; pass `None` for GET/HEAD.
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Response> {
        let url = self.url(path).await;
        let mut attempt: u32 = 0;
        loop {
            let token = self.auth.access_token().await?;
            let http = self.auth.http().await;
            let mut req = http
                .request(method.clone(), &url)
                .bearer_auth(&token)
                .header("Accept", "application/json");
            if let Some(b) = body.as_ref() {
                req = req.body(b.clone());
            }
            let resp = req
                .send()
                .await
                .map_err(|e| CliError::Http(format!("graph {method} {url}: {e}")))?;

            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }

            let retry_after = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_retry_after);

            let body_text = resp.text().await.unwrap_or_default();
            let cfg = self.auth.config().await;
            let detail = if cfg.debug_http {
                format!(": {body_text}")
            } else {
                String::new()
            };

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
                let secs = retry_after
                    .map(|d| d.as_secs())
                    .unwrap_or_else(|| 2u64.pow(attempt));
                sleep(Duration::from_secs(secs)).await;
                attempt += 1;
                continue;
            }
            if status.is_server_error()
                && attempt < MAX_RETRIES
                && matches!(method, Method::GET | Method::HEAD)
            {
                let secs = 2u64.pow(attempt);
                sleep(Duration::from_secs(secs)).await;
                attempt += 1;
                continue;
            }

            return Err(map_status(status, &body_text, &detail));
        }
    }

    /// Return the configured Graph API endpoint (e.g. `"https://graph.microsoft.com/v1.0"`).
    pub async fn graph_endpoint(&self) -> String {
        self.auth.config().await.graph_endpoint.clone()
    }

    /// Drain a paged collection by following `@odata.nextLink` until exhausted.
    pub async fn page_all<T: DeserializeOwned>(&self, first_path: &str) -> Result<Vec<T>> {
        let mut acc = Vec::new();
        let mut next = Some(first_path.to_string());
        while let Some(p) = next.take() {
            let page: PagedResponse<T> = self.get_json(&p).await?;
            acc.extend(page.value);
            next = page.next_link;
        }
        Ok(acc)
    }
}

#[derive(serde::Deserialize)]
pub struct PagedResponse<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    pub next_link: Option<String>,
}

/// Parse the value of a `Retry-After` header as a `Duration`.
///
/// Accepts both numeric seconds (e.g. `"30"`) and HTTP-date form
/// (e.g. `"Thu, 01 Jan 2026 00:00:30 GMT"`), per RFC 7231 §7.1.3.
/// The result is capped at `MAX_RETRY_AFTER_SECS`.
fn parse_retry_after(header: &str) -> Option<Duration> {
    let s = header.trim();
    // Numeric seconds (primary form used by Graph).
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

fn map_status(status: StatusCode, body: &str, detail: &str) -> CliError {
    let primary = extract_graph_error_message(body).unwrap_or_else(|| status.to_string());
    match status {
        StatusCode::UNAUTHORIZED => CliError::Auth(format!("Graph 401: {primary}{detail}")),
        StatusCode::FORBIDDEN => CliError::Auth(format!("Graph 403: {primary}{detail}")),
        StatusCode::NOT_FOUND => CliError::NotFound(format!("{primary}{detail}")),
        StatusCode::TOO_MANY_REQUESTS => CliError::RateLimit,
        s => CliError::Api {
            status: s.as_u16(),
            message: format!("{primary}{detail}"),
        },
    }
}

/// Encode a Graph `@odata.nextLink` URL as a base64 page token for `--page`.
pub fn encode_page_token(next_link: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(next_link.as_bytes())
}

/// Decode a `--page` token back to its Graph URL, validating that the URL's
/// host matches the configured Graph endpoint. Tokens whose host does not
/// match `graph_endpoint` are rejected to prevent bearer-token leakage.
pub fn decode_page_token(graph_endpoint: &str, token: &str) -> Result<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|_| CliError::Input("invalid page token (not base64)".into()))?;
    let s = String::from_utf8(bytes)
        .map_err(|_| CliError::Input("invalid page token (not utf-8)".into()))?;
    let token_url = url::Url::parse(&s)
        .map_err(|_| CliError::Input("invalid page token (not a URL)".into()))?;
    let allowed = url::Url::parse(graph_endpoint)
        .map_err(|_| CliError::Other("invalid configured graph_endpoint".into()))?;
    let allowed_host = allowed
        .host_str()
        .ok_or_else(|| CliError::Other("graph_endpoint must have a host".into()))?;
    if token_url.host_str() != Some(allowed_host) {
        return Err(CliError::Input(format!(
            "page token host mismatch: token points at {:?}, expected {:?} — refusing to follow untrusted host",
            token_url.host_str(),
            allowed_host
        )));
    }
    Ok(s)
}

fn extract_graph_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = v.get("error")?;
    let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("");
    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    Some(if code.is_empty() {
        msg.to_string()
    } else {
        format!("{code}: {msg}")
    })
}

pub mod download;
pub mod drives;
pub mod search;
pub mod sites;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_graph_error_message_handles_well_formed_body() {
        let body =
            r#"{"error":{"code":"itemNotFound","message":"The resource could not be found."}}"#;
        let msg = extract_graph_error_message(body).unwrap();
        assert!(msg.contains("itemNotFound"));
        assert!(msg.contains("could not be found"));
    }

    #[test]
    fn extract_graph_error_message_returns_none_for_non_json() {
        assert!(extract_graph_error_message("not json").is_none());
    }

    #[test]
    fn map_status_404_is_not_found() {
        let body = r#"{"error":{"code":"itemNotFound","message":"missing"}}"#;
        let err = map_status(StatusCode::NOT_FOUND, body, "");
        assert!(matches!(err, CliError::NotFound(_)));
    }

    #[test]
    fn map_status_429_is_rate_limit() {
        let err = map_status(StatusCode::TOO_MANY_REQUESTS, "", "");
        assert!(matches!(err, CliError::RateLimit));
    }

    #[test]
    fn map_status_500_is_api_error() {
        let err = map_status(StatusCode::INTERNAL_SERVER_ERROR, "", "");
        match err {
            CliError::Api { status, .. } => assert_eq!(status, 500),
            _ => panic!("expected API error"),
        }
    }

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
        // Allow 0..=10s to account for sub-second clock drift between creation and parsing.
        assert!(d.as_secs() <= 10, "got {d:?}");
    }

    #[test]
    fn parse_retry_after_rejects_garbage() {
        assert!(parse_retry_after("not-a-time").is_none());
    }

    #[test]
    fn decode_page_token_rejects_token_pointing_at_wrong_host() {
        use base64::Engine as _;
        let attacker = "https://attacker.example/v1.0/sites?$skiptoken=evil";
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(attacker.as_bytes());
        let err = decode_page_token("https://graph.microsoft.com/v1.0", &token).unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
        assert!(err.to_string().contains("host"));
    }

    #[test]
    fn decode_page_token_accepts_matching_host() {
        let url = "https://graph.microsoft.com/v1.0/sites?$skiptoken=ABC";
        let encoded = encode_page_token(url);
        let decoded = decode_page_token("https://graph.microsoft.com/v1.0", &encoded).unwrap();
        assert_eq!(decoded, url);
    }

    #[test]
    fn decode_page_token_rejects_invalid_base64() {
        let err = decode_page_token("https://graph.microsoft.com/v1.0", "!!!").unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
    }

    #[test]
    fn parse_retry_after_rejects_past_http_date() {
        // A date in the past yields duration_since error — returns None.
        let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let header = httpdate::fmt_http_date(when);
        // The date is in the past (epoch + ~11 days), so duration_since(now) fails.
        assert!(parse_retry_after(&header).is_none());
    }
}
