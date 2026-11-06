mod support;

use chrono::{Duration, Utc};
use predicates::str::contains;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn sites_list_returns_followed_sites() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/me/followedSites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": "site-1",
                    "displayName": "Marketing",
                    "webUrl": "https://contoso.sharepoint.com/sites/Marketing",
                    "name": "Marketing"
                }
            ]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("tokens.json");

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
            access_token: "FAKE-TOKEN".into(),
            access_token_expires_at: Utc::now() + Duration::minutes(30),
            refresh_token: Some("RT".to_string()),
            scopes: vec!["Sites.Read.All".into()],
        },
    )
    .unwrap();

    support::isolated_command(dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "FAKE-TOKEN")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
        .args(["--output", "json", "sites", "list"])
        .assert()
        .success()
        .stdout(contains("Marketing"))
        .stdout(contains("\"source\": \"followed\""));
}

#[tokio::test]
async fn sites_list_total_is_null_while_more_sites_remain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/me/followedSites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "s1", "displayName": "Marketing", "webUrl": "https://contoso.sharepoint.com/sites/Marketing"},
                {"id": "s2", "displayName": "Sales", "webUrl": "https://contoso.sharepoint.com/sites/Sales"}
            ]
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let list = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["--output", "json", "sites", "list"];
        args.extend_from_slice(extra);
        let out = support::isolated_command(dir.path())
            .env("SHAREPOINT_TENANT_ID", "contoso")
            .env("SHAREPOINT_CLIENT_ID", "client-1")
            .env("SHAREPOINT_ACCESS_TOKEN", "FAKE-TOKEN")
            .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).unwrap()
    };

    let partial = list(&["--limit", "1"]);
    assert_eq!(partial["items"].as_array().unwrap().len(), 1);
    assert!(partial["next"].is_string());
    assert!(partial["total"].is_null(), "{partial}");

    let complete = list(&[]);
    assert!(complete["next"].is_null());
    assert_eq!(complete["total"], 2);
}
