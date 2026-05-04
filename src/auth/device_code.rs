//! Device-code flow against `login.microsoftonline.com/<tenant>/oauth2/v2.0/`.
//!
//! Polling state machine handles all the cases the spec calls out:
//! - 200 OK → success
//! - 400 authorization_pending → keep polling at the same interval
//! - 400 slow_down → bump interval by +5s
//! - 400 bad_verification_code → keep polling (transient)
//! - 400 authorization_declined / expired_token / access_denied → terminal failure
//! - any other 4xx/5xx → terminal failure with body in message
//!
//! Polling budget tracks scheduled sleep time only; real wall clock can exceed
//! `expires_in` if requests are slow. The server's `expired_token` response is
//! the authoritative cap.

use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use tokio::time::sleep;

use crate::error::{CliError, Result};

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

#[derive(Deserialize)]
struct RawTokenError {
    error: String,
    error_description: Option<String>,
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
        return Err(CliError::Auth(format!(
            "device-code request failed ({status}): {body}"
        )));
    }
    let parsed: DeviceCodeResponse = resp.json().await?;
    Ok(parsed)
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

        let resp = client
            .post(&url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", device_code),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            let raw: RawTokenSuccess = serde_json::from_str(&body).map_err(|e| {
                CliError::Auth(format!("token response was not JSON: {e}; body={body}"))
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
        let parsed: std::result::Result<RawTokenError, _> = serde_json::from_str(&body);
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
                        err.error_description.unwrap_or_default()
                    )));
                }
                other => {
                    return Err(CliError::Auth(format!(
                        "device-code polling failed: {other}: {}",
                        err.error_description.unwrap_or_default()
                    )));
                }
            },
            Err(_) => {
                return Err(CliError::Auth(format!(
                    "device-code polling failed ({status}): {body}"
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
    let resp = client
        .post(&url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", scope),
        ])
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.is_success() {
        let raw: RawTokenSuccess = serde_json::from_str(&body).map_err(|e| {
            CliError::Auth(format!("refresh response was not JSON: {e}; body={body}"))
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
    let parsed: std::result::Result<RawTokenError, _> = serde_json::from_str(&body);
    match parsed {
        Ok(err) if err.error == "invalid_grant" => Err(CliError::Auth(
            "refresh token is no longer valid; run `sharepoint auth login`".into(),
        )),
        Ok(err) => Err(CliError::Auth(format!(
            "refresh failed: {}: {}",
            err.error,
            err.error_description.unwrap_or_default()
        ))),
        Err(_) => Err(CliError::Auth(format!("refresh failed ({status}): {body}"))),
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
}
