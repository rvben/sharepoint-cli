#![cfg(target_os = "linux")]

use assert_cmd::Command;
use wiremock::matchers::{header, method, path, query_param};
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
async fn find_searches_within_library() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param("startrow", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "PrimaryQueryResult": {"RelevantResults": {"TotalRows": 1, "RowCount": 1, "Table": {"Rows": [
                {"Cells": [
                    {"Key": "FileName", "Value": "budget.xlsx"},
                    {"Key": "Path", "Value": "https://contoso.sharepoint.com/sites/Marketing/Shared Documents/Fin/budget.xlsx"},
                    {"Key": "Size", "Value": "2048"},
                    {"Key": "UniqueId", "Value": "{B1}"}
                ]}
            ]}}}
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args([
            "--output",
            "json",
            "files",
            "find",
            "https://contoso.sharepoint.com/sites/Marketing/Documents",
            "--query",
            "budget",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["name"], "budget.xlsx");
    assert_eq!(v["items"][0]["path"], "/Fin/budget.xlsx");
    assert_eq!(v["items"][0]["size"], 2048);
    assert_eq!(v["items"][0]["id"], "B1");
}

const LIBRARY_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents";
const LIBRARY_URL: &str = "https://contoso.sharepoint.com/sites/Marketing/Shared Documents";

fn search_body(total: u64, rows: &[(&str, &str)]) -> serde_json::Value {
    let rows: Vec<_> = rows
        .iter()
        .map(|(name, path)| {
            serde_json::json!({"Cells": [
                {"Key": "FileName", "Value": name},
                {"Key": "Path", "Value": path}
            ]})
        })
        .collect();
    serde_json::json!({
        "PrimaryQueryResult": {"RelevantResults": {
            "TotalRows": total, "RowCount": rows.len(), "Table": {"Rows": rows}
        }}
    })
}

fn find(server: &MockServer, cache: &std::path::Path, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--output", "json", "files", "find", LIBRARY_REF];
    args.extend_from_slice(extra);
    let out = sharepoint(server, cache)
        .args(&args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[tokio::test]
async fn find_parenthesises_query_so_or_stays_in_library() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    let expected = format!("'(a OR b) path:\"{LIBRARY_URL}\" IsDocument:1'");
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param("querytext", expected.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
            1,
            &[("a.txt", &format!("{LIBRARY_URL}/a.txt"))],
        )))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let v = find(&server, dir.path(), &["--query", "a OR b"]);
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["path"], "/a.txt");
}

#[tokio::test]
async fn find_escapes_single_quotes_in_query() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    let expected = format!("'(O''Brien) path:\"{LIBRARY_URL}\" IsDocument:1'");
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param("querytext", expected.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
            1,
            &[("obrien.docx", &format!("{LIBRARY_URL}/obrien.docx"))],
        )))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let v = find(&server, dir.path(), &["--query", "O'Brien"]);
    assert_eq!(v["items"][0]["name"], "obrien.docx");
}

#[tokio::test]
async fn find_drops_rows_outside_the_library() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
            3,
            &[
                ("in.txt", &format!("{LIBRARY_URL}/in.txt")),
                (
                    "other.txt",
                    "https://contoso.sharepoint.com/sites/Marketing/Shared Documents2/other.txt",
                ),
                (
                    "case.txt",
                    "https://contoso.sharepoint.com/sites/marketing/shared documents/Sub/case.txt",
                ),
            ],
        )))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let v = find(&server, dir.path(), &["--query", "txt", "--all"]);
    let names: Vec<_> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["in.txt", "case.txt"]);
    assert_eq!(v["items"][1]["path"], "/Sub/case.txt");
}

#[tokio::test]
async fn find_pages_with_startrow() {
    let server = MockServer::start().await;
    mount_web(&server).await;
    mount_lists(&server).await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param("startrow", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
            2,
            &[("one.txt", &format!("{LIBRARY_URL}/one.txt"))],
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param("startrow", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(
            2,
            &[("two.txt", &format!("{LIBRARY_URL}/two.txt"))],
        )))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let first = find(&server, dir.path(), &["--query", "txt"]);
    assert_eq!(first["items"][0]["name"], "one.txt");
    let token = first["next"].as_str().expect("next token").to_string();
    let second = find(&server, dir.path(), &["--query", "txt", "--page", &token]);
    assert_eq!(second["total"], 1);
    assert_eq!(second["items"][0]["name"], "two.txt");
    assert!(second["next"].is_null());
}
