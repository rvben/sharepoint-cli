mod support;

use assert_cmd::Command;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COOKIE: &str = "FedAuth=F; rtFa=R";

fn sharepoint(server: &MockServer, cache: &std::path::Path) -> Command {
    let mut cmd = support::isolated_command(cache);
    cmd.env("SHAREPOINT_BACKEND", "rest")
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
        .and(query_param(
            "$filter",
            "(BaseTemplate eq 101 or BaseTemplate eq 700) and Hidden eq false",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"Id": "L1", "Title": "Documents", "BaseTemplate": 101, "RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Shared Documents"}},
                {"Id": "L2", "Title": "OneDrive Documents", "BaseTemplate": 700, "RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Documents"}}
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
    assert_eq!(v["total"], 2);
    assert_eq!(v["items"][0]["id"], "L1");
    assert_eq!(v["items"][0]["name"], "Documents");
    assert_eq!(v["items"][0]["drive_type"], "documentLibrary");
    assert_eq!(v["items"][0]["site"]["id"], "W1");
    assert_eq!(v["items"][1]["id"], "L2");
    assert_eq!(v["items"][1]["drive_type"], "business");
}
