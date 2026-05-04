#![cfg(target_os = "linux")]

use assert_cmd::Command;

#[test]
fn sites_use_writes_default_site_to_config() {
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("sharepoint")
        .unwrap()
        .env("XDG_CONFIG_HOME", dir.path())
        // Cache dir must also be set to avoid reading the real token cache.
        .env("XDG_CACHE_HOME", dir.path())
        .env("SHAREPOINT_TENANT_ID", "contoso")
        .env("SHAREPOINT_CLIENT_ID", "client-1")
        .args([
            "sites",
            "use",
            "https://contoso.sharepoint.com/sites/Marketing",
        ])
        .assert()
        .success();

    let config_path = dir.path().join("sharepoint").join("config.toml");
    let content = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("config not written to {}: {e}", config_path.display()));

    assert!(
        content.contains("https://contoso.sharepoint.com/sites/Marketing"),
        "expected default_site URL in config, got:\n{content}"
    );
    assert!(
        content.contains("default_site"),
        "expected 'default_site' key in config, got:\n{content}"
    );
}

#[test]
fn sites_use_is_idempotent_on_second_call() {
    let dir = tempfile::tempdir().unwrap();

    for _ in 0..2 {
        Command::cargo_bin("sharepoint")
            .unwrap()
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_CACHE_HOME", dir.path())
            .env("SHAREPOINT_TENANT_ID", "contoso")
            .env("SHAREPOINT_CLIENT_ID", "client-1")
            .args([
                "sites",
                "use",
                "https://contoso.sharepoint.com/sites/Marketing",
            ])
            .assert()
            .success();
    }

    let config_path = dir.path().join("sharepoint").join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    // The URL should appear exactly once (not duplicated).
    let count = content
        .matches("https://contoso.sharepoint.com/sites/Marketing")
        .count();
    assert_eq!(
        count, 1,
        "URL should appear once; got {count} times in:\n{content}"
    );
}
