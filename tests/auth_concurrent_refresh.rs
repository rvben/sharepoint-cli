//! Verifies that concurrent access_token() calls with an expired cache
//! result in exactly one network refresh, not N races.

use std::sync::Arc;

use chrono::{Duration, Utc};
use sharepoint_cli::auth::AuthContext;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use sharepoint_cli::config::ResolvedConfig;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn concurrent_access_token_calls_trigger_exactly_one_refresh() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "AT-FRESH",
            "refresh_token": "RT-NEW",
            "expires_in": 3600,
            "scope": "User.Read"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("tokens.json");

    // Seed an already-expired token so all callers will attempt to refresh.
    let key = cache_key("contoso", "client-1", "OID-1");
    upsert(
        &cache_path,
        &key,
        CacheEntry {
            account: Account {
                username: "alice@contoso.com".into(),
                name: None,
                tenant_id: "contoso".into(),
                oid: "OID-1".into(),
            },
            access_token: "AT-OLD".into(),
            // Expired well before the refresh margin.
            access_token_expires_at: Utc::now() - Duration::seconds(120),
            refresh_token: Some("RT-OLD".to_string()),
            scopes: vec![],
        },
    )
    .unwrap();

    let cfg = ResolvedConfig {
        profile_name: "default".into(),
        tenant_id: Some("contoso".into()),
        client_id: Some("client-1".into()),
        default_site: None,
        read_only: false,
        site_aliases: Default::default(),
        graph_endpoint: "https://graph.example".into(),
        login_endpoint: server.uri(),
        debug_http: false,
        access_token_override: None,
        refresh_token_seed: None,
    };
    let ctx = Arc::new(AuthContext::new(cfg, cache_path));

    // Spawn 10 concurrent tasks all calling access_token() on the same context.
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let ctx = ctx.clone();
            tokio::spawn(async move { ctx.access_token().await.unwrap() })
        })
        .collect();

    let results: Vec<String> = futures_util::future::join_all(tasks)
        .await
        .into_iter()
        .map(|r| r.expect("task panicked"))
        .collect();

    // All callers must get the fresh token.
    for tok in &results {
        assert_eq!(
            tok, "AT-FRESH",
            "all callers should receive the refreshed token"
        );
    }

    // The mock server must have received exactly one POST (single-flight refresh).
    let received = server.received_requests().await.unwrap();
    assert_eq!(
        received.len(),
        1,
        "expected exactly 1 refresh request; got {}: concurrent callers should coalesce",
        received.len()
    );
}
