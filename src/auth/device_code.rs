//! Device-code flow against `login.microsoftonline.com/<tenant>/oauth2/v2.0/`.
//!
//! Polling state machine handles all the cases the spec calls out:
//! - 200 OK → success
//! - 400 authorization_pending → keep polling at the same interval
//! - 400 slow_down → bump interval by +5s
//! - 400 bad_verification_code → keep polling (transient)
//! - 400 authorization_declined / expired_token / access_denied → terminal failure
//! - any other 4xx/5xx → terminal failure with structured error (body never leaked)
//!
//! Polling budget tracks scheduled sleep time only; real wall clock can exceed
//! `expires_in` if requests are slow. The server's `expired_token` response is
//! the authoritative cap.

use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use tokio::time::sleep;

use crate::error::{CliError, Result};
use crate::util;

/// Maximum number of automatic retries on transient token-endpoint errors
/// (5xx, 429, connection failures). Does not apply to the normal
/// authorization_pending / slow_down state-machine cycles.
const MAX_TOKEN_RETRIES: u32 = 3;

/// Structured OAuth2 error response from the token endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OAuth2Error {
    pub error: String,
    pub error_description: Option<String>,
    pub error_uri: Option<String>,
}

/// Strip token-shaped substrings from any string destined for an error message.
///
/// Prevents OAuth token values from leaking into `CliError` messages when
/// unexpected fields appear in malformed responses. Scans for JSON key-value
/// pairs where the key is a known token field name and replaces the value with
/// `[REDACTED]`.
pub fn redact_token_fields(s: &str) -> String {
    const TOKEN_FIELDS: &[&str] = &["access_token", "refresh_token", "id_token"];
    let mut result = s.to_string();
    for field in TOKEN_FIELDS {
        // Replace `"<field>":"<anything>"` patterns (with optional whitespace).
        let key_pattern = format!("\"{field}\"");
        let mut search_from = 0;
        while let Some(key_pos) = result[search_from..].find(&key_pattern) {
            let abs_key_end = search_from + key_pos + key_pattern.len();
            // Skip whitespace after the key.
            let after_key = result[abs_key_end..].trim_start();
            if !after_key.starts_with(':') {
                search_from = abs_key_end;
                continue;
            }
            let colon_offset = result[abs_key_end..].len() - after_key.len() + 1;
            let abs_colon_end = abs_key_end + colon_offset;
            let after_colon = result[abs_colon_end..].trim_start();
            if !after_colon.starts_with('"') {
                search_from = abs_colon_end;
                continue;
            }
            // Find where the value string ends (first unescaped closing quote).
            let value_start_offset = result[abs_colon_end..].len() - after_colon.len();
            let abs_value_open = abs_colon_end + value_start_offset; // points at opening "
            let value_chars = &result[abs_value_open + 1..]; // skip opening "
            let mut value_len = 0;
            let mut escaped = false;
            for ch in value_chars.chars() {
                value_len += ch.len_utf8();
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                }
            }
            let abs_value_close = abs_value_open + 1 + value_len; // points just past closing "
            let replacement = format!("\"{}\":\"[REDACTED]\"", field);
            result.replace_range(search_from + key_pos..abs_value_close, &replacement);
            // Advance past the replacement to avoid re-processing.
            search_from = search_from + key_pos + replacement.len();
        }
    }
    result
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_in: u64,
    pub scope: String,
}

/// Identity claims we extract from the id_token (`oid`, `tid`,
/// `preferred_username`, `name`).
#[derive(Debug, Clone)]
pub struct IdClaims {
    pub oid: String,
    pub tid: String,
    pub preferred_username: String,
    pub name: String,
}

#[derive(Deserialize)]
struct RawTokenSuccess {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
}

pub async fn request_device_code(
    client: &reqwest::Client,
    login_endpoint: &str,
    tenant: &str,
    client_id: &str,
    scope: &str,
) -> Result<DeviceCodeResponse> {
    let url = format!("{login_endpoint}/{tenant}/oauth2/v2.0/devicecode");
    let resp = client
        .post(&url)
        .form(&[("client_id", client_id), ("scope", scope)])
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        let err_msg = match serde_json::from_str::<OAuth2Error>(&body) {
            Ok(e) => format!(
                "device-code request failed ({status}): {}: {}",
                e.error,
                redact_token_fields(e.error_description.as_deref().unwrap_or_default())
            ),
            Err(_) => {
                tracing::debug!("device-code request failed ({status}): {body}");
                format!("device-code request failed with HTTP {status} and unparseable body")
            }
        };
        return Err(CliError::Auth(err_msg));
    }
    let parsed: DeviceCodeResponse = resp.json().await?;
    Ok(parsed)
}

/// POST `url` with the given form fields, retrying on 5xx / 429 / connection
/// failures up to [`MAX_TOKEN_RETRIES`] times with exponential backoff.
/// Honors `Retry-After` response headers. Returns the final `(status, body)`.
///
/// Terminal 4xx responses (except 408 Request Timeout and 429) are returned
/// immediately without retry — they indicate a hard auth failure.
async fn send_with_retry(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<(reqwest::StatusCode, String)> {
    let mut attempt: u32 = 0;
    loop {
        let result = client.post(url).form(form).send().await;

        match result {
            Err(e) if attempt < MAX_TOKEN_RETRIES => {
                // Connection-level failure (timeout, reset, DNS): retry.
                let backoff = 2u64.pow(attempt);
                tracing::debug!(
                    "token endpoint connection error (attempt {attempt}): {e}; retrying in {backoff}s"
                );
                sleep(Duration::from_secs(backoff)).await;
                attempt += 1;
                continue;
            }
            Err(e) => return Err(CliError::Http(format!("token endpoint: {e}"))),
            Ok(resp) => {
                let status = resp.status();
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(util::parse_retry_after);
                let body = resp.text().await.unwrap_or_default();

                // Retry on 429 and 5xx; return everything else immediately.
                let should_retry = (status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status.is_server_error())
                    && attempt < MAX_TOKEN_RETRIES;

                if should_retry {
                    let secs = retry_after
                        .map(|d| d.as_secs())
                        .unwrap_or_else(|| 2u64.pow(attempt));
                    tracing::debug!(
                        "token endpoint transient error {status} (attempt {attempt}); retrying in {secs}s"
                    );
                    sleep(Duration::from_secs(secs)).await;
                    attempt += 1;
                    continue;
                }

                return Ok((status, body));
            }
        }
    }
}

pub async fn poll_for_token(
    client: &reqwest::Client,
    login_endpoint: &str,
    tenant: &str,
    client_id: &str,
    device_code: &str,
    initial_interval: u64,
    expires_in: u64,
) -> Result<TokenResponse> {
    let url = format!("{login_endpoint}/{tenant}/oauth2/v2.0/token");
    let mut interval = initial_interval.max(1);
    let mut elapsed: u64 = 0;
    loop {
        if elapsed >= expires_in {
            return Err(CliError::Auth(
                "device code expired before sign-in completed; try again".into(),
            ));
        }
        sleep(Duration::from_secs(interval)).await;
        elapsed = elapsed.saturating_add(interval);

        // Inner retry loop handles transient errors (5xx, 429, connection
        // failures) without consuming device-code expiry budget.
        let (status, body) = send_with_retry(
            client,
            &url,
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", device_code),
            ],
        )
        .await?;

        if status.is_success() {
            let raw: RawTokenSuccess = serde_json::from_str(&body).map_err(|e| {
                tracing::debug!("token response body (parse error): {body}");
                CliError::Auth(format!("token response was not valid JSON: {e}"))
            })?;
            return Ok(TokenResponse {
                access_token: raw.access_token,
                refresh_token: raw
                    .refresh_token
                    .ok_or_else(|| CliError::Auth("no refresh_token returned".into()))?,
                id_token: raw.id_token.ok_or_else(|| {
                    CliError::Auth("no id_token returned (need 'openid' scope)".into())
                })?,
                expires_in: raw.expires_in,
                scope: raw.scope.unwrap_or_default(),
            });
        }

        // Non-200: classify the OAuth error code.
        let parsed: std::result::Result<OAuth2Error, _> = serde_json::from_str(&body);
        match parsed {
            Ok(err) => match err.error.as_str() {
                "authorization_pending" | "bad_verification_code" => {}
                "slow_down" => {
                    interval = interval.saturating_add(5);
                }
                "authorization_declined" => {
                    return Err(CliError::Auth("user declined the sign-in request".into()));
                }
                "expired_token" => {
                    return Err(CliError::Auth(
                        "device code expired before sign-in completed; try again".into(),
                    ));
                }
                "access_denied" => {
                    if err
                        .error_description
                        .as_deref()
                        .unwrap_or("")
                        .contains("AADSTS65001")
                    {
                        return Err(CliError::Auth(
                            "admin consent required for this app in your tenant; \
                             ask your IT admin to grant consent for sharepoint-cli, \
                             then try again. Details: AADSTS65001"
                                .into(),
                        ));
                    }
                    return Err(CliError::Auth(format!(
                        "access denied: {}",
                        redact_token_fields(err.error_description.as_deref().unwrap_or_default())
                    )));
                }
                other => {
                    return Err(CliError::Auth(format!(
                        "device-code polling failed: {other}: {}",
                        redact_token_fields(err.error_description.as_deref().unwrap_or_default())
                    )));
                }
            },
            Err(_) => {
                tracing::debug!(
                    "device-code polling failed ({status}) with unparseable body: {body}"
                );
                return Err(CliError::Auth(format!(
                    "token endpoint returned HTTP {status} with unparseable body"
                )));
            }
        }
    }
}

pub async fn refresh(
    client: &reqwest::Client,
    login_endpoint: &str,
    tenant: &str,
    client_id: &str,
    refresh_token: &str,
    scope: &str,
) -> Result<TokenResponse> {
    let url = format!("{login_endpoint}/{tenant}/oauth2/v2.0/token");
    let (status, body) = send_with_retry(
        client,
        &url,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", scope),
        ],
    )
    .await?;
    if status.is_success() {
        let raw: RawTokenSuccess = serde_json::from_str(&body).map_err(|e| {
            tracing::debug!("refresh response body (parse error): {body}");
            CliError::Auth(format!("refresh response was not valid JSON: {e}"))
        })?;
        return Ok(TokenResponse {
            access_token: raw.access_token,
            refresh_token: raw
                .refresh_token
                .unwrap_or_else(|| refresh_token.to_string()),
            id_token: raw.id_token.unwrap_or_default(),
            expires_in: raw.expires_in,
            scope: raw.scope.unwrap_or_default(),
        });
    }
    let parsed: std::result::Result<OAuth2Error, _> = serde_json::from_str(&body);
    match parsed {
        Ok(err) if err.error == "invalid_grant" => Err(CliError::Auth(
            "refresh token is no longer valid; run `sharepoint auth login`".into(),
        )),
        Ok(err) => Err(CliError::Auth(format!(
            "refresh failed: {}: {}",
            err.error,
            redact_token_fields(err.error_description.as_deref().unwrap_or_default())
        ))),
        Err(_) => {
            tracing::debug!("refresh failed ({status}) with unparseable body: {body}");
            Err(CliError::Auth(format!(
                "token endpoint returned HTTP {status} with unparseable body"
            )))
        }
    }
}

/// Decode the middle segment of a JWT (no signature verification — we trust
/// the channel the token came over, like every other MSAL-style client).
pub fn decode_id_token(id_token: &str) -> Result<IdClaims> {
    let mid = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| CliError::Auth("id_token has no payload segment".into()))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(mid)
        .map_err(|e| CliError::Auth(format!("id_token base64 decode: {e}")))?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Auth(format!("id_token JSON decode: {e}")))?;
    let oid = json
        .get("oid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::Auth("id_token missing 'oid' claim".into()))?;
    let tid = json
        .get("tid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CliError::Auth("id_token missing 'tid' claim".into()))?;
    let preferred_username = json
        .get("preferred_username")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(IdClaims {
        oid: oid.into(),
        tid: tid.into(),
        preferred_username,
        name,
    })
}

/// Build the full scope string we request in v0.1.
pub fn default_scope(read_only: bool) -> &'static str {
    if read_only {
        "openid profile offline_access User.Read Files.Read.All Sites.Read.All"
    } else {
        "openid profile offline_access User.Read Files.ReadWrite.All Sites.Read.All"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn make_id_token(payload: &serde_json::Value) -> String {
        let header = "{}";
        let header_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);
        let body_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(payload).unwrap());
        format!("{header_b64}.{body_b64}.sig")
    }

    #[test]
    fn decode_id_token_extracts_required_claims() {
        let token = make_id_token(&serde_json::json!({
            "oid": "OID-123",
            "tid": "TID-456",
            "preferred_username": "alice@contoso.com",
            "name": "Alice"
        }));
        let claims = decode_id_token(&token).unwrap();
        assert_eq!(claims.oid, "OID-123");
        assert_eq!(claims.tid, "TID-456");
        assert_eq!(claims.preferred_username, "alice@contoso.com");
        assert_eq!(claims.name, "Alice");
    }

    #[test]
    fn decode_id_token_errors_when_oid_missing() {
        let token = make_id_token(&serde_json::json!({"tid": "T"}));
        assert!(decode_id_token(&token).is_err());
    }

    #[test]
    fn default_scope_includes_files_readwrite_when_not_readonly() {
        assert!(default_scope(false).contains("Files.ReadWrite.All"));
        assert!(!default_scope(false).contains("Files.Read.All "));
    }

    #[test]
    fn default_scope_uses_files_read_when_readonly() {
        assert!(default_scope(true).contains("Files.Read.All"));
        assert!(!default_scope(true).contains("Files.ReadWrite.All"));
    }

    #[test]
    fn redact_token_fields_removes_access_token_value() {
        let body = r#"{"error":"invalid_client","access_token":"SECRET-TOKEN-VALUE","foo":"bar"}"#;
        let redacted = redact_token_fields(body);
        assert!(
            !redacted.contains("SECRET-TOKEN-VALUE"),
            "access_token value must not appear in redacted output: {redacted}"
        );
        assert!(
            redacted.contains("[REDACTED]"),
            "redacted marker must appear: {redacted}"
        );
        // Non-token fields must be preserved.
        assert!(
            redacted.contains("invalid_client"),
            "error field must survive: {redacted}"
        );
    }

    #[test]
    fn redact_token_fields_handles_refresh_and_id_token() {
        let body = r#"{"refresh_token":"RT-SECRET","id_token":"IT-SECRET","ok":"keep"}"#;
        let redacted = redact_token_fields(body);
        assert!(!redacted.contains("RT-SECRET"));
        assert!(!redacted.contains("IT-SECRET"));
        assert!(redacted.contains("keep"));
        assert_eq!(redacted.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn redact_token_fields_is_noop_when_no_token_fields_present() {
        let s = r#"{"error":"server_error","error_description":"oops"}"#;
        assert_eq!(redact_token_fields(s), s);
    }

    #[tokio::test]
    async fn poll_for_token_unparseable_5xx_does_not_leak_body() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/tenant/oauth2/v2.0/token"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_string(r#"{"access_token":"SHOULD_NOT_APPEAR","x":"y"}"#),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let err = poll_for_token(
            &client,
            &server.uri(),
            "tenant",
            "client-id",
            "DEV-CODE",
            1,
            10,
        )
        .await
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            !msg.contains("SHOULD_NOT_APPEAR"),
            "raw body must not appear in error: {msg}"
        );
        assert!(
            msg.contains("503") || msg.contains("unparseable"),
            "error must mention status or unparseable: {msg}"
        );
    }
}
