//! Verifies AuthContext refreshes a near-expired token and persists the
//! rotated refresh token atomically.

use chrono::{Duration, Utc};
use sharepoint_cli::auth::AuthContext;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use sharepoint_cli::config::ResolvedConfig;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn access_token_refreshes_when_within_60s_of_expiry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/contoso/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "AT-NEW",
            "refresh_token": "RT-NEW",
            "expires_in": 3600,
            "scope": "User.Read Files.ReadWrite.All"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("tokens.json");

    let key = cache_key("contoso", "client-1", "OID-1");
    upsert(
        &cache_path,
        &key,
        CacheEntry {
            account: Account {
                username: "alice@contoso.com".into(),
                name: Some("Alice".to_string()),
                tenant_id: "contoso".into(),
                oid: "OID-1".into(),
            },
            access_token: "AT-OLD".into(),
            access_token_expires_at: Utc::now() + Duration::seconds(30),
            refresh_token: Some("RT-OLD".to_string()),
            scopes: vec!["User.Read".into()],
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
    let ctx = AuthContext::new(cfg, cache_path.clone());
    let tok = ctx.access_token().await.unwrap();
    assert_eq!(tok, "AT-NEW");

    let cache = sharepoint_cli::auth::token_cache::load(&cache_path).unwrap();
    let e = cache.entries.get(&key).unwrap();
    assert_eq!(e.refresh_token.as_deref(), Some("RT-NEW"));
}
