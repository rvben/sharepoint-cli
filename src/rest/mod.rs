//! SharePoint REST (`/_api`) client authenticated with browser-session cookies.
//!
//! The cookie header is only ever sent to the configured endpoint host; any
//! other URL (e.g. from a tampered page token) is rejected before sending.

pub mod download;
pub mod drives;
pub mod search;
pub mod sites;

use std::time::Duration;

use reqwest::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue};
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::config::ResolvedConfig;
use crate::error::{CliError, Result};
use crate::util::parse_retry_after;

// Unlike the Graph client — which retries idempotent requests (GET/HEAD) on
// 5xx but only retries 429 for other methods — the REST client only ever
// issues GET requests, so every request is safe to retry: it honors
// `Retry-After` on both 429 and 5xx responses.
const MAX_RETRIES: u32 = 3;
const ACCEPT_NOMETADATA: &str = "application/json;odata=nometadata";
pub(crate) const RELOGIN_HINT: &str =
    "; the browser session may have expired — run `sharepoint auth login`";

#[derive(Clone)]
pub struct RestClient {
    http: reqwest::Client,
    /// Parsed base URL, e.g. `https://contoso.sharepoint.com/`. Parsed once
    /// in `build()` so `check_host` never has to reparse it, and so an
    /// invalid endpoint is rejected at construction time.
    endpoint: url::Url,
    /// Tenant host that site URLs passed to `site_api` must use (e.g.
    /// `contoso.sharepoint.com`). `None` means `site_api` accepts a site URL
    /// on any host — every request still only ever reaches `endpoint`
    /// (`get` enforces that), so this is a config-sanity check, not the
    /// cookie-leak guard.
    tenant_host: Option<String>,
}

impl RestClient {
    /// Client with no site-host restriction in `site_api`.
    pub fn new(endpoint: &str, cookie: &str) -> Result<Self> {
        Self::build(endpoint, None, cookie)
    }

    /// Client that sends requests to `endpoint` but only accepts `site_api`
    /// URLs on `tenant_host` (they differ when the endpoint is overridden
    /// for tests).
    pub fn with_host(endpoint: &str, tenant_host: &str, cookie: &str) -> Result<Self> {
        Self::build(endpoint, Some(tenant_host.to_ascii_lowercase()), cookie)
    }

    fn build(endpoint: &str, tenant_host: Option<String>, cookie: &str) -> Result<Self> {
        let endpoint_url = url::Url::parse(endpoint)
            .map_err(|e| CliError::Input(format!("invalid REST endpoint '{endpoint}': {e}")))?;
        let mut cookie_value = HeaderValue::from_str(cookie)
            .map_err(|_| CliError::Auth("stored session cookie is not a valid header".into()))?;
        cookie_value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, cookie_value);
        headers.insert(ACCEPT, HeaderValue::from_static(ACCEPT_NOMETADATA));
        let http = reqwest::Client::builder()
            .user_agent(format!("sharepoint-cli/{}", env!("CARGO_PKG_VERSION")))
            .default_headers(headers)
            // Browser-session cookies must never be replayed against a
            // redirect target (e.g. a sign-in page on another origin), so
            // redirects are surfaced as errors instead of being followed.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            http,
            endpoint: endpoint_url,
            tenant_host,
        })
    }

    /// Base URL with no trailing slash, for concatenating API paths onto.
    fn base_str(&self) -> &str {
        self.endpoint.as_str().trim_end_matches('/')
    }

    /// Build a client from config: cookie from `SHAREPOINT_COOKIE` or the session store.
    pub fn from_config(cfg: &ResolvedConfig, session_path: &std::path::Path) -> Result<Self> {
        let host = cfg.rest.require_host()?;
        let endpoint = cfg.rest.endpoint()?;
        let cookie = match &cfg.rest.cookie_override {
            Some(c) => c.clone(),
            None => crate::auth::session_store::get(session_path, host)?
                .map(|s| s.cookie)
                .ok_or_else(|| {
                    CliError::Auth(format!(
                        "no browser session for {host}; run `sharepoint auth login`"
                    ))
                })?,
        };
        Self::with_host(&endpoint, host, &cookie)
    }

    /// Base URL; used to validate page tokens.
    pub fn endpoint(&self) -> &str {
        self.base_str()
    }

    /// Absolute URL for `api_path` (starting with `/_api`) in the web at `site_url`.
    pub fn site_api(&self, site_url: &str, api_path: &str) -> Result<String> {
        let parsed = url::Url::parse(site_url)
            .map_err(|e| CliError::Input(format!("invalid site URL '{site_url}': {e}")))?;
        if let Some(tenant_host) = &self.tenant_host {
            let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
            if host != *tenant_host {
                return Err(CliError::Input(format!(
                    "site '{site_url}' is not on the configured SharePoint host '{tenant_host}'"
                )));
            }
        }
        let site_path = parsed.path().trim_end_matches('/');
        Ok(format!("{}{site_path}{api_path}", self.base_str()))
    }

    /// Absolute URL for `api_path` on the tenant root web.
    pub fn root_api(&self, api_path: &str) -> String {
        format!("{}{api_path}", self.base_str())
    }

    /// Public URL of `server_relative` on the tenant host, for display.
    /// Falls back to the endpoint origin for clients built without a tenant host.
    pub(crate) fn public_url(&self, server_relative: &str) -> String {
        match &self.tenant_host {
            Some(host) => format!("https://{host}{server_relative}"),
            None => format!("{}{server_relative}", self.base_str()),
        }
    }

    fn check_host(&self, url: &str) -> Result<()> {
        let target =
            url::Url::parse(url).map_err(|_| CliError::Input(format!("invalid URL '{url}'")))?;
        if target.origin() != self.endpoint.origin() {
            return Err(CliError::Input(format!(
                "refusing to send session cookies to untrusted host {:?}",
                target.host_str()
            )));
        }
        Ok(())
    }

    /// GET with retry on 429/5xx; returns the successful response.
    pub(crate) async fn get(&self, url: &str) -> Result<Response> {
        self.check_host(url)?;
        let mut attempt = 0;
        loop {
            let resp = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|e| CliError::Http(format!("sharepoint GET {url}: {e}")))?;
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            let retryable = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            if retryable && attempt < MAX_RETRIES {
                let wait = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_retry_after)
                    .unwrap_or_else(|| Duration::from_millis(500 * 2u64.pow(attempt)));
                tokio::time::sleep(wait).await;
                attempt += 1;
                continue;
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(map_status(status, &body));
        }
    }

    /// Identity behind the session; `Auth` error when the cookies are not accepted.
    pub async fn current_user(&self) -> Result<CurrentUser> {
        self.get_json(&self.root_api("/_api/web/currentuser?$select=Title,Email,LoginName"))
            .await
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.get(url).await?;
        let text = resp.text().await?;
        serde_json::from_str(&text)
            .map_err(|e| CliError::Other(format!("parse SharePoint response: {e}")))
    }
}

/// The signed-in user, from `/_api/web/currentuser`.
#[derive(Debug, serde::Deserialize)]
pub struct CurrentUser {
    #[serde(rename = "Title", default)]
    pub title: String,
    #[serde(rename = "Email", default)]
    pub email: String,
    #[serde(rename = "LoginName", default)]
    pub login_name: String,
}

/// Extract the message from an `odata.error` (nometadata) or `error` (verbose) body.
fn extract_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = v.get("odata.error").or_else(|| v.get("error"))?;
    err.get("message")
        .and_then(|m| m.get("value").or(Some(m)))
        .and_then(|m| m.as_str())
        .map(str::to_owned)
}

fn map_status(status: StatusCode, body: &str) -> CliError {
    // The client never follows redirects (see `build`), so a 3xx here means
    // the session cookie is no longer accepted and SharePoint is trying to
    // bounce the request to a sign-in page.
    if status.is_redirection() {
        return CliError::Auth(format!(
            "SharePoint {}: redirected to sign-in{RELOGIN_HINT}",
            status.as_u16()
        ));
    }
    let primary = extract_error_message(body).unwrap_or_else(|| status.to_string());
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => CliError::Auth(format!(
            "SharePoint {}: {primary}{RELOGIN_HINT}",
            status.as_u16()
        )),
        StatusCode::NOT_FOUND => CliError::NotFound(primary),
        StatusCode::TOO_MANY_REQUESTS => CliError::RateLimit,
        _ => CliError::Api {
            status: status.as_u16(),
            message: primary,
        },
    }
}

/// Escape a server-relative path for `…ByServerRelativePath(decodedurl='…')`:
/// double single quotes (OData literal) then percent-encode, keeping `/`.
pub(crate) fn odata_path_literal(path: &str) -> String {
    percent_encode_keep_slash(&path.replace('\'', "''"))
}

/// Percent-encode everything outside RFC 3986 unreserved, keeping `/`.
pub(crate) fn percent_encode_keep_slash(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => write!(out, "%{b:02X}").unwrap(),
        }
    }
    out
}

/// Rows of a `/_api/search/query` response as key→value maps, plus paging info.
pub(crate) struct SearchPage {
    pub(crate) rows: Vec<std::collections::HashMap<String, String>>,
    /// URL of the next page, if `startrow + rowcount < total`.
    pub(crate) next_url: Option<String>,
}

pub(crate) const SEARCH_ROW_LIMIT: usize = 50;

/// Fetch one search page. `url` must carry `startrow=N` as its last query
/// parameter (all builders in this module put it last).
pub(crate) async fn search_page(client: &RestClient, url: &str) -> Result<SearchPage> {
    let v: serde_json::Value = client.get_json(url).await?;
    let relevant = &v["PrimaryQueryResult"]["RelevantResults"];
    let total = relevant["TotalRows"].as_u64().unwrap_or(0) as usize;
    let rows: Vec<_> = relevant["Table"]["Rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    row["Cells"]
                        .as_array()
                        .map(|cells| {
                            cells
                                .iter()
                                .filter_map(|c| {
                                    Some((
                                        c["Key"].as_str()?.to_string(),
                                        c["Value"].as_str()?.to_string(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let start = url
        .rsplit_once("startrow=")
        .and_then(|(_, n)| n.parse::<usize>().ok())
        .unwrap_or(0);
    let next_start = start + rows.len();
    let next_url = (!rows.is_empty() && next_start < total).then(|| {
        let (prefix, _) = url.rsplit_once("startrow=").unwrap_or((url, ""));
        format!("{prefix}startrow={next_start}")
    });
    Ok(SearchPage { rows, next_url })
}

/// Quote a KQL / OData string literal: `'…'` with inner `'` doubled.
pub(crate) fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odata_literal_escapes_quotes_and_spaces() {
        assert_eq!(
            odata_path_literal("/sites/M/Shared Documents/O'Brien.docx"),
            "/sites/M/Shared%20Documents/O%27%27Brien.docx"
        );
    }

    #[test]
    fn quoted_doubles_inner_quotes() {
        assert_eq!(quoted("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn extracts_nometadata_error() {
        let body =
            r#"{"odata.error":{"code":"x","message":{"lang":"en","value":"File Not Found."}}}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("File Not Found.")
        );
    }

    #[test]
    fn map_status_404_is_not_found() {
        assert!(matches!(
            map_status(StatusCode::NOT_FOUND, ""),
            CliError::NotFound(_)
        ));
    }

    #[test]
    fn map_status_429_is_rate_limit() {
        assert!(matches!(
            map_status(StatusCode::TOO_MANY_REQUESTS, ""),
            CliError::RateLimit
        ));
    }

    #[test]
    fn map_status_redirect_is_auth_with_hint() {
        match map_status(StatusCode::FOUND, "") {
            CliError::Auth(m) => {
                assert!(m.contains("redirected to sign-in"), "{m}");
                assert!(m.contains("sharepoint auth login"), "{m}");
            }
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[test]
    fn extracts_verbose_error_message() {
        let body = r#"{"error":{"message":"Something went wrong."}}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("Something went wrong.")
        );
    }

    #[test]
    fn map_status_500_with_verbose_body_is_api_with_message() {
        match map_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"Something went wrong."}}"#,
        ) {
            CliError::Api { status, message } => {
                assert_eq!(status, 500);
                assert_eq!(message, "Something went wrong.");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }
}
