mod support;

use serde_json::{Value, json};
use support::backend::{BACKENDS, Backend};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn drives_list_can_resume_truncated_collections_on_both_backends() {
    for backend in BACKENDS {
        let server = backend.server().await;
        let route = backend.resolution()[1].0;
        let values: Vec<_> = ["Documents", "Archive", "Plans"]
            .iter()
            .map(|name| match backend {
                Backend::Graph => json!({"id": name, "name": name, "driveType": "documentLibrary"}),
                Backend::Rest => json!({"Id": name, "Title": name, "BaseTemplate": 101,
                "RootFolder": {"ServerRelativeUrl": format!("/sites/Marketing/{name}")}}),
            })
            .collect();
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": values})))
            .with_priority(1)
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let first = backend
            .command(&server.uri(), root.path())
            .args([
                "drives",
                "list",
                "https://contoso.sharepoint.com/sites/Marketing",
                "--limit",
                "2",
                "--fields",
                "name",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let first: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(first["total"], 3);
        assert_eq!(
            first["items"],
            json!([{"name":"Documents"}, {"name":"Archive"}])
        );
        let token = first["next"].as_str().unwrap();
        let last = backend
            .command(&server.uri(), root.path())
            .args([
                "drives",
                "list",
                "https://contoso.sharepoint.com/sites/Marketing",
                "--limit",
                "2",
                "--fields",
                "name",
                "--page",
                token,
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let last: Value = serde_json::from_slice(&last).unwrap();
        assert_eq!(last["items"], json!([{"name":"Plans"}]));
        assert!(last["next"].is_null());
        backend
            .command(&server.uri(), root.path())
            .args([
                "drives",
                "list",
                "https://contoso.sharepoint.com/sites/Marketing",
                "--page",
                "invalid",
            ])
            .assert()
            .code(2);
    }
}

#[test]
fn auth_status_resumes_cached_accounts_and_rejects_other_collection_tokens() {
    use chrono::{Duration, Utc};
    use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};
    let root = tempfile::tempdir().unwrap();
    for oid in ["1", "2", "3"] {
        upsert(
            &root.path().join("sharepoint/tokens.json"),
            &cache_key("contoso", "client-1", oid),
            CacheEntry {
                account: Account {
                    username: format!("user{oid}@example.test"),
                    name: None,
                    tenant_id: "contoso".into(),
                    oid: oid.into(),
                },
                access_token: "FAKE".into(),
                access_token_expires_at: Utc::now() + Duration::hours(1),
                refresh_token: None,
                scopes: vec![],
            },
        )
        .unwrap();
    }
    let first = support::isolated_command(root.path())
        .args([
            "auth",
            "status",
            "--offline",
            "--limit",
            "2",
            "--fields",
            "username",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let first: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(
        first["items"],
        json!([{"username":"user1@example.test"}, {"username":"user2@example.test"}])
    );
    let token = first["next"].as_str().unwrap();
    let last = support::isolated_command(root.path())
        .args([
            "auth",
            "status",
            "--offline",
            "--limit",
            "2",
            "--fields",
            "username",
            "--page",
            token,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let last: Value = serde_json::from_slice(&last).unwrap();
    assert_eq!(last["items"], json!([{"username":"user3@example.test"}]));
    assert!(last["next"].is_null());
    support::isolated_command(root.path())
        .args([
            "--profile",
            "other",
            "auth",
            "status",
            "--offline",
            "--page",
            token,
        ])
        .assert()
        .code(2);
    Backend::Rest
        .command("http://127.0.0.1:1", root.path())
        .args(["auth", "status", "--offline", "--page", token])
        .assert()
        .code(2);
}
