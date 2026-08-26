use assert_cmd::Command;

#[test]
fn clap_error_renders_structured_error_on_stderr() {
    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .args(["--bogus-flag"])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();

    // The last non-empty line of stderr must be the JSON error envelope.
    let stderr_text = String::from_utf8_lossy(&output);
    let last_json_line = stderr_text
        .lines()
        .rfind(|l| l.trim_start().starts_with('{'))
        .expect("stderr must contain a JSON line");
    let v: serde_json::Value =
        serde_json::from_str(last_json_line).expect("last stderr JSON line must be valid JSON");

    assert_eq!(v["error"]["kind"], "invalid_input");
    assert_eq!(v["error"]["exit_code"], 2);
    let message = v["error"]["message"]
        .as_str()
        .expect("error.message must be a string");
    assert!(
        !message.starts_with("error: "),
        "should strip clap's leading 'error: ' prefix; got: {message}"
    );
    assert!(
        message.contains("--bogus-flag"),
        "message should name the offending flag; got: {message}"
    );
}
