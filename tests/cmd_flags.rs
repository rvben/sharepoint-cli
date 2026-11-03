mod support;

use support::backend::{BACKENDS, FILE_REF};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[test]
fn parse_errors_honor_explicit_output_format() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["--output", "text", "--bogus"],
        vec!["files", "ls", "--output", "text", "--limit", "0", "REF"],
    ] {
        let out = support::isolated_command(root.path())
            .args(args)
            .assert()
            .code(2)
            .get_output()
            .clone();
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&out.stderr).starts_with("error: "),
            "{out:?}"
        );
    }
    let out = support::isolated_command(root.path())
        .args(["--output", "json", "--bogus"])
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "invalid_input");
}

#[test]
fn option_values_named_like_commands_do_not_trigger_download_aliases() {
    let root = tempfile::tempdir().unwrap();
    let out = support::isolated_command(root.path())
        .args([
            "--profile",
            "files",
            "--client-id",
            "download",
            "config",
            "show",
            "--output",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let config: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(config["profile"], "files");
    assert_eq!(config["client_id"], "download");
}

#[tokio::test]
async fn canonical_download_path_and_legacy_aliases_work_on_both_backends() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"exact bytes"))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        for spelling in [
            vec!["--path", "out.bin"],
            vec!["--output", "out.bin"],
            vec!["--output=out.bin"],
            vec!["-o", "out.bin"],
            vec!["-oout.bin"],
        ] {
            let out = backend
                .command(&server.uri(), root.path())
                .current_dir(root.path())
                .args(["--output", "json", "files", "download", FILE_REF])
                .args(spelling)
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(report, serde_json::json!({"path": "out.bin", "bytes": 11}));
            let target = root.path().join("out.bin");
            assert_eq!(std::fs::read(&target).unwrap(), b"exact bytes");
            std::fs::remove_file(target).unwrap();
        }
    }
}

#[tokio::test]
async fn stdout_download_is_raw_bytes_even_in_json_mode() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\0\xffraw\n"))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let out = backend
            .command(&server.uri(), root.path())
            .args([
                "--output", "json", "--quiet", "files", "download", FILE_REF, "--path", "-",
            ])
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(out.stdout, b"\0\xffraw\n");
        assert!(out.stderr.is_empty());
    }
}
