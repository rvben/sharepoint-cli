#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::str::contains;
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

async fn baseline_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/sites/contoso.sharepoint.com:/sites/Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "S1", "displayName": "Marketing",
            "webUrl": "https://contoso.sharepoint.com/sites/Marketing"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "D1", "name": "Documents", "driveType": "documentLibrary"}]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn files_stat_includes_download_url() {
    let server = MockServer::start().await;
    baseline_mocks(&server).await;
    // wiremock's path() matcher ignores the $select query string the binary sends.
    Mock::given(method("GET"))
        .and(path("/drives/D1/root:/Q4-plan.pptx"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "I1", "name": "Q4-plan.pptx", "size": 100,
            "lastModifiedDateTime": "2025-11-04T16:22:01Z",
            "parentReference": {"driveId": "D1", "path": "/drives/D1/root:"},
            "file": {"hashes": {"quickXorHash": "QX"}},
            "@microsoft.graph.downloadUrl": "https://short.example/download?token=abc"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

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
            "stat",
            "https://contoso.sharepoint.com/sites/Marketing/Documents/Q4-plan.pptx",
        ])
        .assert()
        .success()
        .stdout(contains("\"download_url\""))
        .stdout(contains("short.example/download"))
        .stdout(contains("Q4-plan.pptx"));
}

#[tokio::test]
async fn files_download_writes_stdout_with_dash() {
    let server = MockServer::start().await;
    baseline_mocks(&server).await;
    Mock::given(method("GET"))
        .and(path("/drives/D1/root:/hello.txt:/content"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "FAKE")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri())
        .args([
            "--quiet",
            "files",
            "download",
            "https://contoso.sharepoint.com/sites/Marketing/Documents/hello.txt",
            "--output",
            "-",
        ])
        .assert()
        .success()
        .stdout(predicates::ord::eq(b"hello world".as_ref()));
}
