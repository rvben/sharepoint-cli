mod support;

use support::backend::{BACKENDS, Backend, FILE_REF};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const CONTENT: &[u8] = b"\0\xff\x01complete download\r\n";

#[tokio::test]
async fn failed_download_preserves_existing_file_and_cleans_temporary_files() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let downloads = root.path().join("downloads");
        std::fs::create_dir(&downloads).unwrap();
        let target = downloads.join("important.bin");
        std::fs::write(&target, b"original bytes").unwrap();

        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--overwrite", "--path"])
            .arg(&target)
            .assert()
            .code(4);

        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"original bytes",
            "{backend:?}"
        );
        assert_eq!(
            std::fs::read_dir(&downloads).unwrap().count(),
            1,
            "{backend:?}"
        );
    }
}

#[tokio::test]
async fn successful_download_commits_complete_bytes_and_reports_destination() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(CONTENT))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("nested/downloads/complete.bin");
        let out = backend
            .command(&server.uri(), root.path())
            .args(["--quiet", "files", "download", FILE_REF, "--path"])
            .arg(&target)
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(out.stderr.is_empty(), "{backend:?}: {out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["bytes"], CONTENT.len());
        assert_eq!(report["path"], target.to_str().unwrap());
        assert_eq!(std::fs::read(&target).unwrap(), CONTENT, "{backend:?}");
        assert_eq!(
            std::fs::read_dir(target.parent().unwrap()).unwrap().count(),
            1
        );

        // Explicit overwrite replaces an existing file only after success.
        std::fs::write(&target, b"old").unwrap();
        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--overwrite", "--path"])
            .arg(&target)
            .assert()
            .success();
        assert_eq!(std::fs::read(&target).unwrap(), CONTENT, "{backend:?}");
    }
}

#[tokio::test]
async fn failed_new_download_leaves_no_destination_or_staging_file() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let downloads = root.path().join("downloads");
        let target = downloads.join("new.bin");
        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--path"])
            .arg(&target)
            .assert()
            .code(3);
        assert_eq!(
            std::fs::read_dir(&downloads).unwrap().count(),
            0,
            "{backend:?}"
        );
    }
}

#[tokio::test]
async fn download_does_not_clobber_a_file_created_during_the_transfer() {
    for backend in BACKENDS {
        let server = backend.server().await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("racing.bin");
        let concurrent_target = target.clone();
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(move |_: &wiremock::Request| {
                std::fs::write(&concurrent_target, b"concurrent writer").unwrap();
                ResponseTemplate::new(200).set_body_bytes(CONTENT)
            })
            .mount(&server)
            .await;
        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--path"])
            .arg(&target)
            .assert()
            .code(2);
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"concurrent writer",
            "{backend:?}"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn download_refuses_existing_files_without_requesting_content() {
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(CONTENT))
            .expect(0)
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("existing.bin");
        std::fs::write(&target, b"original").unwrap();
        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--path"])
            .arg(&target)
            .assert()
            .code(2);
        assert_eq!(std::fs::read(&target).unwrap(), b"original", "{backend:?}");
    }
}

// A real HTTP peer closes before Content-Length bytes arrive. Mock servers
// normalize response framing, so they cannot model an interrupted body reliably.
fn truncated_server(backend: support::backend::Backend) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut served = 0;
        while served < 3 && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("accept: {e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let api = request
                .split_whitespace()
                .nth(1)
                .unwrap()
                .split('?')
                .next()
                .unwrap();
            if let Some((_, body)) = backend.resolution().iter().find(|(route, _)| *route == api) {
                let body = body.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            } else {
                assert!(
                    api.ends_with(":/content") || api.ends_with("/$value"),
                    "{api}"
                );
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial").unwrap();
            }
            served += 1;
        }
        assert_eq!(served, 3);
    });
    (endpoint, worker)
}

#[test]
fn interrupted_download_cleans_up_and_preserves_existing_bytes() {
    for backend in BACKENDS {
        for existing in [false, true] {
            let (endpoint, worker) = truncated_server(backend);
            let root = tempfile::tempdir().unwrap();
            let downloads = root.path().join("downloads");
            std::fs::create_dir(&downloads).unwrap();
            let target = downloads.join("interrupted.bin");
            if existing {
                std::fs::write(&target, b"keep this file").unwrap();
            }
            backend
                .command(&endpoint, root.path())
                .timeout(std::time::Duration::from_secs(15))
                .args(["files", "download", FILE_REF, "--overwrite", "--path"])
                .arg(&target)
                .assert()
                .code(1);
            worker.join().unwrap();
            if existing {
                assert_eq!(
                    std::fs::read(&target).unwrap(),
                    b"keep this file",
                    "{backend:?}"
                );
            } else {
                assert!(!target.exists(), "{backend:?}");
            }
            assert_eq!(
                std::fs::read_dir(&downloads).unwrap().count(),
                usize::from(existing)
            );
        }
    }
}

#[tokio::test]
async fn graph_download_follows_storage_redirect_without_forwarding_bearer_token() {
    let server = Backend::Graph.server().await;
    let storage = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(Backend::Graph.content_path()))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/download", storage.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(CONTENT))
        .expect(1)
        .mount(&storage)
        .await;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("redirected.bin");
    Backend::Graph
        .command(&server.uri(), root.path())
        .args(["files", "download", FILE_REF, "--path"])
        .arg(&target)
        .assert()
        .success();
    assert_eq!(std::fs::read(target).unwrap(), CONTENT);
    let requests = storage.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].headers.contains_key("authorization"));
    assert!(!requests[0].headers.contains_key("cookie"));
}

#[cfg(unix)]
#[tokio::test]
async fn overwrite_replaces_symlink_entry_and_keeps_download_private() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for backend in BACKENDS {
        let server = backend.server().await;
        Mock::given(method("GET"))
            .and(path(backend.content_path()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(CONTENT))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original.bin");
        std::fs::write(&original, b"linked original").unwrap();
        let target = root.path().join("link.bin");
        symlink(&original, &target).unwrap();
        backend
            .command(&server.uri(), root.path())
            .args(["files", "download", FILE_REF, "--overwrite", "--path"])
            .arg(&target)
            .assert()
            .success();
        assert_eq!(std::fs::read(original).unwrap(), b"linked original");
        assert_eq!(std::fs::read(&target).unwrap(), CONTENT);
        assert!(
            !std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}
