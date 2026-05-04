#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::str::contains;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn drives_list_returns_libraries_for_site_url() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/sites/contoso.sharepoint.com:/sites/Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "S1",
            "displayName": "Marketing",
            "webUrl": "https://contoso.sharepoint.com/sites/Marketing"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "D1", "name": "Documents", "driveType": "documentLibrary"},
                {"id": "D2", "name": "Reports",   "driveType": "documentLibrary"}
            ]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("tokens.json");
    upsert(
        &cache_path,
        &cache_key("contoso", "client-1", "OID"),
        CacheEntry {
            account: Account {
                username: "u".into(),
                name: Some("n".to_string()),
                tenant_id: "contoso".into(),
                oid: "OID".into(),
            },
            access_token: "AT".into(),
            access_token_expires_at: Utc::now() + Duration::minutes(30),
            refresh_token: Some("RT".to_string()),
            scopes: vec![],
        },
    )
    .unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "FAKE")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
        .args([
            "--json",
            "drives",
            "list",
            "https://contoso.sharepoint.com/sites/Marketing",
        ])
        .assert()
        .success()
        .stdout(contains("Documents"))
        .stdout(contains("Reports"));
}

#[tokio::test]
async fn drives_list_accepts_spo_uri() {
    let server = MockServer::start().await;

    // spo://Marketing → SiteRef::Name("Marketing") → GET /sites?search=Marketing
    Mock::given(method("GET"))
        .and(path("/sites"))
        .and(query_param("search", "Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": "S2",
                "displayName": "Marketing",
                "webUrl": "https://contoso.sharepoint.com/sites/Marketing"
            }]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sites/S2/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "D3", "name": "Shared Documents", "driveType": "documentLibrary"}
            ]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("tokens.json");
    upsert(
        &cache_path,
        &cache_key("contoso", "client-1", "OID"),
        CacheEntry {
            account: Account {
                username: "u".into(),
                name: Some("n".to_string()),
                tenant_id: "contoso".into(),
                oid: "OID".into(),
            },
            access_token: "AT".into(),
            access_token_expires_at: Utc::now() + Duration::minutes(30),
            refresh_token: Some("RT".to_string()),
            scopes: vec![],
        },
    )
    .unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "FAKE")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
        .args(["--json", "drives", "list", "spo://Marketing"])
        .assert()
        .success()
        .stdout(contains("Shared Documents"));
}
