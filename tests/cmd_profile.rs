use assert_cmd::Command;

fn isolated(dir: &tempfile::TempDir) -> Command {
    let mut command = Command::cargo_bin("sharepoint").unwrap();
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path());
    command
}

fn init_profile(dir: &tempfile::TempDir, name: &str) {
    isolated(dir)
        .args([
            "--profile",
            name,
            "init",
            "--tenant",
            "contoso.onmicrosoft.com",
            "--client-id",
            "00000000-0000-0000-0000-000000000000",
            "--no-login",
        ])
        .assert()
        .success();
}

#[test]
fn profiles_can_be_listed_selected_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    init_profile(&dir, "work");
    init_profile(&dir, "sandbox");

    isolated(&dir)
        .args(["profile", "use", "work"])
        .assert()
        .success();

    let list = isolated(&dir)
        .args(["--output", "json", "profile", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let value: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(value["total"], 2);
    assert!(
        value["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| { profile["name"] == "work" && profile["active"] == true })
    );

    isolated(&dir)
        .args(["profile", "remove", "sandbox", "--yes"])
        .assert()
        .success();
}

#[test]
fn init_makes_a_named_profile_active() {
    let dir = tempfile::tempdir().unwrap();
    init_profile(&dir, "work");

    let show = isolated(&dir)
        .args(["--output", "json", "config", "show"])
        .output()
        .unwrap();
    assert!(show.status.success());
    let value: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(value["profile"], "work");
}
