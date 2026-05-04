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
fn config_show_omits_token_fields() {
    let out = Command::cargo_bin("sharepoint")
        .unwrap()
        .args(["--json", "config", "show"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("access_token"), "leaked: {stdout}");
    assert!(!stdout.contains("refresh_token"), "leaked: {stdout}");
}
