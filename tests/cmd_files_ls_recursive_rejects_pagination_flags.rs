#![cfg(target_os = "linux")]

use assert_cmd::Command;
use predicates::str::contains;

fn base() -> Command {
    let mut c = Command::cargo_bin("sharepoint").unwrap();
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env(
            "SHAREPOINT_TENANT_ID",
            "00000000-0000-0000-0000-000000000000",
        )
        .env("SHAREPOINT_CLIENT_ID", "client-1");
    c
}

#[test]
fn recursive_with_limit_rejected() {
    base()
        .args([
            "--json",
            "files",
            "ls",
            "--recursive",
            "--limit",
            "5",
            "Lib",
        ])
        .assert()
        .code(2)
        .stdout(contains("recursive"));
}

#[test]
fn recursive_with_all_rejected() {
    base()
        .args(["--json", "files", "ls", "--recursive", "--all", "Lib"])
        .assert()
        .code(2)
        .stdout(contains("recursive"));
}

#[test]
fn recursive_with_page_rejected() {
    base()
        .args([
            "--json",
            "files",
            "ls",
            "--recursive",
            "--page",
            "abc",
            "Lib",
        ])
        .assert()
        .code(2)
        .stdout(contains("recursive"));
}
