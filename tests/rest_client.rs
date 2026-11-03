use sharepoint_cli::error::CliError;
use sharepoint_cli::rest::RestClient;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COOKIE: &str = "FedAuth=F; rtFa=R";

#[tokio::test]
async fn sends_cookie_and_nometadata_accept() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sites/Marketing/_api/web"))
        .and(header("cookie", COOKIE))
        .and(header("accept", "application/json;odata=nometadata"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"Title": "Marketing"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let url = client
        .site_api(
            "https://contoso.sharepoint.com/sites/Marketing",
            "/_api/web",
        )
        .unwrap();
    let v: serde_json::Value = client.get_json(&url).await.unwrap();
    assert_eq!(v["Title"], "Marketing");
}

#[tokio::test]
async fn refuses_foreign_host() {
    let client = RestClient::new("http://127.0.0.1:1", COOKIE).unwrap();
    let err = client
        .get_json::<serde_json::Value>("https://attacker.example/_api/web")
        .await
        .unwrap_err();
    assert!(matches!(err, CliError::Input(_)), "{err:?}");
}

#[tokio::test]
async fn unauthorized_maps_to_auth_error_with_hint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "odata.error": {"code": "-2147024891", "message": {"lang": "en-US", "value": "Access denied."}}
        })))
        .mount(&server)
        .await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&format!("{}/_api/web", server.uri()))
        .await
        .unwrap_err();
    match err {
        CliError::Auth(m) => {
            assert!(m.contains("Access denied."), "{m}");
            assert!(m.contains("sharepoint auth login"), "{m}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
}

#[tokio::test]
async fn retries_503_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let v: serde_json::Value = client
        .get_json(&format!("{}/_api/web", server.uri()))
        .await
        .unwrap();
    assert_eq!(v["ok"], true);
}

#[tokio::test]
async fn exhausts_retries_on_429_then_rate_limits() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .mount(&server)
        .await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&format!("{}/_api/web", server.uri()))
        .await
        .unwrap_err();
    assert!(matches!(err, CliError::RateLimit), "{err:?}");
}

#[tokio::test]
async fn redirect_to_other_origin_maps_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "https://login.microsoftonline.com/signin"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&format!("{}/_api/web", server.uri()))
        .await
        .unwrap_err();
    match err {
        CliError::Auth(m) => {
            assert!(m.contains("redirected to sign-in"), "{m}");
            assert!(m.contains("sharepoint auth login"), "{m}");
        }
        other => panic!("expected Auth, got {other:?}"),
    }
}

#[tokio::test]
async fn refuses_same_host_different_scheme() {
    let server = MockServer::start().await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let other_scheme_url = format!("https://{}/_api/web", server.address());
    let err = client
        .get_json::<serde_json::Value>(&other_scheme_url)
        .await
        .unwrap_err();
    assert!(matches!(err, CliError::Input(_)), "{err:?}");
}

#[tokio::test]
async fn refuses_same_host_different_port() {
    let server = MockServer::start().await;
    let client = RestClient::new(&server.uri(), COOKIE).unwrap();
    let addr = server.address();
    let other_port_url = format!("http://{}:{}/_api/web", addr.ip(), addr.port() + 1);
    let err = client
        .get_json::<serde_json::Value>(&other_port_url)
        .await
        .unwrap_err();
    assert!(matches!(err, CliError::Input(_)), "{err:?}");
}

#[test]
fn site_api_rejects_other_tenant_host() {
    let client =
        RestClient::with_host("http://127.0.0.1:1", "contoso.sharepoint.com", COOKIE).unwrap();
    let err = client
        .site_api("https://fabrikam.sharepoint.com/sites/X", "/_api/web")
        .unwrap_err();
    assert!(matches!(err, CliError::Input(_)));
}
