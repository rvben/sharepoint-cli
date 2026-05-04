#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::prelude::PredicateBooleanExt;
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
async fn files_find_returns_matches_with_query() {
    let server = MockServer::start().await;
    baseline_mocks(&server).await;

    // Graph search endpoint: /drives/{id}/root/search(q='{query}')
    Mock::given(method("GET"))
        .and(path("/drives/D1/root/search(q='plan')"))
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
                    "name": "project-plan.docx",
                    "size": 51200,
                    "lastModifiedDateTime": "2025-09-15T09:00:00Z",
                    "file": {}
                }
            ]
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
            "find",
            "https://contoso.sharepoint.com/sites/Marketing/Documents",
            "--query",
            "plan",
        ])
        .assert()
        .success()
        .stdout(contains("Q4-plan.pptx"))
        .stdout(contains("project-plan.docx"));
}

#[tokio::test]
async fn files_find_filters_by_glob() {
    let server = MockServer::start().await;
    baseline_mocks(&server).await;

    // When only --name is provided (no --query), the binary defaults to q='*'.
    Mock::given(method("GET"))
        .and(path("/drives/D1/root/search(q='*')"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": "F1",
                    "name": "report.pdf",
                    "size": 102400,
                    "lastModifiedDateTime": "2025-10-01T12:00:00Z",
                    "file": {}
                },
                {
                    "id": "F2",
                    "name": "notes.txt",
                    "size": 1024,
                    "lastModifiedDateTime": "2025-09-15T09:00:00Z",
                    "file": {}
                },
                {
                    "id": "F3",
                    "name": "slides.pptx",
                    "size": 307200,
                    "lastModifiedDateTime": "2025-08-20T14:00:00Z",
                    "file": {}
                }
            ]
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
            "find",
            "https://contoso.sharepoint.com/sites/Marketing/Documents",
            "--name",
            "*.pdf",
        ])
        .assert()
        .success()
        .stdout(contains("report.pdf"))
        .stdout(predicates::str::contains("notes.txt").not())
        .stdout(predicates::str::contains("slides.pptx").not());
}
