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

#[tokio::test]
async fn sites_list_uses_search_and_emits_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(header("cookie", COOKIE))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "PrimaryQueryResult": {"RelevantResults": {"TotalRows": 1, "RowCount": 1, "Table": {"Rows": [
                {"Cells": [
                    {"Key": "Title", "Value": "Marketing"},
                    {"Key": "Path", "Value": "https://contoso.sharepoint.com/sites/Marketing"},
                    {"Key": "WebId", "Value": "W1"}
                ]}
            ]}}}
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "sites", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["source"], "search");
    assert_eq!(v["items"][0]["name"], "Marketing");
    assert_eq!(
        v["items"][0]["url"],
        "https://contoso.sharepoint.com/sites/Marketing"
    );
    assert!(v["next"].is_null());
}

#[tokio::test]
async fn missing_session_is_auth_exit_3() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    sharepoint(&server, dir.path())
        .env_remove("SHAREPOINT_COOKIE")
        .args(["--output", "json", "sites", "list"])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("sharepoint auth login"));
}

fn site_rows(total: u64, title: &str) -> serde_json::Value {
    serde_json::json!({
        "PrimaryQueryResult": {"RelevantResults": {"TotalRows": total, "RowCount": 1, "Table": {"Rows": [
            {"Cells": [
                {"Key": "Title", "Value": title},
                {"Key": "Path", "Value": format!("https://contoso.sharepoint.com/sites/{title}")},
                {"Key": "WebId", "Value": title}
            ]}
        ]}}}
    })
}

#[tokio::test]
async fn sites_list_pages_with_startrow_and_parenthesised_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param(
            "querytext",
            "'(mark OR sales) contentclass:STS_Site'",
        ))
        .and(query_param("startrow", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(site_rows(2, "Marketing")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/_api/search/query"))
        .and(query_param(
            "querytext",
            "'(mark OR sales) contentclass:STS_Site'",
        ))
        .and(query_param("startrow", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(site_rows(2, "Sales")))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec![
            "--output",
            "json",
            "sites",
            "list",
            "--query",
            "mark OR sales",
        ];
        args.extend_from_slice(extra);
        let out = sharepoint(&server, dir.path())
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<serde_json::Value>(&out).unwrap()
    };
    let first = run(&[]);
    assert_eq!(first["items"][0]["name"], "Marketing");
    let token = first["next"].as_str().expect("next token").to_string();
    let second = run(&["--page", &token]);
    assert_eq!(second["items"][0]["name"], "Sales");
    assert!(second["next"].is_null());
}
