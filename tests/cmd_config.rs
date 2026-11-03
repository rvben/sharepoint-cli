use assert_cmd::Command;
use predicates::str::contains;

mod support;

#[test]
fn config_show_uses_xdg_directories_for_profiles_and_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config_root = dir.path().join("custom config");
    let cache_root = dir.path().join("custom cache");
    let profile_dir = config_root.join("sharepoint");
    std::fs::create_dir_all(&profile_dir).unwrap();
    std::fs::write(
        profile_dir.join("config.toml"),
        "[profile.default]\nbackend = 'rest'\nsharepoint_host = 'contoso.sharepoint.com'\n",
    )
    .unwrap();

    let out = support::isolated_command(dir.path())
        .env("XDG_CONFIG_HOME", &config_root)
        .env("XDG_CACHE_HOME", &cache_root)
        .args(["--output", "json", "config", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    for (field, expected) in [
        ("config_path", profile_dir.join("config.toml")),
        ("cache_path", cache_root.join("sharepoint/tokens.json")),
        ("session_path", cache_root.join("sharepoint/sessions.json")),
    ] {
        assert_eq!(
            std::path::Path::new(value[field].as_str().unwrap()),
            expected,
            "{field} must honor the XDG override"
        );
    }
    assert_eq!(value["backend"], "rest");
    assert_eq!(value["sharepoint_host"], "contoso.sharepoint.com");
}

#[test]
fn config_path_prints_path() {
    Command::cargo_bin("sharepoint")
        .unwrap()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(contains("config.toml"));
}

#[test]
fn config_show_has_exact_key_set() {
    // output() creates a pipe even when Nix gives the test runner a TTY.
    for args in [
        vec!["config", "show"],
        vec!["--output", "json", "config", "show"],
    ] {
        let out = Command::cargo_bin("sharepoint")
            .unwrap()
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        let obj = value.as_object().expect("top-level object");
        let mut actual: Vec<&str> = obj.keys().map(String::as_str).collect();
        actual.sort_unstable();
        let mut expected = vec![
            "backend",
            "cache_path",
            "client_id",
            "config_path",
            "default_site",
            "graph_endpoint",
            "login_endpoint",
            "profile",
            "read_only",
            "rest_endpoint",
            "session_path",
            "sharepoint_host",
            "site_aliases",
            "tenant_id",
        ];
        expected.sort_unstable();
        assert_eq!(actual, expected, "config show JSON keys drifted");
    }
}
