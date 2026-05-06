//! Verifies that transient 5xx errors on the token endpoint are retried and
//! that retry exhaustion produces a structured error rather than a panic.

use std::time::Duration;

use sharepoint_cli::auth::device_code::{poll_for_token, refresh};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn id_token(payload: &serde_json::Value) -> String {
    use base64::Engine;
    let h = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}");
    let p = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(payload).unwrap());
    format!("{h}.{p}.sig")
}

/// A single 503 before a 200 succeeds: the retry recovers.
#[tokio::test]
async fn poll_retries_on_single_503_then_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "AT",
            "refresh_token": "RT",
            "id_token": id_token(&serde_json::json!({
                "oid": "OID-1",
                "tid": "TID-1",
                "preferred_username": "alice@contoso.com",
                "name": "Alice"
            })),
            "expires_in": 3600,
            "scope": "User.Read"
        })))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let tok = tokio::time::timeout(
        Duration::from_secs(15),
        poll_for_token(&client, &server.uri(), "tenant", "cid", "DEV", 1, 30),
    )
    .await
    .expect("timed out")
    .expect("should succeed after retry");

    assert_eq!(tok.access_token, "AT");
}

/// Four consecutive 503s exhaust retries (MAX_TOKEN_RETRIES = 3 means attempts
/// 0..=3 → 4 total; after 3 retries the 4th attempt at the outer poll loop is
/// returned as an error).
#[tokio::test]
async fn poll_fails_after_retry_exhaustion() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = tokio::time::timeout(
        Duration::from_secs(30),
        poll_for_token(&client, &server.uri(), "tenant", "cid", "DEV", 1, 60),
    )
    .await
    .expect("timed out")
    .unwrap_err();

    let msg = err.to_string();
    // Must be a structured error — no raw body leakage.
    assert!(
        msg.contains("503") || msg.contains("unparseable"),
        "error message should mention status: {msg}"
    );
}

/// Refresh also recovers from a single 503.
#[tokio::test]
async fn refresh_retries_on_single_503_then_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "AT-NEW",
            "refresh_token": "RT-NEW",
            "expires_in": 3600,
            "scope": "User.Read"
        })))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let tok = tokio::time::timeout(
        Duration::from_secs(15),
        refresh(&client, &server.uri(), "tenant", "cid", "RT-OLD", "User.Read"),
    )
    .await
    .expect("timed out")
    .expect("should succeed after retry");

    assert_eq!(tok.access_token, "AT-NEW");
}
