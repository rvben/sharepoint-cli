use assert_cmd::Command;

/// The clispec v0.2 JSON Schema, vendored so the test runs offline.
const CLISPEC_V0_2: &str = include_str!("fixtures/clispec-v0.2.json");

/// `sharepoint schema` exits 0 and emits valid JSON without any
/// credentials, config file, or network access.
#[test]
fn schema_works_without_credentials() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let schema_doc: serde_json::Value =
        serde_json::from_slice(&output).expect("schema must emit valid JSON");

    // Must declare the clispec version.
    assert_eq!(
        schema_doc["clispec"].as_str().unwrap_or(""),
        "0.2",
        "clispec field must be '0.2'"
    );

    // Must name the tool.
    assert_eq!(
        schema_doc["name"].as_str().unwrap_or(""),
        "sharepoint",
        "name field must be 'sharepoint'"
    );

    // Must include a version string.
    assert!(
        schema_doc["version"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "version field must be a non-empty string"
    );
}

/// The output of `sharepoint schema` must validate against the clispec v0.2
/// JSON Schema (vendored in tests/fixtures/clispec-v0.2.json).
#[test]
fn schema_validates_against_clispec_v0_2() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let instance: serde_json::Value =
        serde_json::from_slice(&output).expect("schema must emit valid JSON");

    let meta_schema: serde_json::Value =
        serde_json::from_str(CLISPEC_V0_2).expect("vendored clispec schema must be valid JSON");

    let validator = jsonschema::draft202012::new(&meta_schema)
        .expect("vendored clispec schema must be a valid Draft 2020-12 schema");

    let errors: Vec<_> = validator.iter_errors(&instance).collect();
    assert!(
        errors.is_empty(),
        "schema output must validate against clispec v0.2: {}",
        errors
            .iter()
            .map(|e| format!("{}: {}", e.instance_path(), e))
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// The schema must declare every error kind with an exit_code.
#[test]
fn schema_errors_all_have_exit_codes() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let doc: serde_json::Value =
        serde_json::from_slice(&output).expect("schema must emit valid JSON");

    let errors = doc["errors"].as_array().expect("errors must be an array");
    assert!(!errors.is_empty(), "errors array must not be empty");

    for entry in errors {
        let kind = entry["kind"].as_str().expect("each error must have a kind");
        assert!(
            entry["exit_code"].is_number(),
            "error kind '{kind}' is missing an exit_code"
        );
    }
}

/// Every leaf command in the schema must carry an explicit mutating marker.
#[test]
fn schema_all_leaf_commands_have_mutating() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::cargo_bin("sharepoint")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("XDG_CACHE_HOME", dir.path())
        .args(["schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let doc: serde_json::Value =
        serde_json::from_slice(&output).expect("schema must emit valid JSON");

    fn check_leaves(cmd: &serde_json::Value, path: &str) {
        if let Some(subs) = cmd.get("subcommands").and_then(|s| s.as_array())
            && !subs.is_empty()
        {
            for sub in subs {
                let name = sub["name"].as_str().unwrap_or("?");
                check_leaves(sub, &format!("{path} {name}"));
            }
            return;
        }
        assert!(
            cmd.get("mutating").and_then(|m| m.as_bool()).is_some(),
            "leaf command '{path}' is missing a boolean mutating field"
        );
    }

    let commands = doc["commands"]
        .as_array()
        .expect("commands must be an array");
    for cmd in commands {
        let name = cmd["name"].as_str().unwrap_or("?");
        check_leaves(cmd, name);
    }
}
