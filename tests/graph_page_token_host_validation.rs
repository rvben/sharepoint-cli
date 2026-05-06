#![cfg(target_os = "linux")]

use assert_cmd::Command;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use predicates::str::contains;

#[test]
fn page_token_pointing_at_attacker_host_is_rejected() {
    let attacker = "https://attacker.example/v1.0/sites?$skiptoken=evil";
    let token = URL_SAFE_NO_PAD.encode(attacker.as_bytes());

    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .env(
            "MICROSOFT_GRAPH_ENDPOINT",
            "https://graph.microsoft.com/v1.0",
        )
        .env(
            "SHAREPOINT_TENANT_ID",
            "11111111-1111-1111-1111-111111111111",
        )
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args(["--json", "sites", "list", "--page", &token])
        .assert()
        .code(2)
        .stdout(contains("host"));
}
