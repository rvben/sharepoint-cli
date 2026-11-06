//! Smoke checks shared by the example and CI, against a caller-selected CLI.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

use super::server::{COOKIE, Fixtures, HOST, Result, Scenario, Simulator};

pub async fn run(cli: &Path) -> Result<Vec<String>> {
    let cli = cli.canonicalize().map_err(|e| {
        format!(
            "CLI binary {}: {e}; build it first with `cargo build --bin sharepoint`",
            cli.display()
        )
    })?;
    let simulator = Simulator::start(0, Scenario::Healthy, 1, Fixtures::bundled()).await?;
    let endpoint = simulator.uri();
    let binary = cli.clone();
    let mut checks = tokio::task::spawn_blocking(move || happy_path(&binary, &endpoint)).await??;
    for (scenario, exit, kind, requests) in [
        (Scenario::Expired, 3, "auth", 1),
        (Scenario::Forbidden, 3, "auth", 1),
        (Scenario::Redirect, 3, "auth", 1),
        (Scenario::Throttled, 6, "rate_limit", 4),
        (Scenario::Unavailable, 5, "api_error", 4),
        (Scenario::ThrottleOnce, 0, "", 2),
        (Scenario::Transient, 0, "", 2),
    ] {
        let simulator = Simulator::start(0, scenario, 1, Fixtures::bundled()).await?;
        let endpoint = simulator.uri();
        let binary = cli.clone();
        tokio::task::spawn_blocking(move || {
            let runner = Runner::new(&binary, &endpoint)?;
            let output = runner.output(&["sites", "list"], exit)?;
            if exit == 0 {
                let value: Value = serde_json::from_slice(&output.stdout)?;
                ensure(
                    value["items"][0]["name"] == "Marketing",
                    "retry did not return the expected site",
                )?;
            } else {
                let value: Value = serde_json::from_slice(&output.stderr)?;
                ensure(
                    value["error"]["kind"] == kind,
                    "unexpected structured error kind",
                )?;
                ensure(
                    value["error"]["exit_code"] == exit,
                    "unexpected structured exit code",
                )?;
                if exit == 3 {
                    ensure(
                        value["error"]["message"]
                            .as_str()
                            .is_some_and(|m| m.contains("sharepoint auth login")),
                        "missing session recovery hint",
                    )?;
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await??;
        ensure(
            simulator.requests() == requests,
            &format!(
                "{scenario:?}: expected {requests} requests, received {}",
                simulator.requests()
            ),
        )?;
        checks.push(format!("{scenario:?}: exit {exit}, {requests} request(s)"));
    }
    let contracts = Fixtures::parse(include_str!("contracts/pnpcore.json"))?;
    let simulator = Simulator::start(0, Scenario::Healthy, 1, contracts).await?;
    let endpoint = simulator.uri();
    checks.extend(tokio::task::spawn_blocking(move || recorded_responses(&cli, &endpoint)).await??);
    Ok(checks)
}

fn recorded_responses(cli: &Path, endpoint: &str) -> Result<Vec<String>> {
    let runner = Runner::new(cli, endpoint)?;
    let mut checks = Vec::new();
    let identity = runner.json(&["auth", "status"])?;
    ensure(
        identity["verified"] == true && identity["identity"]["email"] == "recorded@example.test",
        "recorded identity was not parsed",
    )?;
    checks.push("PnP recordings: identity".into());

    let library = format!("https://{HOST}/sites/Marketing/Documents");
    let file = runner.json(&["files", "stat", &format!("{library}/hello.txt")])?;
    ensure(
        file["size"] == 19824
            && file["path"] == "/hello.txt"
            && file["created"] == "2024-03-29T17:56:36Z"
            && file["etag"] == "\"{00000000-0000-4000-8000-000000000003},1\"",
        "recorded file length, timestamp, or ETag was not preserved",
    )?;
    checks.push("PnP recordings: file metadata and string Length".into());

    let folder = runner.json(&["files", "stat", &format!("{library}/Plans")])?;
    ensure(
        folder["kind"] == "folder"
            && folder["path"] == "/Plans"
            && folder["created"] == "2021-09-11T23:42:33Z",
        "recorded folder fallback or timestamp differs",
    )?;
    checks.push("PnP recordings: folder metadata after file 404".into());

    let first = runner.json(&["files", "find", &library, "--query", "*"])?;
    let next = cursor(&first)?;
    let second = runner.json(&["files", "find", &library, "--query", "*", "--page", &next])?;
    ensure(
        first["total"] == 10
            && second["total"] == 10
            && first["items"][0]["path"] == "/result-01.txt"
            && second["items"][0]["path"] == "/result-11.txt"
            && second["items"][9]["path"] == "/result-20.txt"
            && second["next"].is_string(),
        "recorded search pages, null cells, or filename fallback differ",
    )?;
    checks.push("PnP recordings: search pages with typed and null cells".into());

    let output = runner.output(&["files", "stat", &format!("{library}/missing.txt")], 4)?;
    let error: Value = serde_json::from_slice(&output.stderr)?;
    ensure(
        error["error"]["kind"] == "not_found"
            && error["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("File Not Found.")),
        "recorded OData error was not surfaced",
    )?;
    checks.push("PnP recordings: OData not-found error".into());
    Ok(checks)
}

fn happy_path(cli: &Path, endpoint: &str) -> Result<Vec<String>> {
    let runner = Runner::new(cli, endpoint)?;
    let mut checks = Vec::new();
    let auth = runner.json(&["auth", "status"])?;
    ensure(
        auth["verified"] == true && auth["identity"]["email"] == "simulator@example.test",
        "identity verification failed",
    )?;
    ensure(
        runner.json(&["doctor"])?["healthy"] == true,
        "doctor reports unhealthy",
    )?;
    checks.push("identity and doctor".into());

    let first = runner.json(&["sites", "list"])?;
    let next = cursor(&first)?;
    ensure(
        first["items"][0]["name"] == "Marketing",
        "first site page differs",
    )?;
    let second = runner.json(&["sites", "list", "--page", &next])?;
    ensure(
        second["items"][0]["name"] == "Engineering" && second["next"].is_null(),
        "second site page differs",
    )?;
    let sites = runner.json(&["sites", "list", "--all"])?;
    ensure(sites["total"] == 2, "--all did not follow site pages")?;
    let filtered = runner.json(&["sites", "list", "--query", "Engineering"])?;
    ensure(
        filtered["total"] == 1 && filtered["items"][0]["name"] == "Engineering",
        "site filtering differs",
    )?;
    checks.push("site discovery, filtering, and pagination".into());

    let site = format!("https://{HOST}/sites/Marketing");
    let library = format!("{site}/Documents");
    let drives = runner.json(&["drives", "list", &site])?;
    ensure(
        drives["total"] == 2 && drives["items"][0]["drive_type"] == "documentLibrary",
        "document libraries differ",
    )?;
    let personal = runner
        .command()
        .env("SHAREPOINT_HOST", "simulator-my.sharepoint.com")
        .args([
            "drives",
            "list",
            "https://simulator-my.sharepoint.com/personal/simulator",
        ])
        .output()?;
    ensure(personal.status.success(), "OneDrive library lookup failed")?;
    let personal: Value = serde_json::from_slice(&personal.stdout)?;
    ensure(
        personal["items"][0]["drive_type"] == "business",
        "OneDrive template is not a business drive",
    )?;
    checks.push("SharePoint and OneDrive libraries".into());

    let first = runner.json(&["files", "ls", &library, "--limit", "2"])?;
    let next = cursor(&first)?;
    let second = runner.json(&["files", "ls", &library, "--page", &next])?;
    ensure(
        first["total"] == 2 && second["total"] == 2,
        "folder cursor lost or duplicated items",
    )?;
    let all = runner.json(&["files", "ls", &library, "--all"])?;
    ensure(
        all["total"] == 4
            && !all["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["name"] == "Forms"),
        "root folder listing differs",
    )?;
    let tree = runner.json(&["files", "ls", &library, "--recursive"])?;
    ensure(
        tree["total"] == 5
            && tree["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["path"] == "/Plans/Q4 plan.txt"),
        "recursive traversal differs",
    )?;
    ensure(
        runner.json(&["files", "ls", &format!("{library}/Empty")])?["total"] == 0,
        "empty folder is not empty",
    )?;
    ensure(
        runner.json(&["files", "stat", &format!("{library}/Plans")])?["kind"] == "folder",
        "folder metadata fallback failed",
    )?;
    checks.push("folder listing, cursors, empty folders, and recursion".into());

    let first = runner.json(&["files", "find", &library, "--query", "txt"])?;
    let next = cursor(&first)?;
    let second = runner.json(&["files", "find", &library, "--query", "txt", "--page", &next])?;
    ensure(
        first["items"][0]["path"] != second["items"][0]["path"],
        "search cursor repeated the first page",
    )?;
    ensure(
        runner.json(&["files", "find", &library, "--query", "txt", "--all"])?["total"] == 3,
        "search pagination or library scope differs",
    )?;
    let filtered = runner.json(&["files", "find", &library, "--query", "O'Brien"])?;
    ensure(
        filtered["total"] == 1 && filtered["items"][0]["name"] == "O'Brien #1 %.txt",
        "quoted search query differs",
    )?;
    let plans = runner.json(&[
        "files",
        "find",
        &format!("{library}/Plans"),
        "--query",
        "txt",
    ])?;
    ensure(
        plans["total"] == 1 && plans["items"][0]["path"] == "/Plans/Q4 plan.txt",
        "folder search is not scoped to the folder",
    )?;
    checks.push("library search, scope, and pagination".into());

    let file = format!("{library}/O%27Brien%20%231%20%25.txt");
    let bytes = "Unicode download: café 🧪\n".as_bytes();
    let meta = runner.json(&["files", "stat", &file])?;
    ensure(
        meta["size"] == bytes.len() && meta["path"] == "/O'Brien #1 %.txt",
        "escaped filename metadata differs",
    )?;
    ensure(
        runner
            .output(&["files", "download", &file, "--path", "-"], 0)?
            .stdout
            == bytes,
        "downloaded stdout bytes differ",
    )?;
    let destination = runner.root.path().join("download with spaces.txt");
    runner.output(
        &[
            "files",
            "download",
            &file,
            "--path",
            destination.to_str().ok_or("non-UTF-8 temp directory")?,
        ],
        0,
    )?;
    ensure(
        std::fs::read(&destination)? == bytes,
        "downloaded file bytes differ",
    )?;
    checks.push("escaped filenames, metadata, and exact download bytes".into());

    let output = runner.output(&["files", "stat", &format!("{library}/missing.txt")], 4)?;
    let error: Value = serde_json::from_slice(&output.stderr)?;
    ensure(
        error["error"]["kind"] == "not_found",
        "missing file error differs",
    )?;
    let output = runner
        .command()
        .env_remove("SHAREPOINT_COOKIE")
        .args(["sites", "list"])
        .output()?;
    ensure(
        output.status.code() == Some(3),
        "missing session did not exit 3",
    )?;
    checks.push("missing files and missing sessions".into());
    let locked = format!("{site}/Archive/locked.txt");
    ensure(
        runner.json(&["files", "stat", &locked])?["kind"] == "file",
        "blocked download metadata is unavailable",
    )?;
    let blocked = runner.output(&["files", "download", &locked, "--path", "-"], 3)?;
    let error: Value = serde_json::from_slice(&blocked.stderr)?;
    ensure(
        error["error"]["message"]
            .as_str()
            .is_some_and(|s| s.contains("Download blocked for this resource")),
        "resource failure message differs",
    )?;
    ensure(
        runner
            .output(
                &[
                    "files",
                    "download",
                    &format!("{site}/Archive/old.txt"),
                    "--path",
                    "-",
                ],
                0,
            )?
            .stdout
            == b"Archived fixture.\n",
        "resource failure blocked an unrelated download",
    )?;
    checks.push("resource-specific download failure with other access preserved".into());
    Ok(checks)
}

struct Runner {
    cli: PathBuf,
    endpoint: String,
    root: tempfile::TempDir,
}

impl Runner {
    fn new(cli: &Path, endpoint: &str) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let runner = Self {
            cli: cli.into(),
            endpoint: endpoint.into(),
            root,
        };
        // Ask the selected binary instead of assuming it honors XDG overrides.
        let locations = runner.json(&["config", "show"])?;
        for key in ["config_path", "cache_path", "session_path"] {
            let path = Path::new(
                locations[key]
                    .as_str()
                    .ok_or("CLI did not report its config/cache/session paths")?,
            );
            if !path.starts_with(runner.root.path())
                || path
                    .components()
                    .any(|c| c == std::path::Component::ParentDir)
            {
                return Err(format!("cannot isolate CLI {key}: {}; use a build that honors XDG_CONFIG_HOME/XDG_CACHE_HOME", path.display()).into());
            }
        }
        let config = Path::new(locations["config_path"].as_str().unwrap());
        std::fs::create_dir_all(config.parent().ok_or("config path has no parent")?)?;
        std::fs::write(
            config,
            format!(
                "[profile.default]\nbackend = \"rest\"\nsharepoint_host = \"{HOST}\"\nread_only = true\n"
            ),
        )?;
        Ok(runner)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.cli);
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name.starts_with("SHAREPOINT_") || name.starts_with("MICROSOFT_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_CACHE_HOME", self.root.path().join("cache"))
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env("SHAREPOINT_BACKEND", "rest")
            .env("SHAREPOINT_HOST", HOST)
            .env("SHAREPOINT_REST_ENDPOINT", &self.endpoint)
            .env("SHAREPOINT_COOKIE", COOKIE)
            .timeout(Duration::from_secs(30))
            .args(["--output", "json", "--no-color"]);
        command
    }

    fn output(&self, args: &[&str], expected_exit: i32) -> Result<Output> {
        let output = self.command().args(args).output()?;
        ensure(
            output.status.code() == Some(expected_exit),
            &format!(
                "{}: expected exit {expected_exit}, got {}\n{}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
        Ok(output)
    }

    fn json(&self, args: &[&str]) -> Result<Value> {
        Ok(serde_json::from_slice(&self.output(args, 0)?.stdout)?)
    }
}

fn cursor(value: &Value) -> Result<String> {
    value["next"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "expected a continuation cursor".into())
}

fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
