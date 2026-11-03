#[path = "../examples/simulator/server.rs"]
mod server;
#[path = "../examples/simulator/smoke.rs"]
mod smoke;

use server::{COOKIE, Fixtures, Scenario, Simulator};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_smoke_against_reusable_simulator() {
    let checks = smoke::run(std::path::Path::new(env!("CARGO_BIN_EXE_sharepoint")))
        .await
        .unwrap();
    assert!(
        checks
            .iter()
            .any(|check| check == "PnP recordings: identity")
    );
    assert_eq!(checks.len(), 20);
}

#[tokio::test]
async fn simulator_serves_custom_fixtures_and_rejects_unsupported_requests() {
    let fixtures = Fixtures::parse(r#"{"sites":[{"id":"w","name":"Custom","host":"custom.sharepoint.com","path":"/sites/Custom","libraries":[{"id":"l","name":"Documents","root":"Shared Documents","template":101,"files":[{"path":"Nested/100%20 café.txt","content":"custom bytes"}]}]}]}"#).unwrap();
    assert_eq!(fixtures.host(), "custom.sharepoint.com");
    let simulator = Simulator::start(0, Scenario::Healthy, 1, fixtures)
        .await
        .unwrap();
    let http = reqwest::Client::builder()
        .default_headers(reqwest::header::HeaderMap::from_iter([(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json;odata=nometadata"),
        )]))
        .build()
        .unwrap();
    let endpoint = simulator.uri();
    assert!(endpoint.starts_with("http://127.0.0.1:"));
    assert_eq!(
        http.get(format!("{endpoint}/_simulator/health"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        http.get(format!("{endpoint}/_api/web/currentuser"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.post(format!("{endpoint}/_api/web/currentuser"))
            .header("Cookie", COOKIE)
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    assert_eq!(
        http.get(format!("{endpoint}/_api/unsupported"))
            .header("Cookie", COOKIE)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let file = format!(
        "{endpoint}/sites/Custom/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Custom/Shared%20Documents/Nested/100%2520%20caf%C3%A9.txt')/$value"
    );
    let response = http
        .get(file)
        .header("Cookie", COOKIE)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"custom bytes");
    let folder = format!(
        "{endpoint}/sites/Custom/_api/web/GetFolderByServerRelativePath(decodedurl='/sites/Custom/Shared%20Documents')/Folders"
    );
    let response: serde_json::Value = http
        .get(folder)
        .header("Cookie", COOKIE)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["value"][0]["Name"], "Nested");
    let response = http
        .get(format!("{endpoint}/_api/search/query"))
        .header("Cookie", COOKIE)
        .query(&[
            ("querytext", "'contentclass:STS_Site'"),
            ("startrow", "invalid"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[test]
fn simulator_rejects_bad_fixture_paths() {
    let fixture = include_str!("../examples/simulator/fixtures.json");
    for path in ["../escape.txt", "/absolute.txt", "a//b.txt", "a/../b.txt"] {
        assert!(Fixtures::parse(&fixture.replace("hello.txt", path)).is_err());
    }
    assert!(Fixtures::parse(&fixture.replace("hello.txt", "Plans")).is_err());
    assert!(
        Fixtures::parse(
            &fixture.replace("simulator.sharepoint.com", "invalid'host.sharepoint.com")
        )
        .is_err()
    );
    assert!(Fixtures::parse("{\"sites\":[]}").is_err());
}

#[tokio::test]
async fn simulator_projects_selected_fields_and_validates_expansion() {
    let simulator = Simulator::start(0, Scenario::Healthy, 50, Fixtures::bundled())
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let url = format!("{}/sites/Marketing/_api/web/lists", simulator.uri());
    let response: serde_json::Value = http
        .get(&url)
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[("$select", "Title")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        response["value"][0],
        serde_json::json!({"Title": "Documents"})
    );
    let response = http
        .get(&url)
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[
            ("$select", "RootFolder/ServerRelativeUrl"),
            ("$expand", "RootFolder"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["value"][0],
        serde_json::json!({"RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Shared Documents"}})
    );
    for params in [
        vec![("$select", "Typo")],
        vec![("$expand", "Typo")],
        vec![("$select", "RootFolder/Typo"), ("$expand", "RootFolder")],
    ] {
        let response = http
            .get(&url)
            .header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata")
            .query(&params)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }
}

#[tokio::test]
async fn simulator_filters_mixed_lists_with_boolean_precedence() {
    let fixtures = Fixtures::bundled();
    let simulator = Simulator::start(0, Scenario::Healthy, 50, fixtures)
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let url = format!("{}/sites/Marketing/_api/web/lists", simulator.uri());
    for (filter, titles) in [
        (
            "(BaseTemplate eq 101 or BaseTemplate eq 700) and Hidden eq false",
            vec!["Documents", "Archive"],
        ),
        (
            "BaseTemplate eq 100 or Hidden eq true and BaseTemplate eq 101",
            vec!["Tasks", "Hidden documents"],
        ),
        ("BaseTemplate eq 999", vec![]),
    ] {
        let response: serde_json::Value = http
            .get(&url)
            .header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata")
            .query(&[("$select", "Title"), ("$filter", filter)])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let actual: Vec<_> = response["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["Title"].as_str().unwrap())
            .collect();
        assert_eq!(actual, titles);
    }
    for filter in [
        "BaseTemplate eq",
        "Typo eq 101",
        "Hidden eq 101",
        "BaseTemplate ne 101",
        "RootFolder/ServerRelativeUrl eq '/sites/Marketing/Shared Documents'",
        "(BaseTemplate eq 101",
    ] {
        let response = http
            .get(&url)
            .header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata")
            .query(&[("$filter", filter)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{filter}");
    }
}

#[tokio::test]
async fn simulator_rejects_requests_that_would_hide_client_query_bugs() {
    let simulator = Simulator::start(0, Scenario::Healthy, 50, Fixtures::bundled())
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let url = format!("{}/sites/Marketing/_api/web/lists", simulator.uri());
    let response = http
        .get(&url)
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=verbose")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 406);
    for params in [
        vec![("$selct", "Id")],
        vec![("$select", "Id"), ("$select", "Title")],
        vec![("$expand", "RootFolder"), ("$select", "")],
    ] {
        let response = http
            .get(&url)
            .header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata")
            .query(&params)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }
    let response = http
        .get(format!("{}/_api/search/query", simulator.uri()))
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[
            ("querytext", "'contentclass:STS_Site'"),
            ("selectproperties", "'Typo'"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let response = http
        .get(format!(
            "{}/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/hello.txt')/$value",
            simulator.uri()
        ))
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[("$selct", "Name")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn simulator_pages_collections_without_losing_query_projection() {
    let simulator = Simulator::start(0, Scenario::Healthy, 1, Fixtures::bundled())
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let first = http
        .get(format!(
            "{}/sites/Marketing/_api/web/lists",
            simulator.uri()
        ))
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[
            ("$select", "Title"),
            ("$filter", "Hidden eq false"),
            ("$top", "2"),
        ])
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(first["value"], serde_json::json!([{"Title": "Documents"}]));
    let next = first["odata.nextLink"]
        .as_str()
        .expect("collection continuation");
    assert!(next.starts_with(&simulator.uri()));
    let second = http
        .get(next)
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(second["value"], serde_json::json!([{"Title": "Archive"}]));
    assert!(second.get("odata.nextLink").is_none());
}

#[tokio::test]
async fn simulator_can_deny_one_download_while_metadata_and_other_files_work() {
    let mut data: serde_json::Value =
        serde_json::from_str(include_str!("../examples/simulator/fixtures.json")).unwrap();
    data["faults"] = serde_json::json!([{"resource":"/sites/Marketing/Shared Documents/Plans", "recursive":true, "operation":"download", "status":403, "message":"Download blocked for this resource."}]);
    let simulator = Simulator::start(
        0,
        Scenario::Healthy,
        1,
        Fixtures::parse(&data.to_string()).unwrap(),
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let file = "/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/Plans/Q4%20plan.txt')";
    for (path, status) in [
        ("/_api/web/currentuser".to_string(), 200),
        (file.to_string(), 200),
        (format!("{file}/$value"), 403),
        ("/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Shared%20Documents/hello.txt')/$value".to_string(), 200),
    ] {
        let response = http.get(format!("{}{path}", simulator.uri())).header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata").send().await.unwrap();
        assert_eq!(response.status(), status, "{path}");
    }
}

#[tokio::test]
async fn simulator_replays_response_contracts_without_falling_back_to_generated_data() {
    let fixtures = Fixtures::parse(r#"{
      "recording": {
        "host": "simulator.sharepoint.com", "source": "synthetic test of the replay mechanism; not a tenant capture",
        "exchanges": [
          {"path":"/sites/Recorded/_api/web/lists", "query":{"$select":"Title"}, "status":200,
           "body":{"value":[{"Title":"Recorded first"}], "odata.nextLink":"{{endpoint}}/recorded-page-two"}},
          {"path":"/recorded-page-two", "status":200, "body":{"value":[{"Title":"Recorded second"}]}}
        ]
      }
    }"#).unwrap();
    let simulator = Simulator::start(0, Scenario::Healthy, 1, fixtures)
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let response = http
        .get(format!("{}/sites/Recorded/_api/web/lists", simulator.uri()))
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .query(&[("$select", "Title")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let first: serde_json::Value = response.json().await.unwrap();
    assert_eq!(first["value"][0]["Title"], "Recorded first");
    let next = first["odata.nextLink"].as_str().unwrap();
    assert!(next.starts_with(&simulator.uri()));
    let second: serde_json::Value = http
        .get(next)
        .header("Cookie", COOKIE)
        .header("Accept", "application/json;odata=nometadata")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(second["value"][0]["Title"], "Recorded second");
    for path in [
        "/sites/Recorded/_api/web/lists?$select=Typo",
        "/_api/web/currentuser",
    ] {
        let response = http
            .get(format!("{}{path}", simulator.uri()))
            .header("Cookie", COOKIE)
            .header("Accept", "application/json;odata=nometadata")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
