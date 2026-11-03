#![cfg(target_os = "linux")]

use assert_cmd::Command;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COOKIE: &str = "FedAuth=F; rtFa=R";

fn sharepoint(server: &MockServer, cache: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("sharepoint").unwrap();
    cmd.env("XDG_CACHE_HOME", cache)
        .env("XDG_CONFIG_HOME", cache)
        .env("SHAREPOINT_BACKEND", "rest")
        .env("SHAREPOINT_HOST", "contoso.sharepoint.com")
        .env("SHAREPOINT_REST_ENDPOINT", server.uri())
        .env("SHAREPOINT_COOKIE", COOKIE)
        .env_remove("SHAREPOINT_TENANT_ID")
        .env_remove("SHAREPOINT_CLIENT_ID");
    cmd
}

async fn mount_web(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/sites/Marketing/_api/web"))
        .and(header("cookie", COOKIE))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Id": "W1", "Title": "Marketing", "Url": "https://contoso.sharepoint.com/sites/Marketing"
        })))
        .mount(server)
        .await;
}

async fn mount_lists(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/sites/Marketing/_api/web/lists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"Id": "L1", "Title": "Documents", "RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Shared Documents"}}
            ]
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn drives_list_returns_document_libraries() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args([
            "--output",
            "json",
            "drives",
            "list",
            "https://contoso.sharepoint.com/sites/Marketing",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["id"], "L1");
    assert_eq!(v["items"][0]["name"], "Documents");
    assert_eq!(v["items"][0]["site"]["id"], "W1");
}
