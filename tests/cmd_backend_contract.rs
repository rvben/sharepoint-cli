mod support;

use serde_json::{Value, json};
use support::backend::{BACKENDS, Backend, FILE_REF, LIBRARY_REF};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn items(backend: Backend, names: &[&str]) -> Value {
    json!({"value": names.iter().map(|name| match backend {
        Backend::Graph => json!({"id": name, "name": name, "size": 7, "file": {}}),
        Backend::Rest => json!({"UniqueId": name, "Name": name, "Length": "7",
            "ServerRelativeUrl": format!("/sites/Marketing/Documents/{name}")}),
    }).collect::<Vec<_>>()})
}

fn children(backend: Backend) -> &'static str {
    match backend {
        Backend::Graph => "/drives/D1/root/children",
        Backend::Rest => {
            "/sites/Marketing/_api/web/GetFolderByServerRelativePath(decodedurl='/sites/Marketing/Documents')/Files"
        }
    }
}

async fn empty_folders(backend: Backend, server: &MockServer) {
    if matches!(backend, Backend::Rest) {
        Mock::given(method("GET"))
            .and(path("/sites/Marketing/_api/web/GetFolderByServerRelativePath(decodedurl='/sites/Marketing/Documents')/Folders"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": []})))
            .mount(server).await;
    }
}

#[tokio::test]
async fn list_json_fields_quiet_and_text_contract_match_across_backends() {
    for backend in BACKENDS {
        let server = backend.server().await;
        empty_folders(backend, &server).await;
        Mock::given(method("GET"))
            .and(path(children(backend)))
            .respond_with(ResponseTemplate::new(200).set_body_json(items(backend, &["hello.bin"])))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        for format in [&[][..], &["--output", "json"], &["--json"]] {
            let out = backend
                .command(&server.uri(), root.path())
                .args(format)
                .args([
                    "--quiet",
                    "files",
                    "ls",
                    LIBRARY_REF,
                    "--fields",
                    "name,size",
                ])
                .assert()
                .success()
                .get_output()
                .clone();
            assert!(out.stderr.is_empty(), "{backend:?}: {out:?}");
            let report: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(
                report,
                json!({"total": 1, "next": null, "items": [{"name": "hello.bin", "size": 7}]}),
                "{backend:?}"
            );
        }
        let out = backend
            .command(&server.uri(), root.path())
            .args(["--output", "text", "files", "ls", LIBRARY_REF])
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello.bin"));
        assert!(String::from_utf8_lossy(&out.stderr).contains("1 item(s)"));
        assert!(!out.stdout.starts_with(b"{"));
    }
}

#[tokio::test]
async fn list_at_exact_limit_has_no_phantom_next_page() {
    for backend in BACKENDS {
        let server = backend.server().await;
        empty_folders(backend, &server).await;
        Mock::given(method("GET"))
            .and(path(children(backend)))
            .respond_with(ResponseTemplate::new(200).set_body_json(items(backend, &["one", "two"])))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let out = backend
            .command(&server.uri(), root.path())
            .args(["files", "ls", LIBRARY_REF, "--limit", "2"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["items"].as_array().unwrap().len(), 2);
        assert!(report["next"].is_null(), "{backend:?}: {report}");
    }
}

#[tokio::test]
async fn graph_list_cursor_moves_directly_to_next_server_page_at_a_page_boundary() {
    let backend = Backend::Graph;
    let server = backend.server().await;
    let mut first = items(backend, &["one", "two"]);
    first["@odata.nextLink"] = json!(format!("{}{}?page=2", server.uri(), children(backend)));
    Mock::given(method("GET"))
        .and(path(children(backend)))
        .and(query_param_is_missing("page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(children(backend)))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(items(backend, &["three"])))
        .expect(1)
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let out = backend
        .command(&server.uri(), root.path())
        .args(["files", "ls", LIBRARY_REF, "--limit", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let first: Value = serde_json::from_slice(&out).unwrap();
    let out = backend
        .command(&server.uri(), root.path())
        .args([
            "files",
            "ls",
            LIBRARY_REF,
            "--limit",
            "2",
            "--page",
            first["next"].as_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let last: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(last["items"][0]["name"], "three");
    assert!(last["next"].is_null());
}

#[tokio::test]
async fn failures_have_stable_exit_codes_and_json_stderr_without_stdout_data() {
    for backend in BACKENDS {
        for (status, exit, kind) in [
            (401, 3, "auth"),
            (403, 3, "auth"),
            (404, 4, "not_found"),
            (429, 6, "rate_limit"),
            (503, 5, "api_error"),
        ] {
            let server = backend.server().await;
            Mock::given(method("GET"))
                .and(path(backend.content_path()))
                .respond_with(ResponseTemplate::new(status).insert_header("Retry-After", "0"))
                .mount(&server)
                .await;
            let root = tempfile::tempdir().unwrap();
            let out = backend
                .command(&server.uri(), root.path())
                .args(["--quiet", "files", "download", FILE_REF, "--path", "-"])
                .assert()
                .code(exit)
                .get_output()
                .clone();
            assert!(out.stdout.is_empty(), "{backend:?} {status}: {out:?}");
            let error: Value = serde_json::from_slice(&out.stderr).unwrap();
            assert_eq!(error["error"]["exit_code"], exit);
            assert_eq!(error["error"]["kind"], kind, "{backend:?} {status}");
            assert!(!error["error"]["message"].as_str().unwrap().is_empty());
        }
    }
}

#[test]
fn zero_limits_are_input_errors_before_any_network_request() {
    let root = tempfile::tempdir().unwrap();
    for backend in BACKENDS {
        for command in [
            vec!["sites", "list"],
            vec!["drives", "list", "default"],
            vec!["files", "ls", LIBRARY_REF],
            vec!["files", "find", LIBRARY_REF],
            vec!["auth", "status", "--offline"],
        ] {
            backend
                .command("http://127.0.0.1:1", root.path())
                .args(command)
                .args(["--limit", "0"])
                .assert()
                .code(2);
        }
    }
}

#[tokio::test]
async fn malformed_api_bodies_are_api_errors_on_both_backends() {
    for backend in BACKENDS {
        for body in ["not JSON", "{}"] {
            let server = backend.server().await;
            Mock::given(method("GET"))
                .and(path(backend.resolution()[0].0))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .with_priority(1)
                .mount(&server)
                .await;
            let root = tempfile::tempdir().unwrap();
            let out = backend
                .command(&server.uri(), root.path())
                .args(["files", "ls", LIBRARY_REF])
                .assert()
                .code(5)
                .get_output()
                .clone();
            assert!(out.stdout.is_empty());
            let error: Value = serde_json::from_slice(&out.stderr).unwrap();
            assert_eq!(error["error"]["kind"], "api_error");
            assert_eq!(error["error"]["exit_code"], 5);
        }
    }
}

fn search_rows(names: &[&str], total: usize) -> Value {
    json!({"PrimaryQueryResult":{"RelevantResults":{
        "TotalRows": total, "RowCount": names.len(), "Table":{"Rows":
            names.iter().map(|name| json!({"Cells":[
                {"Key":"Title","Value":name}, {"Key":"FileName","Value":name},
                {"Key":"WebId","Value":name}, {"Key":"UniqueId","Value":name},
                {"Key":"Path","Value":format!("https://contoso.sharepoint.com/sites/Marketing/Documents/{name}")},
                {"Key":"Size","Value":"7"}
            ]})).collect::<Vec<_>>()
        }
    }}})
}

#[tokio::test]
async fn site_and_search_pages_at_exact_limit_finish_without_phantom_cursors() {
    for backend in BACKENDS {
        for sites in [true, false] {
            let server = backend.server().await;
            let (route, body) = match (backend, sites) {
                (Backend::Graph, true) => (
                    "/me/followedSites",
                    json!({"value":[{
                        "id":"S1", "displayName":"Marketing", "webUrl":"https://contoso.sharepoint.com/sites/Marketing"
                    }]}),
                ),
                (Backend::Graph, false) => (
                    "/drives/D1/root/search(q='%2A')",
                    items(backend, &["one.txt"]),
                ),
                (Backend::Rest, _) => ("/_api/search/query", search_rows(&["one.txt"], 1)),
            };
            Mock::given(method("GET"))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let root = tempfile::tempdir().unwrap();
            let command = if sites {
                vec!["sites", "list"]
            } else {
                vec!["files", "find", LIBRARY_REF]
            };
            let out = backend
                .command(&server.uri(), root.path())
                .args(command)
                .args(["--limit", "1"])
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let report: Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(report["total"], 1);
            assert!(
                report["next"].is_null(),
                "{backend:?} sites={sites}: {report}"
            );
        }
    }
}

#[tokio::test]
async fn search_glob_cursors_resume_without_skipping_or_repeating_matches() {
    for backend in BACKENDS {
        let server = backend.server().await;
        let route = match backend {
            Backend::Graph => "/drives/D1/root/search(q='%2A')",
            Backend::Rest => "/_api/search/query",
        };
        let mut first = match backend {
            Backend::Graph => items(backend, &["skip.log", "one.txt"]),
            Backend::Rest => search_rows(&["skip.log", "one.txt"], 3),
        };
        if matches!(backend, Backend::Graph) {
            first["@odata.nextLink"] = json!(format!("{}{route}?offset=2", server.uri()));
        }
        let mock = Mock::given(method("GET")).and(path(route));
        let mock = match backend {
            Backend::Graph => mock.and(query_param_is_missing("offset")),
            Backend::Rest => mock.and(query_param("startrow", "0")),
        };
        mock.respond_with(ResponseTemplate::new(200).set_body_json(first))
            .expect(1)
            .mount(&server)
            .await;
        let last = match backend {
            Backend::Graph => items(backend, &["two.txt"]),
            Backend::Rest => search_rows(&["two.txt"], 3),
        };
        Mock::given(method("GET"))
            .and(path(route))
            .and(query_param(
                if matches!(backend, Backend::Graph) {
                    "offset"
                } else {
                    "startrow"
                },
                "2",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(last))
            .expect(1)
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let out = backend
            .command(&server.uri(), root.path())
            .args([
                "files",
                "find",
                LIBRARY_REF,
                "--name",
                "*.txt",
                "--limit",
                "1",
                "--fields",
                "name",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let first: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(first["items"], json!([{"name":"one.txt"}]));
        let token = first["next"].as_str().unwrap();
        let out = backend
            .command(&server.uri(), root.path())
            .args([
                "files",
                "find",
                LIBRARY_REF,
                "--name",
                "*.txt",
                "--limit",
                "1",
                "--fields",
                "name",
                "--page",
                token,
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let last: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(last["items"], json!([{"name":"two.txt"}]));
        assert!(last["next"].is_null());
    }
}
