use sharepoint_cli::auth::AuthContext;
use sharepoint_cli::config::ResolvedConfig;
use sharepoint_cli::graph::{GraphClient, download};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn download_streams_body_to_writer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drives/D1/root:/folder/file.txt:/content"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let cfg = ResolvedConfig {
        profile_name: "default".into(),
        tenant_id: Some("t".into()),
        client_id: Some("c".into()),
        default_site: None,
        read_only: false,
        site_aliases: Default::default(),
        graph_endpoint: server.uri(),
        login_endpoint: "https://login.example".into(),
        debug_http: false,
        access_token_override: Some("FAKE".into()),
        refresh_token_seed: None,
    };
    let auth = AuthContext::new(cfg, dir.path().join("tokens.json"));
    let graph = GraphClient::new(auth);

    let mut buf = Vec::new();
    let n = download::download_to_writer(&graph, "D1", "/folder/file.txt", &mut buf)
        .await
        .unwrap();
    assert_eq!(n, 11);
    assert_eq!(buf, b"hello world");
}
