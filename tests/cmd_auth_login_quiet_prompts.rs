//! Verifies that the device-code prompt (verification URL + user code) is
//! always emitted on stderr even when --quiet or --json is set.
//!
//! The device-code prompt is an interactive instruction, not optional status.
//! Suppressing it under --quiet would hang the user silently.

use assert_cmd::Command;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn jwt(tid: &str, oid: &str, upn: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
    let claims = json!({"tid": tid, "oid": oid, "preferred_username": upn, "name": "Test"});
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    format!("{header}.{payload}.")
}

async fn setup_server(tenant: &str) -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(format!("/{tenant}/oauth2/v2.0/devicecode")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "DEV",
            "user_code": "UNIQUE-CODE",
            "verification_uri": "https://example.com/device",
            "expires_in": 900,
            "interval": 1,
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("/{tenant}/oauth2/v2.0/token")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT",
            "refresh_token": "RT",
            "id_token": jwt(tenant, "OID-1", "alice@contoso.com"),
            "expires_in": 3600,
            "scope": "User.Read",
        })))
        .mount(&server)
        .await;

    server
}

/// --quiet must not suppress the device-code prompt.
#[tokio::test]
async fn auth_login_quiet_still_shows_device_code_prompt_on_stderr() {
    let server = setup_server("t1").await;
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("MICROSOFT_LOGIN_ENDPOINT", server.uri())
        .env("SHAREPOINT_TENANT_ID", "t1")
        .env("SHAREPOINT_CLIENT_ID", "cid-1")
        .args(["--quiet", "auth", "login"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected success; stderr: {stderr}"
    );
    assert!(
        stderr.contains("UNIQUE-CODE"),
        "--quiet must not suppress the user code; stderr: {stderr}"
    );
    assert!(
        stderr.contains("https://example.com/device"),
        "--quiet must not suppress the verification URL; stderr: {stderr}"
    );
}

/// --json --quiet must still show the prompt on stderr (not on stdout).
#[tokio::test]
async fn auth_login_json_quiet_shows_prompt_on_stderr_not_stdout() {
    let server = setup_server("t2").await;
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("MICROSOFT_LOGIN_ENDPOINT", server.uri())
        .env("SHAREPOINT_TENANT_ID", "t2")
        .env("SHAREPOINT_CLIENT_ID", "cid-2")
        .args(["--json", "--quiet", "auth", "login"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "expected success; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stderr.contains("UNIQUE-CODE"),
        "--json --quiet must emit user code on stderr; stderr: {stderr}"
    );
    // The verification URL must NOT appear on stdout (JSON stream must remain clean).
    assert!(
        !stdout.contains("UNIQUE-CODE"),
        "device-code prompt must not pollute stdout; stdout: {stdout}"
    );
}
