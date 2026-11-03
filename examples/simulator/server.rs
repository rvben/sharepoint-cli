//! Local, read-only subset of the SharePoint REST API used by the CLI.

#[path = "odata.rs"]
mod odata;
#[path = "recording.rs"]
mod recording;

use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const HOST: &str = "simulator.sharepoint.com";
pub const COOKIE: &str = "FedAuth=simulator; rtFa=simulator";
const TIMESTAMP: &str = "2026-01-01T00:00:00Z";
const FOLDER_COLUMNS: &[&str] = &[
    "UniqueId",
    "Name",
    "ServerRelativeUrl",
    "TimeCreated",
    "TimeLastModified",
];
const FILE_COLUMNS: &[&str] = &[
    "UniqueId",
    "Name",
    "ServerRelativeUrl",
    "Length",
    "ETag",
    "TimeCreated",
    "TimeLastModified",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Scenario {
    #[default]
    Healthy,
    Expired,
    Forbidden,
    Redirect,
    Throttled,
    ThrottleOnce,
    Transient,
    Unavailable,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixtures {
    #[serde(default)]
    sites: Vec<Site>,
    #[serde(default)]
    faults: Vec<ResourceFault>,
    recording: Option<recording::Recording>,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Metadata,
    List,
    Download,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceFault {
    resource: String,
    status: u16,
    message: String,
    #[serde(default)]
    recursive: bool,
    operation: Option<Operation>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Site {
    id: String,
    name: String,
    host: String,
    path: String,
    libraries: Vec<Library>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    id: String,
    name: String,
    root: String,
    template: u32,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    folders: Vec<String>,
    #[serde(default)]
    files: Vec<File>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    content: String,
}

impl Fixtures {
    pub fn parse(text: &str) -> Result<Self> {
        let fixtures: Self = serde_json::from_str(text)?;
        if let Some(recording) = &fixtures.recording {
            recording.validate()?;
            if !fixtures.sites.is_empty() || !fixtures.faults.is_empty() {
                return Err(
                    "recordings cannot be mixed with generated sites or resource faults".into(),
                );
            }
        } else if fixtures.sites.is_empty() {
            return Err("fixtures must contain at least one site".into());
        }
        for fault in &fixtures.faults {
            if !(400..=599).contains(&fault.status)
                || !fault.resource.starts_with('/')
                || !valid_relative(&fault.resource[1..])
            {
                return Err(
                    "resource faults need an absolute resource path and a 4xx/5xx status".into(),
                );
            }
        }
        let mut sites = BTreeSet::new();
        for site in &fixtures.sites {
            if site.name.is_empty()
                || !valid_host(&site.host)
                || !site.path.starts_with('/')
                || !valid_relative(&site.path[1..])
                || !sites.insert(site.path.to_ascii_lowercase())
            {
                return Err(format!("invalid or duplicate site: {}", site.path).into());
            }
            let mut roots = BTreeSet::new();
            for library in &site.libraries {
                if library.name.is_empty()
                    || !valid_relative(&library.root)
                    || library.template == 0
                    || !roots.insert(library.root.to_ascii_lowercase())
                {
                    return Err(format!("invalid or duplicate library: {}", library.name).into());
                }
                let mut files = BTreeSet::new();
                for file in &library.files {
                    if !valid_relative(&file.path) || !files.insert(file.path.to_ascii_lowercase())
                    {
                        return Err(format!("invalid or duplicate file: {}", file.path).into());
                    }
                }
                if library.folders.iter().any(|p| !valid_relative(p))
                    || library
                        .folders()
                        .iter()
                        .any(|p| files.contains(&p.to_ascii_lowercase()))
                {
                    return Err(format!("invalid or conflicting folder in {}", library.name).into());
                }
            }
        }
        Ok(fixtures)
    }

    pub fn bundled() -> Self {
        Self::parse(include_str!("fixtures.json")).expect("valid bundled fixtures")
    }

    pub fn host(&self) -> &str {
        self.recording
            .as_ref()
            .map_or_else(|| self.sites[0].host.as_str(), |r| r.host.as_str())
    }
}

fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0', '\n', '\r'])
        && path.split('/').all(|p| !matches!(p, "" | "." | ".."))
}

fn valid_host(host: &str) -> bool {
    host.ends_with(".sharepoint.com")
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

impl Site {
    fn url(&self) -> String {
        format!("https://{}{}", self.host, self.path)
    }
}

impl Library {
    fn root_path(&self, site: &Site) -> String {
        format!("{}/{}", site.path, self.root)
    }

    // Infer intermediate folders, allowing fixtures to specify just file paths.
    fn folders(&self) -> BTreeSet<String> {
        let mut folders = BTreeSet::new();
        for path in self
            .folders
            .iter()
            .chain(self.files.iter().map(|f| &f.path))
        {
            let mut parent = path.as_str();
            if self.folders.contains(path) {
                folders.insert(parent.to_string());
            }
            while let Some((head, _)) = parent.rsplit_once('/') {
                folders.insert(head.to_string());
                parent = head;
            }
        }
        folders
    }
}

pub struct Simulator {
    server: MockServer,
    requests: Arc<AtomicUsize>,
}

impl Simulator {
    pub async fn start(
        port: u16,
        scenario: Scenario,
        page_size: usize,
        fixtures: Fixtures,
    ) -> Result<Self> {
        if page_size == 0 {
            return Err("page size must be greater than zero".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let server = MockServer::builder()
            .listener(listener)
            .disable_request_recording()
            .start()
            .await;
        let requests = Arc::new(AtomicUsize::new(0));
        Mock::given(|_: &Request| true)
            .respond_with(Api {
                endpoint: server.uri(),
                fixtures,
                scenario,
                page_size,
                requests: requests.clone(),
            })
            .mount(&server)
            .await;
        Ok(Self { server, requests })
    }

    pub fn uri(&self) -> String {
        self.server.uri()
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
}

struct Api {
    endpoint: String,
    fixtures: Fixtures,
    scenario: Scenario,
    page_size: usize,
    requests: Arc<AtomicUsize>,
}

impl Respond for Api {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if request.method != "GET" {
            return error(405, "The simulator supports GET only.");
        }
        if request.url.path() == "/_simulator/health" {
            return ok(
                json!({"simulator": true, "scenario": format!("{:?}", self.scenario), "recording": self.fixtures.recording.is_some()}),
            );
        }
        if request.headers.get("cookie").and_then(|v| v.to_str().ok()) != Some(COOKIE) {
            return error(401, "Use the simulator's dummy session cookie.");
        }
        let accept = request
            .headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase()
            .replace(' ', "");
        if accept != "application/json;odata=nometadata" {
            return error(
                406,
                "The simulator supports Accept: application/json;odata=nometadata only.",
            );
        }
        let index = self.requests.fetch_add(1, Ordering::Relaxed);
        match self.scenario {
            Scenario::Expired => return error(401, "The simulated browser session has expired."),
            Scenario::Forbidden => return error(403, "Access denied by the simulator."),
            Scenario::Redirect => {
                return ResponseTemplate::new(302).insert_header("Location", "/_simulator/signin");
            }
            Scenario::Throttled => return retry(429),
            Scenario::ThrottleOnce if index == 0 => return retry(429),
            Scenario::Transient if index == 0 => return retry(503),
            Scenario::Unavailable => return retry(503),
            _ => {}
        }
        self.route(request)
            .unwrap_or_else(|message| error(400, &message))
    }
}

impl Api {
    fn route(&self, request: &Request) -> std::result::Result<ResponseTemplate, String> {
        if let Some(recording) = &self.fixtures.recording {
            return Ok(recording.respond(request, &self.endpoint));
        }
        // Decode exactly once. OData string quotes are unescaped separately below.
        let path = decode_path(request.url.path())?;
        if path == "/_api/web/currentuser" {
            return entity(
                request,
                json!({"Title": "Simulator User", "Email": "simulator@example.test", "LoginName": "simulator@example.test"}),
                &["Title", "Email", "LoginName"],
            );
        }
        if path == "/_api/search/query" {
            return self.search(request);
        }
        let Some((site_path, api)) = path.split_once("/_api/web") else {
            return Ok(error(404, "Unsupported simulator endpoint."));
        };
        let Some(site) = self
            .fixtures
            .sites
            .iter()
            .find(|s| s.path.eq_ignore_ascii_case(site_path))
        else {
            return Ok(error(404, "Site not found."));
        };
        if api.is_empty() {
            if let Some(response) = self.resource_fault(&site.path, Operation::Metadata) {
                return Ok(response);
            }
            return entity(
                request,
                json!({"Id": site.id, "Title": site.name, "Url": site.url()}),
                &["Id", "Title", "Url"],
            );
        }
        if api == "/lists" {
            if let Some(response) = self.resource_fault(&site.path, Operation::List) {
                return Ok(response);
            }
            return self.collection(
                request,
                site.libraries
                    .iter()
                    .map(|l| {
                        json!({
                        "Id": l.id, "Title": l.name, "BaseTemplate": l.template, "Hidden": l.hidden,
                                "RootFolder": {"ServerRelativeUrl": l.root_path(site)}
                            })
                    })
                    .collect(),
                &[
                    "Id",
                    "Title",
                    "BaseTemplate",
                    "Hidden",
                    "RootFolder/ServerRelativeUrl",
                ],
                &["RootFolder"],
            );
        }
        for (operation, is_file) in [
            ("GetFileByServerRelativePath", true),
            ("GetFolderByServerRelativePath", false),
        ] {
            let Some(rest) = api.strip_prefix(&format!("/{operation}(decodedurl='")) else {
                continue;
            };
            let Some((literal, suffix)) = rest.rsplit_once("')") else {
                return Err("Invalid OData path literal.".into());
            };
            let server_relative = literal.replace("''", "'");
            // Validate escaping instead of accepting malformed literals.
            if server_relative.replace('\'', "''") != literal {
                return Err("Single quotes in OData paths must be doubled.".into());
            }
            let operation = match suffix {
                "" => Some(Operation::Metadata),
                "/$value" => Some(Operation::Download),
                "/Files" | "/Folders" => Some(Operation::List),
                _ => None,
            };
            if let Some(response) =
                operation.and_then(|op| self.resource_fault(&server_relative, op))
            {
                return Ok(response);
            }
            for library in &site.libraries {
                let root = library.root_path(site);
                let relative = if server_relative == root {
                    ""
                } else {
                    let Some(relative) = server_relative.strip_prefix(&format!("{root}/")) else {
                        continue;
                    };
                    relative
                };
                if is_file {
                    if let Some(file) = library.files.iter().find(|f| f.path == relative) {
                        return Ok(match suffix {
                            "" => entity(request, file_json(&root, file), FILE_COLUMNS)?,
                            "/$value" => {
                                odata::Query::parse(&request.url, &[])?;
                                ResponseTemplate::new(200)
                                    .set_body_bytes(file.content.as_bytes())
                                    .insert_header("Content-Type", "application/octet-stream")
                            }
                            _ => error(404, "Unsupported file operation."),
                        });
                    }
                } else {
                    let folders = library.folders();
                    if relative.is_empty() || folders.contains(relative) {
                        return Ok(match suffix {
                            "" => entity(request, folder_json(&root, relative), FOLDER_COLUMNS)?,
                            "/Folders" => self.collection(
                                request,
                                folders
                                    .iter()
                                    .filter(|p| parent(p) == relative)
                                    .map(|p| folder_json(&root, p))
                                    .collect(),
                                FOLDER_COLUMNS,
                                &[],
                            )?,
                            "/Files" => self.collection(
                                request,
                                library
                                    .files
                                    .iter()
                                    .filter(|f| parent(&f.path) == relative)
                                    .map(|f| file_json(&root, f))
                                    .collect(),
                                FILE_COLUMNS,
                                &[],
                            )?,
                            _ => error(404, "Unsupported folder operation."),
                        });
                    }
                }
            }
            return Ok(error(404, "File or folder not found."));
        }
        Ok(error(404, "Unsupported simulator endpoint."))
    }

    fn search(&self, request: &Request) -> std::result::Result<ResponseTemplate, String> {
        odata::Query::parse(
            &request.url,
            &["querytext", "selectproperties", "rowlimit", "startrow"],
        )?;
        let params: std::collections::HashMap<_, _> =
            request.url.query_pairs().into_owned().collect();
        let query = params.get("querytext").ok_or("Missing querytext.")?;
        let query = query
            .strip_prefix('\'')
            .and_then(|q| q.strip_suffix('\''))
            .ok_or("querytext must be quoted.")?
            .replace("''", "'");
        let mut rows = Vec::new();
        let selected = params
            .get("selectproperties")
            .map(|s| {
                let s = s
                    .strip_prefix('\'')
                    .and_then(|s| s.strip_suffix('\''))
                    .ok_or("selectproperties must be quoted.")?;
                let columns: Vec<_> = s.split(',').collect();
                for column in &columns {
                    if ![
                        "Title",
                        "Path",
                        "WebId",
                        "FileName",
                        "Size",
                        "UniqueId",
                        "LastModifiedTime",
                    ]
                    .contains(column)
                    {
                        return Err(format!("Unsupported search property: {column}"));
                    }
                }
                Ok::<_, String>(columns)
            })
            .transpose()?;
        if let Some(text) = query.strip_suffix("contentclass:STS_Site") {
            let text = text
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_lowercase();
            for site in &self.fixtures.sites {
                if !site.path.starts_with("/personal/") && matches_text(&site.name, &text) {
                    rows.push(cells(&[
                        ("Title", site.name.clone()),
                        ("Path", site.url()),
                        ("WebId", site.id.clone()),
                    ]));
                }
            }
        } else if let Some((text, scope)) = query.split_once(") path:\"") {
            let scope = scope
                .strip_suffix("\" IsDocument:1")
                .ok_or("Unsupported search scope.")?;
            let text = text
                .strip_prefix('(')
                .ok_or("Unsupported query.")?
                .to_lowercase();
            for site in &self.fixtures.sites {
                for library in &site.libraries {
                    let root = library.root_path(site);
                    if format!("https://{}{root}", site.host) != scope {
                        continue;
                    }
                    for file in &library.files {
                        if matches_text(&file.path, &text) || matches_text(&file.content, &text) {
                            rows.push(cells(&[
                                ("FileName", name(&file.path).into()),
                                ("Title", name(&file.path).into()),
                                ("Path", format!("https://{}{root}/{}", site.host, file.path)),
                                ("Size", file.content.len().to_string()),
                                ("UniqueId", format!("{root}/{}", file.path)),
                                ("LastModifiedTime", TIMESTAMP.into()),
                            ]));
                        }
                    }
                }
            }
        } else {
            return Err("Only CLI site and library search queries are supported.".into());
        }
        let start = number(&params, "startrow", 0)?;
        let limit = number(&params, "rowlimit", 50)?.min(self.page_size);
        if limit == 0 {
            return Err("rowlimit must be positive.".into());
        }
        let total = rows.len();
        let page: Vec<_> = rows
            .into_iter()
            .skip(start)
            .take(limit)
            .map(|mut row| {
                if let Some(selected) = &selected {
                    row["Cells"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|cell| selected.contains(&cell["Key"].as_str().unwrap()));
                }
                row
            })
            .collect();
        Ok(ok(json!({"PrimaryQueryResult": {"RelevantResults": {
            "TotalRows": total, "RowCount": page.len(), "Table": {"Rows": page}
        }}})))
    }
}

fn number(
    params: &std::collections::HashMap<String, String>,
    key: &str,
    default: usize,
) -> std::result::Result<usize, String> {
    params.get(key).map_or(Ok(default), |s| {
        s.parse()
            .map_err(|_| format!("{key} must be a nonnegative integer."))
    })
}

fn matches_text(value: &str, query: &str) -> bool {
    query == "*" || value.to_lowercase().contains(query)
}

fn cells(values: &[(&str, String)]) -> Value {
    json!({"Cells": values.iter().map(|(k, v)| json!({"Key": k, "Value": v})).collect::<Vec<_>>()})
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}
fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn folder_json(root: &str, relative: &str) -> Value {
    let path = if relative.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{relative}")
    };
    json!({"UniqueId": path, "Name": name(&path), "ServerRelativeUrl": path, "TimeCreated": TIMESTAMP, "TimeLastModified": TIMESTAMP})
}

fn file_json(root: &str, file: &File) -> Value {
    json!({"UniqueId": format!("{root}/{}", file.path), "Name": name(&file.path), "ServerRelativeUrl": format!("{root}/{}", file.path), "Length": file.content.len().to_string(), "ETag": "\"simulator,1\"", "TimeCreated": TIMESTAMP, "TimeLastModified": TIMESTAMP})
}

fn decode_path(path: &str) -> std::result::Result<String, String> {
    let mut bytes = Vec::new();
    let mut chars = path.bytes();
    while let Some(b) = chars.next() {
        bytes.push(if b == b'%' {
            let hex = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
            let hi = chars
                .next()
                .and_then(hex)
                .ok_or("Invalid percent escape.")?;
            let lo = chars
                .next()
                .and_then(hex)
                .ok_or("Invalid percent escape.")?;
            hi * 16 + lo
        } else {
            b
        });
    }
    String::from_utf8(bytes).map_err(|_| "Paths must be UTF-8.".into())
}

fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn entity(
    request: &Request,
    body: Value,
    columns: &[&str],
) -> std::result::Result<ResponseTemplate, String> {
    let query = odata::Query::parse(&request.url, &["$select"])?;
    Ok(ok(query.projection(columns, &[])?.apply(&body)))
}

impl Api {
    fn resource_fault(&self, resource: &str, operation: Operation) -> Option<ResponseTemplate> {
        let resource = resource.to_ascii_lowercase();
        self.fixtures
            .faults
            .iter()
            .find(|fault| {
                let target = fault.resource.to_ascii_lowercase();
                fault.operation.is_none_or(|op| op == operation)
                    && (resource == target
                        || (fault.recursive && resource.starts_with(&format!("{target}/"))))
            })
            .map(|fault| {
                let response = error(fault.status, &fault.message);
                if fault.status == 429 || fault.status >= 500 {
                    response.insert_header("Retry-After", "0")
                } else {
                    response
                }
            })
    }

    fn collection(
        &self,
        request: &Request,
        mut rows: Vec<Value>,
        columns: &[&str],
        expandable: &[&str],
    ) -> std::result::Result<ResponseTemplate, String> {
        let query = odata::Query::parse(
            &request.url,
            &["$select", "$expand", "$filter", "$top", "$skiptoken"],
        )?;
        let projection = query.projection(columns, expandable)?;
        query.filter(&mut rows, columns)?;
        let rows = rows.iter().map(|row| projection.apply(row)).collect();
        Ok(ok(query.page(
            &request.url,
            &self.endpoint,
            rows,
            self.page_size,
        )?))
    }
}
fn error(status: u16, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(
        json!({"odata.error": {"code": "simulator", "message": {"lang": "en", "value": message}}}),
    )
}
fn retry(status: u16) -> ResponseTemplate {
    error(status, "Simulated retryable failure.").insert_header("Retry-After", "0")
}
