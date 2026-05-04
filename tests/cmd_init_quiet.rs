use assert_cmd::Command;

#[test]
fn init_errors_with_quiet() {
    let out = Command::cargo_bin("sharepoint")
        .unwrap()
        .args(["--quiet", "init"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("interactive"), "got: {combined}");
}
