use super::isolated_command;
use assert_cmd::Command;
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const FILE_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents/hello.bin";
pub const LIBRARY_REF: &str = "https://contoso.sharepoint.com/sites/Marketing/Documents";

#[derive(Clone, Copy, Debug)]
pub enum Backend {
    Graph,
    Rest,
}

impl Backend {
    pub fn command(self, endpoint: &str, root: &Path) -> Command {
        let mut command = isolated_command(root);
        match self {
            Self::Graph => {
                command
                    .env("SHAREPOINT_BACKEND", "graph")
                    .env("SHAREPOINT_TENANT_ID", "contoso")
                    .env("SHAREPOINT_CLIENT_ID", "client-1")
                    .env("SHAREPOINT_ACCESS_TOKEN", "FAKE")
                    .env("MICROSOFT_GRAPH_ENDPOINT", endpoint);
            }
            Self::Rest => {
                command
                    .env("SHAREPOINT_BACKEND", "rest")
                    .env("SHAREPOINT_HOST", "contoso.sharepoint.com")
                    .env("SHAREPOINT_COOKIE", "FedAuth=F; rtFa=R")
                    .env("SHAREPOINT_REST_ENDPOINT", endpoint);
            }
        }
        command
    }

    pub fn content_path(self) -> &'static str {
        match self {
            Self::Graph => "/drives/D1/root:/hello.bin:/content",
            Self::Rest => {
                "/sites/Marketing/_api/web/GetFileByServerRelativePath(decodedurl='/sites/Marketing/Documents/hello.bin')/$value"
            }
        }
    }

    pub fn resolution(self) -> [(&'static str, serde_json::Value); 2] {
        match self {
            Self::Graph => [
                (
                    "/sites/contoso.sharepoint.com:/sites/Marketing",
                    json!({
                        "id": "S1", "displayName": "Marketing",
                        "webUrl": "https://contoso.sharepoint.com/sites/Marketing"
                    }),
                ),
                (
                    "/sites/S1/drives",
                    json!({"value": [{
                        "id": "D1", "name": "Documents", "driveType": "documentLibrary"
                    }]}),
                ),
            ],
            Self::Rest => [
                (
                    "/sites/Marketing/_api/web",
                    json!({
                        "Id": "S1", "Title": "Marketing",
                        "Url": "https://contoso.sharepoint.com/sites/Marketing"
                    }),
                ),
                (
                    "/sites/Marketing/_api/web/lists",
                    json!({"value": [{
                        "Id": "D1", "Title": "Documents", "BaseTemplate": 101,
                        "RootFolder": {"ServerRelativeUrl": "/sites/Marketing/Documents"}
                    }]}),
                ),
            ],
        }
    }

    pub async fn server(self) -> MockServer {
        let server = MockServer::start().await;
        for (api, body) in self.resolution() {
            Mock::given(method("GET"))
                .and(path(api))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        server
    }
}

pub const BACKENDS: [Backend; 2] = [Backend::Graph, Backend::Rest];
