#![cfg(target_os = "linux")]

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn auth_login_errors_when_client_id_is_missing() {
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso.onmicrosoft.com")
        .args(["auth", "login"])
        .assert()
        .code(3)
        .stdout(contains("client_id is required"));
}

#[test]
fn init_errors_when_client_id_prompt_is_blank() {
    // Feed: tenant = "contoso", client_id = "" (blank). Init should bail before
    // touching the network or saving the config.
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["init"])
        .write_stdin("contoso\n\n")
        .assert()
        .failure()
        .stdout(contains("client_id is required"));
}
