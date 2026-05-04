//! Verifies GraphClient retries 429 with Retry-After honored.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sharepoint_cli::auth::AuthContext;
use sharepoint_cli::config::ResolvedConfig;
use sharepoint_cli::graph::GraphClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn graph_retries_on_429_then_succeeds() {
    let server = MockServer::start().await;

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    Mock::given(method("GET"))
        .and(path("/me"))
        .respond_with(move |_req: &wiremock::Request| {
            let n = counter_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "1")
                    .set_body_string("rate limited")
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "user-1",
                    "userPrincipalName": "alice@contoso.com"
                }))
            }
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cfg = ResolvedConfig {
        profile_name: "default".into(),
        tenant_id: Some("t".into()),
        client_id: Some("c".into()),
        default_site: None,
        read_only: false,
        site_aliases: Default::default(),
        graph_endpoint: server.uri(),
        login_endpoint: "https://login.example".into(),
        debug_http: false,
        access_token_override: Some("FAKE".into()),
        refresh_token_seed: None,
    };
    let auth = AuthContext::new(cfg, dir.path().join("tokens.json"));
    let graph = GraphClient::new(auth);

    let v: serde_json::Value = graph.get_json("/me").await.unwrap();
    assert_eq!(v["userPrincipalName"], "alice@contoso.com");
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}
