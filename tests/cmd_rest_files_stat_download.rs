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

const FILE_API: &str = "/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/Plans/q1.txt')";
const FILE_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents/Plans/q1.txt";

#[tokio::test]
async fn stat_returns_canonical_file() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path(FILE_API))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "UniqueId": "I9", "Name": "q1.txt",
            "ServerRelativeUrl": "/sites/Marketing/Shared Documents/Plans/q1.txt", "Length": "11"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "files", "stat", FILE_REF])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["id"], "I9");
    assert_eq!(v["path"], "/Plans/q1.txt");
    assert_eq!(v["size"], 11);
    assert!(v.get("download_url").is_none());
}

#[tokio::test]
async fn download_to_stdout_streams_value() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("{FILE_API}/$value")))
        .and(header("cookie", COOKIE))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    sharepoint(&server, dir.path())
        .args(["files", "download", FILE_REF, "--path", "-"])
        .assert()
        .success()
        .stdout("hello world");
}

#[tokio::test]
async fn stat_falls_back_to_folder_when_file_is_missing() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path(
            "/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/Plans')",
        ))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": "-2130575338", "message": {"value": "File Not Found."}}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/sites/Marketing/_api/web/GetFolderByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/Plans')",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "UniqueId": "F1", "Name": "Plans",
            "ServerRelativeUrl": "/sites/Marketing/Shared Documents/Plans"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args([
            "--output",
            "json",
            "files",
            "stat",
            "https://contoso.sharepoint.com/sites/Marketing/Documents/Plans",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["id"], "F1");
    assert_eq!(v["kind"], "folder");
    assert_eq!(v["path"], "/Plans");
}
