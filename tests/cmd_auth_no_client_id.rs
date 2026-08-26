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
        .stderr(contains("client_id is required"));
}

#[test]
fn headless_init_errors_when_client_id_is_missing() {
    // Headless init requires explicit values and must bail before touching the
    // network or saving config when the client ID is absent.
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["--tenant", "contoso", "init"])
        .assert()
        .failure()
        .stderr(contains("--client-id"));
}
