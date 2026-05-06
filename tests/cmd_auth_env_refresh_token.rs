//! Integration tests for the `SHAREPOINT_REFRESH_TOKEN` environment variable.
//!
//! When the env var is set and no valid cached token exists, the CLI must
//! bootstrap an access token via the OAuth refresh exchange rather than
//! immediately prompting for a device-code login.  This enables headless/CI
//! usage without persistent cache files.

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Happy path: SHAREPOINT_REFRESH_TOKEN is set, no cache exists, mock refresh
/// endpoint returns tokens, and a subsequent Graph call succeeds.
#[tokio::test]
async fn env_refresh_token_bootstraps_auth_when_no_cache() {
    let login_server = MockServer::start().await;
    let graph_server = MockServer::start().await;

    // The refresh endpoint receives the env-var token and issues new tokens.
    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT-FROM-REFRESH",
            "refresh_token": "RT-NEW",
            "expires_in": 3600,
            "scope": "Files.ReadWrite.All offline_access"
        })))
        .mount(&login_server)
        .await;

    // A simple Graph call that needs auth (auth status reads from cache, so use
    // sites list which actually calls Graph).
    Mock::given(method("GET"))
        .and(path("/me/followedSites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": []
        })))
        .mount(&graph_server)
        .await;

    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_REFRESH_TOKEN", "ENV-RT")
        .env("MICROSOFT_LOGIN_ENDPOINT", login_server.uri())
        .env("MICROSOFT_GRAPH_ENDPOINT", graph_server.uri())
        .args(["--json", "sites", "list"])
        .assert()
        .success()
        .stdout(contains("\"items\""));
}

/// Failure path: SHAREPOINT_REFRESH_TOKEN is set but the refresh endpoint
/// returns `invalid_grant`.  The CLI must exit with code 3 (auth failure) and
/// must NOT fall back to interactive device-code flow.
#[tokio::test]
async fn env_refresh_token_fails_with_auth_error_on_invalid_grant() {
    let login_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Refresh token has expired"
        })))
        .mount(&login_server)
        .await;

    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_REFRESH_TOKEN", "REVOKED-RT")
        .env("MICROSOFT_LOGIN_ENDPOINT", login_server.uri())
        .env("MICROSOFT_GRAPH_ENDPOINT", "https://graph.example.test")
        .args(["--json", "sites", "list"])
        .assert()
        .failure()
        .code(3);
}
