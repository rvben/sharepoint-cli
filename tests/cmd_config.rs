use assert_cmd::Command;
use predicates::str::contains;

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
    let out = Command::cargo_bin("sharepoint")
        .unwrap()
        .args(["--json", "config", "show"])
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
        "cache_path",
        "client_id",
        "config_path",
        "default_site",
        "graph_endpoint",
        "login_endpoint",
        "profile",
        "read_only",
        "site_aliases",
        "tenant_id",
    ];
    expected.sort_unstable();
    assert_eq!(actual, expected, "config show JSON keys drifted");
}
