#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn seed_cache(dir: &std::path::Path) {
    let cache_dir = dir.join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    upsert(
        &cache_dir.join("tokens.json"),
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
}

#[tokio::test]
async fn drives_list_all_follows_next_link() {
    let server = MockServer::start().await;

    // Site resolution
    Mock::given(method("GET"))
        .and(path("/sites/contoso.sharepoint.com:/sites/Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "S1",
            "displayName": "Marketing",
            "webUrl": "https://contoso.sharepoint.com/sites/Marketing"
        })))
        .mount(&server)
        .await;

    let next_link = format!("{}/sites/S1/drives?$skiptoken=PAGE2", server.uri());

    // First page: 3 drives + nextLink
    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "@odata.nextLink": next_link,
            "value": [
                {"id": "d1", "name": "D1", "driveType": "documentLibrary", "webUrl": "https://x"},
                {"id": "d2", "name": "D2", "driveType": "documentLibrary", "webUrl": "https://x"},
                {"id": "d3", "name": "D3", "driveType": "documentLibrary", "webUrl": "https://x"},
            ]
        })))
        .mount(&server)
        .await;

    // Second page: 3 more drives, no nextLink
    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "d4", "name": "D4", "driveType": "documentLibrary", "webUrl": "https://x"},
                {"id": "d5", "name": "D5", "driveType": "documentLibrary", "webUrl": "https://x"},
                {"id": "d6", "name": "D6", "driveType": "documentLibrary", "webUrl": "https://x"},
            ]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

    let out = Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "AT")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
        .args([
            "--json",
            "drives",
            "list",
            "--all",
            "https://contoso.sharepoint.com/sites/Marketing",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = v["items"]
        .as_array()
        .expect("drives list should have items");
    assert_eq!(
        arr.len(),
        6,
        "expected 6 drives across 2 pages, got {}",
        arr.len()
    );
}
