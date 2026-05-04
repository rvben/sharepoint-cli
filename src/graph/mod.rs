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
                .and_then(|s| s.parse::<u64>().ok());

            let body_text = resp.text().await.unwrap_or_default();
            let cfg = self.auth.config().await;
            let detail = if cfg.debug_http {
                format!(": {body_text}")
            } else {
                String::new()
            };

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
                let secs = retry_after
                    .map(|s| s.min(MAX_RETRY_AFTER_SECS))
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

pub mod drives;
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
}
