#![cfg(target_os = "linux")]

use assert_cmd::Command;
use chrono::{Duration, Utc};
use predicates::str::contains;
use sharepoint_cli::auth::token_cache::{Account, CacheEntry, cache_key, load, upsert};

fn make_entry(username: &str, tenant_id: &str, oid: &str) -> CacheEntry {
    CacheEntry {
        account: Account {
            username: username.into(),
            name: Some(username.to_string()),
            tenant_id: tenant_id.into(),
            oid: oid.into(),
        },
        access_token: "AT".into(),
        access_token_expires_at: Utc::now() + Duration::minutes(30),
        refresh_token: Some("RT".to_string()),
        scopes: vec![],
    }
}

#[test]
fn auth_logout_removes_matching_tenant_entries() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("tokens.json");

    // Seed two entries: one for contoso:client-1 (should be removed), one for other tenant (kept).
    upsert(
        &cache_path,
        &cache_key("contoso", "client-1", "OID-1"),
        make_entry("alice@contoso.com", "contoso", "OID-1"),
    )
    .unwrap();
    upsert(
        &cache_path,
        &cache_key("other", "client-1", "OID-2"),
        make_entry("bob@other.com", "other", "OID-2"),
    )
    .unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args(["--json", "auth", "logout"])
        .assert()
        .success()
        .stdout(contains("\"removed\": 1"));

    // Reload the cache and verify only the other-tenant entry remains.
    let cache = load(&cache_path).unwrap();
    assert_eq!(
        cache.entries.len(),
        1,
        "expected 1 remaining entry, got {}: {:#?}",
        cache.entries.len(),
        cache.entries.keys().collect::<Vec<_>>()
    );
    let remaining_key = cache.entries.keys().next().unwrap();
    assert!(
        remaining_key.starts_with("other:client-1:"),
        "expected the 'other' tenant entry to remain, got key: {remaining_key}"
    );
    assert!(!cache.entries.contains_key("contoso:client-1:OID-1"));
}

#[test]
fn auth_logout_is_no_op_when_no_matching_entry() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("sharepoint");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("tokens.json");

    // Only seed an entry for a different tenant.
    upsert(
        &cache_path,
        &cache_key("other", "client-1", "OID-X"),
        make_entry("carol@other.com", "other", "OID-X"),
    )
    .unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args(["--json", "auth", "logout"])
        .assert()
        .success()
        .stdout(contains("\"removed\": 0"));

    // Unrelated entry must be untouched.
    let cache = load(&cache_path).unwrap();
    assert_eq!(cache.entries.len(), 1);
}
