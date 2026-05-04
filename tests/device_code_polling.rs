//! Wiremock test for the device-code polling state machine.
//! Covers authorization_pending → slow_down → success.

use std::time::Duration;

use sharepoint_cli::auth::device_code::poll_for_token;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn poll_handles_pending_then_slow_down_then_success() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "authorization_pending",
            "error_description": "user has not yet entered the code"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "slow_down",
            "error_description": "too fast"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "AT",
            "refresh_token": "RT",
            "id_token": header_dot_payload_dot_sig(&serde_json::json!({
                "oid": "OID-1",
                "tid": "TID-1",
                "preferred_username": "alice@contoso.com",
                "name": "Alice"
            })),
            "expires_in": 3600,
            "scope": "User.Read Files.ReadWrite.All"
        })))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let token = tokio::time::timeout(
        Duration::from_secs(15),
        poll_for_token(
            &client,
            &server.uri(),
            "contoso",
            "client-id",
            "DEV-CODE",
            1,
            30,
        ),
    )
    .await
    .expect("poll did not finish in time")
    .expect("poll returned error");

    assert_eq!(token.access_token, "AT");
    assert_eq!(token.refresh_token, "RT");
}

#[tokio::test]
async fn poll_returns_auth_error_when_user_declines() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "authorization_declined",
            "error_description": "user said no"
        })))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = poll_for_token(
        &client,
        &server.uri(),
        "contoso",
        "client-id",
        "DEV-CODE",
        1,
        30,
    )
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("declined"));
}

#[tokio::test]
async fn poll_detects_admin_consent_required() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "access_denied",
            "error_description": "AADSTS65001: The user or administrator has not consented to use the application"
        })))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = poll_for_token(
        &client,
        &server.uri(),
        "contoso",
        "client-id",
        "DEV-CODE",
        1,
        30,
    )
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("admin consent required"));
}

fn header_dot_payload_dot_sig(payload: &serde_json::Value) -> String {
    use base64::Engine;
    let h = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
    let p = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(payload).unwrap());
    format!("{h}.{p}.sig")
}
