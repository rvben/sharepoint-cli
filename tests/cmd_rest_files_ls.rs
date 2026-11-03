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
                {"Id": "L1", "Title": "Documents", "BaseTemplate": 101, "RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Shared Documents"}}
            ]
        })))
        .mount(server)
        .await;
}

const LIBRARY_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents";

async fn mount_root_children(server: &MockServer) {
    let folder = "/sites/Marketing/_api/web/GetFolderByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents')";
    Mock::given(method("GET"))
        .and(path(format!("{folder}/Folders")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"UniqueId": "F1", "Name": "Plans", "ServerRelativeUrl": "/sites/Marketing/Shared Documents/Plans",
             "TimeCreated": "2025-01-01T00:00:00Z", "TimeLastModified": "2025-02-01T00:00:00Z"},
            {"UniqueId": "F0", "Name": "Forms", "ServerRelativeUrl": "/sites/Marketing/Shared Documents/Forms"}
        ]})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{folder}/Files")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value": [
            {"UniqueId": "I1", "Name": "a.txt", "ServerRelativeUrl": "/sites/Marketing/Shared Documents/a.txt",
             "Length": "5", "ETag": "\"{I1},1\"", "TimeLastModified": "2025-02-01T00:00:00Z"},
            {"UniqueId": "I2", "Name": "b.txt", "ServerRelativeUrl": "/sites/Marketing/Shared Documents/b.txt",
             "Length": "7"}
        ]})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn ls_lists_folders_and_files_and_hides_forms() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    mount_root_children(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "files", "ls", LIBRARY_REF])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 3);
    assert_eq!(v["items"][0]["kind"], "folder");
    assert_eq!(v["items"][0]["path"], "/Plans");
    assert_eq!(v["items"][1]["path"], "/a.txt");
    assert_eq!(v["items"][1]["size"], 5);
    assert_eq!(v["items"][1]["drive"]["name"], "Documents");
}

#[tokio::test]
async fn ls_limit_then_page_resumes_mid_folder() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    mount_root_children(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let first = sharepoint(&server, dir.path())
        .args([
            "--output",
            "json",
            "files",
            "ls",
            "--limit",
            "2",
            LIBRARY_REF,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(v["total"], 2);
    let token = v["next"].as_str().expect("next token").to_string();
    let second = sharepoint(&server, dir.path())
        .args([
            "--output",
            "json",
            "files",
            "ls",
            "--page",
            &token,
            LIBRARY_REF,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v2: serde_json::Value = serde_json::from_slice(&second).unwrap();
    assert_eq!(v2["total"], 1);
    assert_eq!(v2["items"][0]["name"], "b.txt");
    assert!(v2["next"].is_null());
}
