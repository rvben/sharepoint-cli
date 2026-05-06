use assert_cmd::Command;

#[test]
fn clap_error_renders_as_json_on_stdout() {
    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .args(["--bogus-flag"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("stdout must be valid JSON");
    assert_eq!(v["error"]["code"], "input");
    assert_eq!(v["error"]["exit"], 2);
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
