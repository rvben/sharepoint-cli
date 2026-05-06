use assert_cmd::Command;
use chrono::{Duration, Utc};
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
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

/// Mock site + drive resolution (shared across tests).
async fn mount_site_and_drive(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/sites/contoso.sharepoint.com:/sites/Marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "S1",
            "displayName": "Marketing",
            "webUrl": "https://contoso.sharepoint.com/sites/Marketing",
            "name": "Marketing"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sites/S1/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{"id": "D1", "name": "Documents", "driveType": "documentLibrary", "webUrl": ""}]
        })))
        .mount(server)
        .await;
}

/// Drive children — first page (3 items) with a nextLink.
async fn mount_page1(server: &MockServer) -> String {
    let next_link = format!("{}/drives/D1/root/children?$skiptoken=PAGE2", server.uri());

    Mock::given(method("GET"))
        .and(path("/drives/D1/root/children"))
        .and(query_param_is_missing("$skiptoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "@odata.nextLink": next_link,
            "value": [
                {"id": "f1", "name": "file1.txt", "size": 100, "file": {}},
                {"id": "f2", "name": "file2.txt", "size": 200, "file": {}},
                {"id": "f3", "name": "file3.txt", "size": 300, "file": {}},
            ]
        })))
        .mount(server)
        .await;

    next_link
}

/// Drive children — second page (3 items, no nextLink).
/// Uses a `$skiptoken` query param matcher so it only fires for continuation
/// requests and doesn't shadow the first-page mock.
async fn mount_page2(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/drives/D1/root/children"))
        .and(query_param("$skiptoken", "PAGE2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": "f4", "name": "file4.txt", "size": 400, "file": {}},
                {"id": "f5", "name": "file5.txt", "size": 500, "file": {}},
                {"id": "f6", "name": "file6.txt", "size": 600, "file": {}},
            ]
        })))
        .mount(server)
        .await;
}

fn cli(server: &MockServer, dir: &tempfile::TempDir) -> Command {
    let mut c = Command::cargo_bin("sharepoint").unwrap();
    c.env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .env("SHAREPOINT_ACCESS_TOKEN", "AT")
        .env("MICROSOFT_GRAPH_ENDPOINT", server.uri());
    c
}

const SITE_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents";

/// With `--limit 2` and 3 items on the first Graph page: the response must
/// contain exactly 2 items and a non-null `next` token that encodes position
/// within the current page (mid-page cursor, skip=2).
#[tokio::test]
async fn files_ls_limit_mid_page_emits_cursor() {
    let server = MockServer::start().await;
    mount_site_and_drive(&server).await;
    mount_page1(&server).await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

    let out = cli(&server, &dir)
        .args(["--json", "files", "ls", "--limit", "2", SITE_REF])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 2, "expected 2 items, got {}", v["total"]);
    assert!(
        v["next"].is_string(),
        "expected non-null next token, got {:?}",
        v["next"]
    );
    // Verify the two items returned are from the first page.
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0]["name"], "file1.txt");
    assert_eq!(items[1]["name"], "file2.txt");
}

/// Replaying with the mid-page cursor from the previous test must yield the
/// remaining item from page 1 (file3) plus all items from page 2, up to limit.
#[tokio::test]
async fn files_ls_limit_mid_page_resumes_correctly() {
    let server = MockServer::start().await;
    mount_site_and_drive(&server).await;
    let next_link = mount_page1(&server).await;
    mount_page2(&server).await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

    // First request: --limit 2 → get file1 + file2, cursor points at page1 skip=2.
    let first_out = cli(&server, &dir)
        .args(["--json", "files", "ls", "--limit", "2", SITE_REF])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let first_v: serde_json::Value = serde_json::from_slice(&first_out).unwrap();
    let token = first_v["next"]
        .as_str()
        .expect("first response must have a next token");

    // The page1 mock stays active (wiremock does not consume mocks). The second
    // call re-fetches page1 (cursor points back to the same URL, skip=2), skips
    // file1 + file2, then follows the nextLink to page2.
    let _ = next_link;

    // Second request: --limit 4 starting from the mid-page cursor.
    // Should yield file3 (remaining from page 1) + file4/file5/file6 (page 2) → 4 items.
    let second_out = cli(&server, &dir)
        .args([
            "--json", "files", "ls", "--limit", "4", "--page", token, SITE_REF,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let second_v: serde_json::Value = serde_json::from_slice(&second_out).unwrap();
    let items = second_v["items"].as_array().expect("items array");
    // file3 from page1 (after skipping 2) + file4/file5/file6 from page2 = 4 items
    assert_eq!(
        items.len(),
        4,
        "expected 4 items on resume, got {}",
        items.len()
    );
    assert_eq!(
        items[0]["name"], "file3.txt",
        "first resumed item must be file3"
    );
    assert_eq!(items[1]["name"], "file4.txt");
    assert_eq!(items[2]["name"], "file5.txt");
    assert_eq!(items[3]["name"], "file6.txt");
}

/// With `--all`, the listing must return all 6 items across both pages.
#[tokio::test]
async fn files_ls_all_follows_next_link() {
    let server = MockServer::start().await;
    mount_site_and_drive(&server).await;
    mount_page1(&server).await;
    mount_page2(&server).await;

    let dir = tempfile::tempdir().unwrap();
    seed_cache(dir.path());

    let out = cli(&server, &dir)
        .args(["--json", "files", "ls", "--all", SITE_REF])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let items = v["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        6,
        "expected 6 items with --all, got {}",
        items.len()
    );
    assert!(
        v["next"].is_null(),
        "next must be null when all items fetched"
    );
}
