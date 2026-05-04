#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::str::contains;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn files_ls_returns_items_at_drive_root() {
    let server = MockServer::start().await;

    // Site lookup
    Mock::given(method("GET"))
        .and(path("/sites/contoso.sharepoint.com:/sites/Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "S1",
            "displayName": "Marketing",
            "webUrl": "https://contoso.sharepoint.com/sites/Marketing",
            "name": "Marketing"
        })))
        .mount(&server)
        .await;

    // Drive lookup
    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "D1", "name": "Documents", "driveType": "documentLibrary", "webUrl": ""}
            ]
        })))
        .mount(&server)
        .await;

    // Root children
    Mock::given(method("GET"))
        .and(path("/drives/D1/root/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": "F1",
                    "name": "Q4-plan.pptx",
                    "size": 204800,
                    "lastModifiedDateTime": "2025-10-01T12:00:00Z",
                    "file": {}
                },
                {
                    "id": "F2",
                    "name": "Archive",
                    "size": 0,
                    "lastModifiedDateTime": "2025-09-01T08:00:00Z",
                    "folder": {"childCount": 3}
                }
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
            "files",
            "ls",
            "https://contoso.sharepoint.com/sites/Marketing/Documents",
        ])
        .assert()
        .success()
        .stdout(contains("Q4-plan.pptx"))
        .stdout(contains("Archive"))
        .stdout(contains("\"kind\": \"folder\""));
}
