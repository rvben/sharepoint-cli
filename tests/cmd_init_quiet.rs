use assert_cmd::Command;

fn isolated() -> (tempfile::TempDir, Command) {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("sharepoint").unwrap();
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path());
    (dir, command)
}

#[test]
fn init_requires_explicit_values_when_non_interactive() {
    let (_dir, mut command) = isolated();
    let out = command.args(["--output", "text", "init"]).output().unwrap();
    assert!(!out.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("--tenant"), "got: {combined}");
}

#[test]
fn init_can_configure_quietly_without_login() {
    let (dir, mut command) = isolated();
    let out = command
        .args([
            "--quiet",
            "init",
            "--tenant",
            "contoso.onmicrosoft.com",
            "--client-id",
            "00000000-0000-0000-0000-000000000000",
            "--default-site",
            "Marketing",
            "--read-only",
            "--no-login",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["signed_in"], false);
    assert_eq!(result["read_only"], true);

    let config_path = result["config_path"].as_str().unwrap();
    assert!(
        std::path::Path::new(config_path).starts_with(dir.path()),
        "config escaped isolated home: {config_path}"
    );
    let config = std::fs::read_to_string(config_path).unwrap();
    assert!(config.contains("contoso.onmicrosoft.com"));
    assert!(config.contains("default_site = \"Marketing\""));
    assert!(config.contains("read_only = true"));
}
