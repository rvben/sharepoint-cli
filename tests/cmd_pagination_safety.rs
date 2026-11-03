mod support;

use serde_json::json;
use sharepoint_cli::graph::{Cursor, encode_cursor};
use support::backend::{BACKENDS, Backend, LIBRARY_REF};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn page_tokens_cannot_send_credentials_to_another_port_on_the_same_host() {
    for backend in BACKENDS {
        let origin = backend.server().await;
        let foreign = MockServer::start().await;
        let token = encode_cursor(&Cursor {
            next: Some(format!("{}/steal", foreign.uri())),
            skip: 0,
        });
        let root = tempfile::tempdir().unwrap();
        backend
            .command(&origin.uri(), root.path())
            .args(["sites", "list", "--page", &token])
            .assert()
            .code(2);
        assert!(
            foreign.received_requests().await.unwrap().is_empty(),
            "{backend:?}"
        );
    }
}

#[tokio::test]
async fn graph_server_continuations_cannot_send_bearer_tokens_to_another_origin() {
    for command in [
        vec!["sites", "list", "--all"],
        vec![
            "drives",
            "list",
            "https://contoso.sharepoint.com/sites/Marketing",
            "--all",
        ],
        vec!["files", "ls", LIBRARY_REF, "--all"],
        vec!["files", "find", LIBRARY_REF, "--all"],
    ] {
        let backend = Backend::Graph;
        let server = backend.server().await;
        let foreign = MockServer::start().await;
        let route = match command.as_slice() {
            ["sites", ..] => "/me/followedSites",
            ["drives", ..] => "/sites/S1/drives",
            ["files", "find", ..] => "/drives/D1/root/search(q='%2A')",
            _ => "/drives/D1/root/children",
        };
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [], "@odata.nextLink": format!("{}/steal", foreign.uri())
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        backend
            .command(&server.uri(), root.path())
            .args(command)
            .assert()
            .code(5);
        assert!(foreign.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn graph_cyclic_continuations_fail_instead_of_looping() {
    for command in [
        vec!["sites", "list", "--all"],
        vec![
            "drives",
            "list",
            "https://contoso.sharepoint.com/sites/Marketing",
            "--all",
        ],
        vec!["files", "ls", LIBRARY_REF, "--all"],
        vec!["files", "ls", LIBRARY_REF, "--recursive"],
        vec!["files", "find", LIBRARY_REF, "--all"],
    ] {
        let backend = Backend::Graph;
        let server = backend.server().await;
        // Keep resolution working, then introduce a cycle only in the selected collection.
        let route = match command.as_slice() {
            ["sites", ..] => "/me/followedSites",
            ["drives", ..] => "/sites/S1/drives",
            ["files", "find", ..] => "/drives/D1/root/search(q='%2A')",
            _ => "/drives/D1/root/children",
        };
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [], "@odata.nextLink": format!("{}{route}", server.uri())
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let out = backend
            .command(&server.uri(), root.path())
            .timeout(std::time::Duration::from_secs(5))
            .args(command)
            .assert()
            .code(5)
            .get_output()
            .clone();
        assert!(out.stdout.is_empty());
        let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("repeated a continuation")
        );
    }
}
