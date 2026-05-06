use assert_cmd::Command;
use predicates::str::contains;
use sharepoint_cli::graph::{Cursor, encode_cursor};

#[test]
fn page_token_pointing_at_attacker_host_is_rejected() {
    let cursor = Cursor {
        next: Some("https://attacker.example/v1.0/sites?$skiptoken=evil".to_string()),
        skip: 0,
    };
    let token = encode_cursor(&cursor);

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
