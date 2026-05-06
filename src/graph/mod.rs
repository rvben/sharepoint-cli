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
use crate::util;

/// Maximum number of automatic retries on 429 / 5xx.
const MAX_RETRIES: u32 = 3;

#[derive(Clone)]
pub struct GraphClient {
    auth: AuthContext,
}

impl GraphClient {
    pub fn new(auth: AuthContext) -> Self {
        Self { auth }
    }

    /// Build a fully-qualified URL from a path. Accepts both absolute URLs
    /// (used when following `@odata.nextLink`) and bare paths.
    pub(crate) async fn url(&self, path: &str) -> String {
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
    pub(crate) async fn send(
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
                .and_then(util::parse_retry_after);

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
    pub(crate) async fn graph_endpoint(&self) -> String {
        self.auth.config().await.graph_endpoint.clone()
    }

    /// Drain a paged collection by following `@odata.nextLink` until exhausted.
    pub(crate) async fn page_all<T: DeserializeOwned>(&self, first_path: &str) -> Result<Vec<T>> {
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
pub(crate) struct PagedResponse<T> {
    pub(crate) value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    pub(crate) next_link: Option<String>,
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

/// A pagination cursor that tracks position across Graph pages.
///
/// Encoded as base64(JSON) in the `--page` / `next` fields so agents
/// can resume exactly where they left off, even mid-page.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    /// The Graph URL to fetch next. `None` means the collection is exhausted.
    pub next: Option<String>,
    /// Number of items to skip at the beginning of the page returned by `next`.
    /// Non-zero only when `--limit` cut off partway through a Graph page.
    #[serde(default)]
    pub skip: usize,
}

/// Encode a `Cursor` as a base64url string suitable for the `--page` / `next`
/// field. The encoding is base64(JSON) so the token is opaque to the caller.
pub fn encode_cursor(cursor: &Cursor) -> String {
    use base64::Engine as _;
    let json = serde_json::to_string(cursor).expect("Cursor serialization is infallible");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// Decode a `--page` token back to a `Cursor`, validating that any URL in
/// `cursor.next` has a host matching the configured Graph endpoint.
/// Tokens whose host does not match are rejected to prevent bearer-token
/// leakage to untrusted hosts.
pub(crate) fn decode_cursor(graph_endpoint: &str, token: &str) -> Result<Cursor> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|_| CliError::Input("invalid page token (not base64)".into()))?;
    let s = String::from_utf8(bytes)
        .map_err(|_| CliError::Input("invalid page token (not utf-8)".into()))?;
    let cursor: Cursor =
        serde_json::from_str(&s).map_err(|_| CliError::Input("invalid page token".into()))?;
    if let Some(ref url) = cursor.next {
        validate_token_host(graph_endpoint, url)?;
    }
    Ok(cursor)
}

/// Validate that `candidate` URL has the same host as `graph_endpoint`.
/// Rejects tokens that point at an unexpected host to prevent bearer-token
/// leakage.
fn validate_token_host(graph_endpoint: &str, candidate: &str) -> Result<()> {
    let token_url = url::Url::parse(candidate)
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
    Ok(())
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
pub(crate) mod drives;
pub(crate) mod search;
pub(crate) mod sites;

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
    fn cursor_round_trips_with_skip() {
        let url = "https://graph.microsoft.com/v1.0/sites?$skiptoken=ABC";
        let cursor = Cursor {
            next: Some(url.to_string()),
            skip: 42,
        };
        let encoded = encode_cursor(&cursor);
        let decoded = decode_cursor("https://graph.microsoft.com/v1.0", &encoded).unwrap();
        assert_eq!(decoded.next.as_deref(), Some(url));
        assert_eq!(decoded.skip, 42);
    }

    #[test]
    fn cursor_round_trips_exhausted() {
        let cursor = Cursor {
            next: None,
            skip: 0,
        };
        let encoded = encode_cursor(&cursor);
        let decoded = decode_cursor("https://graph.microsoft.com/v1.0", &encoded).unwrap();
        assert!(decoded.next.is_none());
        assert_eq!(decoded.skip, 0);
    }

    #[test]
    fn decode_cursor_rejects_token_pointing_at_wrong_host() {
        let cursor = Cursor {
            next: Some("https://attacker.example/v1.0/sites?$skiptoken=evil".to_string()),
            skip: 0,
        };
        let token = encode_cursor(&cursor);
        let err = decode_cursor("https://graph.microsoft.com/v1.0", &token).unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
        assert!(err.to_string().contains("host"));
    }

    #[test]
    fn decode_cursor_rejects_invalid_base64() {
        let err = decode_cursor("https://graph.microsoft.com/v1.0", "!!!").unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
    }

    #[test]
    fn decode_cursor_rejects_non_json_content() {
        use base64::Engine as _;
        // Simulate an old-format token: base64-encoded plain URL (not JSON).
        let old_token = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(b"https://graph.microsoft.com/v1.0/sites?$skiptoken=old");
        let err = decode_cursor("https://graph.microsoft.com/v1.0", &old_token).unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
    }
}
