#![cfg(target_os = "linux")]

//! After a successful device-code login with a domain tenant configured,
//! the resolved tenant GUID from the id token must be persisted to the
//! profile so that subsequent commands look up the cache by GUID.

use assert_cmd::Command;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn jwt_with_tid(tid: &str, oid: &str, upn: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
    let claims = json!({"tid": tid, "oid": oid, "preferred_username": upn, "name": "Alice"});
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    format!("{header}.{payload}.")
}

#[tokio::test]
async fn login_canonicalizes_domain_tenant_to_guid_in_config() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/contoso.onmicrosoft.com/oauth2/v2.0/devicecode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "DEV", "user_code": "U-CODE",
            "verification_uri": "https://login.example/device",
            "expires_in": 900, "interval": 1,
        })))
        .mount(&server)
        .await;

    let id_token = jwt_with_tid(
        "11111111-1111-1111-1111-111111111111",
        "OID-1",
        "alice@contoso.com",
    );
    Mock::given(method("POST"))
        .and(path("/contoso.onmicrosoft.com/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AT", "refresh_token": "RT",
            "expires_in": 3600, "scope": "Files.Read.All offline_access",
            "id_token": id_token,
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("MICROSOFT_LOGIN_ENDPOINT", server.uri())
        .env("SHAREPOINT_TENANT_ID", "contoso.onmicrosoft.com")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args(["auth", "login"])
        .assert()
        .success();

    let cfg_path = dir.path().join("sharepoint").join("config.toml");
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        cfg.contains("11111111-1111-1111-1111-111111111111"),
        "config should contain the GUID tenant after login; got:\n{cfg}"
    );
    assert!(
        !cfg.contains("contoso.onmicrosoft.com"),
        "config should not still contain the domain tenant; got:\n{cfg}"
    );
}
