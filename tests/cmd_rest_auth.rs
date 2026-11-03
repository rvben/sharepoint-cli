mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "contoso.sharepoint.com";

fn sharepoint(server: &MockServer, cache: &std::path::Path) -> Command {
    let mut cmd = support::isolated_command(cache);
    cmd.env("SHAREPOINT_BACKEND", "rest")
        .env("SHAREPOINT_HOST", HOST)
        .env("SHAREPOINT_REST_ENDPOINT", server.uri())
        .env_remove("SHAREPOINT_COOKIE")
        .env_remove("SHAREPOINT_BROWSER")
        .env_remove("SHAREPOINT_TENANT_ID")
        .env_remove("SHAREPOINT_CLIENT_ID");
    cmd
}

fn session_path(cache: &std::path::Path) -> std::path::PathBuf {
    cache.join("sharepoint").join("sessions.json")
}

fn seed_session(cache: &std::path::Path) {
    sharepoint_cli::auth::session_store::upsert(
        &session_path(cache),
        HOST,
        sharepoint_cli::auth::session_store::Session {
            cookie: "FedAuth=F; rtFa=R".into(),
            captured_at: chrono::Utc::now(),
            username: Some("ada@contoso.com".into()),
        },
    )
    .unwrap();
}

#[tokio::test]
async fn status_verifies_stored_session() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/web/currentuser"))
        .and(header("cookie", "FedAuth=F; rtFa=R"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Title": "Ada",
            "Email": "ada@contoso.com",
            "LoginName": "i:0#.f|membership|ada@contoso.com"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    seed_session(dir.path());
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["backend"], "rest");
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["host"], HOST);
    assert_eq!(v["items"][0]["username"], "ada@contoso.com");
    assert_eq!(v["verified"], true);
    assert_eq!(v["identity"]["email"], "ada@contoso.com");
    assert!(
        !String::from_utf8_lossy(&out).contains("FedAuth=F"),
        "cookie must not be printed"
    );
}

#[tokio::test]
async fn status_with_rejected_session_is_auth_exit_3() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/web/currentuser"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "https://login.example/"),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    seed_session(dir.path());
    sharepoint(&server, dir.path())
        .args(["--output", "json", "auth", "status"])
        .assert()
        .code(3)
        .stderr(predicates::str::contains("sharepoint auth login"));
}

#[tokio::test]
async fn status_offline_without_session_reports_zero() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "auth", "status", "--offline"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 0);
    assert_eq!(v["verified"], false);
    assert!(v["identity"].is_null());
}

#[tokio::test]
async fn status_verifies_cookie_from_env_without_stored_session() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/web/currentuser"))
        .and(header("cookie", "FedAuth=E; rtFa=E"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Title": "Ada", "Email": "ada@contoso.com", "LoginName": "ada"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .env("SHAREPOINT_COOKIE", "FedAuth=E; rtFa=E")
        .args(["--output", "json", "auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["total"], 0);
    assert_eq!(v["verified"], true);
}

#[tokio::test]
async fn status_text_reports_env_cookie_and_identity() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/web/currentuser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Title": "Ada", "Email": "ada@contoso.com", "LoginName": "ada"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    sharepoint(&server, dir.path())
        .env("SHAREPOINT_COOKIE", "FedAuth=E; rtFa=E")
        .args(["--output", "text", "auth", "status"])
        .assert()
        .success()
        .stderr(predicates::str::contains("SHAREPOINT_COOKIE"))
        .stderr(predicates::str::contains("Verified as ada@contoso.com"))
        .stderr(predicates::str::contains("No browser session").not());
}

#[tokio::test]
async fn logout_removes_session() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    seed_session(dir.path());
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "auth", "logout"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["removed"], 1);
    assert_eq!(v["backend"], "rest");
    assert_eq!(v["host"], HOST);
    assert!(
        sharepoint_cli::auth::session_store::get(&session_path(dir.path()), HOST)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn login_with_missing_browser_is_input_error() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    sharepoint(&server, dir.path())
        .env("SHAREPOINT_BROWSER", dir.path().join("missing-browser"))
        .args(["--output", "json", "auth", "login"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("launch"))
        .stderr(predicates::str::contains("missing-browser"));
}

#[tokio::test]
async fn doctor_checks_rest_session() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_api/web/currentuser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Title": "Ada", "Email": "ada@contoso.com", "LoginName": "x"
        })))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    seed_session(dir.path());
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let checks = v["checks"].as_array().unwrap();
    let rest = checks
        .iter()
        .find(|c| c["name"] == "sharepoint_rest")
        .unwrap();
    assert_eq!(rest["status"], "pass");
    assert_eq!(rest["detail"], "ada@contoso.com");
    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    for graph_only in ["profile", "credential_cache", "microsoft_graph"] {
        assert!(!names.contains(&graph_only), "{graph_only} in {names:?}");
    }
    assert!(
        !String::from_utf8_lossy(&out).contains("FedAuth=F"),
        "cookie must not be printed"
    );
}

#[tokio::test]
async fn doctor_offline_without_session_fails_browser_session() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "doctor", "--offline"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["healthy"], false);
    let checks = v["checks"].as_array().unwrap();
    let find = |name: &str| checks.iter().find(|c| c["name"] == name).unwrap();
    assert_eq!(find("sharepoint_host")["status"], "pass");
    assert_eq!(find("browser_session")["status"], "fail");
    assert_eq!(find("sharepoint_rest")["status"], "skip");
}

#[tokio::test]
async fn doctor_reports_corrupt_session_store() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let path = session_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not valid json").unwrap();
    let out = sharepoint(&server, dir.path())
        .args(["--output", "json", "doctor", "--offline"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["healthy"], false);
    let checks = v["checks"].as_array().unwrap();
    let find = |name: &str| checks.iter().find(|c| c["name"] == name).unwrap();
    let browser_session = find("browser_session");
    assert_eq!(browser_session["status"], "fail");
    assert!(
        browser_session["detail"]
            .as_str()
            .unwrap()
            .contains("sessions.json is corrupt"),
        "{browser_session:?}"
    );
}

#[tokio::test]
async fn config_show_reports_rest_settings_without_cookie() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let out = sharepoint(&server, dir.path())
        .env("SHAREPOINT_COOKIE", "FedAuth=E; rtFa=E")
        .args(["--output", "json", "config", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["backend"], "rest");
    assert_eq!(v["sharepoint_host"], HOST);
    assert_eq!(v["rest_endpoint"], server.uri());
    assert!(
        v["session_path"]
            .as_str()
            .unwrap()
            .ends_with("sessions.json")
    );
    assert!(
        !String::from_utf8_lossy(&out).contains("FedAuth=E"),
        "cookie must not be printed"
    );
}
