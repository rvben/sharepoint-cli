#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::str::contains;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, upsert};

#[test]
fn auth_status_prints_cached_account() {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("tokens.json");
    let key = cache_key("contoso", "client-1", "OID-1");
    upsert(
        &cache_path,
        &key,
        CacheEntry {
            account: Account {
                username: "alice@contoso.com".into(),
                name: Some("Alice".to_string()),
                tenant_id: "contoso".into(),
                oid: "OID-1".into(),
            },
            access_token: "AT".into(),
            access_token_expires_at: Utc::now() + Duration::minutes(30),
            refresh_token: Some("RT".to_string()),
            scopes: vec!["User.Read".into()],
        },
    )
    .unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args(["--json", "auth", "status"])
        .assert()
        .success()
        .stdout(contains("alice@contoso.com"));
}
