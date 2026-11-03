//! Interactive browser sign-in for the SharePoint REST backend.
//!
//! Launches a Chromium-family browser with a throwaway profile and the
//! DevTools protocol bound to 127.0.0.1, waits for the user to finish SSO,
//! then reads the `FedAuth`/`rtFa` cookies via `Storage.getCookies`. The
//! cookie values are secrets: never print or log them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::{CliError, Result};

pub(crate) const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const BROWSER_START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(750);
const SESSION_COOKIES: [&str; 2] = ["FedAuth", "rtFa"];
const LOGIN_CANCELLED: &str = "login cancelled: the browser was closed before sign-in completed";

const CANDIDATES: &[&str] = &[
    "microsoft-edge",
    "microsoft-edge-stable",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "brave-browser",
];

#[cfg(target_os = "macos")]
const MAC_APPS: &[&str] = &[
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
];

/// Find the first candidate executable on `path_var` (a `PATH`-style list).
fn find_in_path(path_var: &str, names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|name| {
        std::env::split_paths(path_var)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// Browser to launch: explicit override, else first known browser found.
pub(crate) fn find_browser(override_path: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(PathBuf::from(p));
    }
    let path_var = std::env::var("PATH").unwrap_or_default();
    if let Some(p) = find_in_path(&path_var, CANDIDATES) {
        return Ok(p);
    }
    #[cfg(target_os = "macos")]
    if let Some(p) = MAC_APPS.iter().map(PathBuf::from).find(|p| p.is_file()) {
        return Ok(p);
    }
    Err(CliError::Input(
        "no Chromium-based browser found (Edge, Chrome, Chromium, Brave); \
         set SHAREPOINT_BROWSER to its executable"
            .into(),
    ))
}

fn launch_args(user_data_dir: &Path, start_url: &str) -> Vec<String> {
    vec![
        "--remote-debugging-port=0".into(),
        "--remote-debugging-address=127.0.0.1".into(),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--new-window".into(),
        start_url.into(),
    ]
}

/// Parse `DevToolsActivePort`: line 1 is the port, line 2 the browser ws path.
fn parse_devtools_active_port(text: &str) -> Option<(u16, String)> {
    let mut lines = text.lines();
    let port = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    path.starts_with('/').then(|| (port, path.to_string()))
}

fn cookie_domain_matches(domain: &str, host: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    h == d || h.ends_with(&format!(".{d}"))
}

/// `FedAuth=…; rtFa=…` when both session cookies for `host` are present.
fn session_cookie_header(cookies: &[Value], host: &str) -> Option<String> {
    let parts: Vec<String> = SESSION_COOKIES
        .iter()
        .map(|name| {
            cookies
                .iter()
                .find(|c| {
                    c["name"].as_str() == Some(name)
                        && c["domain"]
                            .as_str()
                            .is_some_and(|d| cookie_domain_matches(d, host))
                })
                .and_then(|c| c["value"].as_str())
                .filter(|v| !v.is_empty())
                .map(|v| format!("{name}={v}"))
        })
        .collect::<Option<_>>()?;
    Some(parts.join("; "))
}

/// Map a DevTools websocket error. Closing the browser window tears the
/// socket down, often without a close handshake; that is the user cancelling
/// the login, not a protocol failure.
fn ws_error(context: &str, err: tungstenite::Error) -> CliError {
    match err {
        tungstenite::Error::ConnectionClosed
        | tungstenite::Error::AlreadyClosed
        | tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ) => CliError::Auth(LOGIN_CANCELLED.into()),
        tungstenite::Error::Io(ref io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            ) =>
        {
            CliError::Auth(LOGIN_CANCELLED.into())
        }
        other => CliError::Auth(format!("browser DevTools {context}: {other}")),
    }
}

struct Cdp {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    async fn connect(port: u16, path: &str) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}{path}"))
            .await
            .map_err(|e| CliError::Auth(format!("connect to browser DevTools: {e}")))?;
        Ok(Self { ws, next_id: 1 })
    }

    /// Send one command and wait for its response, skipping events.
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"id": id, "method": method, "params": params}).to_string();
        self.ws
            .send(Message::Text(msg.into()))
            .await
            .map_err(|e| ws_error("send", e))?;
        while let Some(frame) = self.ws.next().await {
            let frame = frame.map_err(|e| ws_error("read", e))?;
            let Message::Text(text) = frame else {
                continue;
            };
            let v: Value = serde_json::from_str(&text)?;
            if v["id"].as_u64() == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(CliError::Auth(format!("browser DevTools {method}: {err}")));
                }
                return Ok(v["result"].clone());
            }
        }
        Err(CliError::Auth(LOGIN_CANCELLED.into()))
    }
}

async fn wait_for_devtools(
    user_data_dir: &Path,
    child: &mut tokio::process::Child,
) -> Result<(u16, String)> {
    let file = user_data_dir.join("DevToolsActivePort");
    let deadline = tokio::time::Instant::now() + BROWSER_START_TIMEOUT;
    loop {
        if let Ok(text) = tokio::fs::read_to_string(&file).await
            && let Some(found) = parse_devtools_active_port(&text)
        {
            return Ok(found);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(CliError::Auth(format!(
                "browser exited ({status}) before its DevTools endpoint started"
            )));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CliError::Auth(
                "browser did not start its DevTools endpoint in time; a snap-packaged \
                 Chromium cannot share its private /tmp, so set SHAREPOINT_BROWSER to a \
                 non-snap Chrome, Chromium or Edge"
                    .into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Result of checking one candidate cookie header.
pub(crate) enum Probe {
    Accepted,
    /// Not (yet) accepted; the reason must not contain cookie values.
    Rejected(String),
}

fn timeout_error(last_rejection: Option<&str>) -> CliError {
    match last_rejection {
        Some(reason) => {
            // The user is already running `auth login`; telling them to run it
            // again is noise, so drop the re-login hint from the probe reason.
            let reason = reason
                .strip_suffix(crate::rest::RELOGIN_HINT)
                .unwrap_or(reason);
            CliError::Auth(format!(
                "timed out waiting for browser sign-in (last check: {reason})"
            ))
        }
        None => CliError::Auth("timed out waiting for browser sign-in".into()),
    }
}

/// Poll the browser's cookies until `probe` accepts a session header for `host`.
async fn poll_for_session<F, Fut>(
    cdp: &mut Cdp,
    host: &str,
    probe: &mut F,
    last_rejection: &mut Option<String>,
) -> Result<String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Probe>>,
{
    let mut last_tried: Option<String> = None;
    loop {
        let cookies = cdp.call("Storage.getCookies", json!({})).await?;
        let list = cookies["cookies"].as_array().cloned().unwrap_or_default();
        if let Some(header) = session_cookie_header(&list, host)
            && last_tried.as_deref() != Some(header.as_str())
        {
            last_tried = Some(header.clone());
            match probe(header.clone()).await? {
                Probe::Accepted => return Ok(header),
                Probe::Rejected(reason) => *last_rejection = Some(reason),
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Launch the browser at `start_url`, wait for SSO and return a cookie header
/// for `host` that `probe` accepts. `probe` is called with each new candidate
/// (cookies can appear before the session is fully established).
///
/// The whole wait is bounded by `LOGIN_TIMEOUT` and interruptible with Ctrl-C;
/// either way the browser is shut down and its profile directory removed.
pub(crate) async fn capture_session<F, Fut>(
    browser: &Path,
    start_url: &str,
    host: &str,
    mut probe: F,
) -> Result<String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Probe>>,
{
    let profile = tempfile::Builder::new()
        .prefix("sharepoint-login-")
        .tempdir()
        .map_err(|e| CliError::Other(format!("create browser profile dir: {e}")))?;
    let mut child = tokio::process::Command::new(browser)
        .args(launch_args(profile.path(), start_url))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| CliError::Input(format!("launch {}: {e}", browser.display())))?;

    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    let mut cdp: Option<Cdp> = None;
    let mut last_rejection: Option<String> = None;
    let session = async {
        let (port, path) = wait_for_devtools(profile.path(), &mut child).await?;
        let cdp = cdp.insert(Cdp::connect(port, &path).await?);
        poll_for_session(cdp, host, &mut probe, &mut last_rejection).await
    };
    let result = tokio::select! {
        r = tokio::time::timeout_at(deadline, session) => {
            r.unwrap_or_else(|_| Err(timeout_error(last_rejection.as_deref())))
        }
        Ok(()) = tokio::signal::ctrl_c() => Err(CliError::Auth(LOGIN_CANCELLED.into())),
    };

    // Best effort: ask the browser to close, then make sure it is gone before
    // the throwaway profile directory is removed.
    if let Some(cdp) = cdp.as_mut() {
        let _ = tokio::time::timeout(Duration::from_secs(2), cdp.call("Browser.close", json!({})))
            .await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    let _ = child.kill().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devtools_active_port() {
        assert_eq!(
            parse_devtools_active_port("41234\n/devtools/browser/abc-def\n"),
            Some((41234, "/devtools/browser/abc-def".into()))
        );
        assert_eq!(parse_devtools_active_port("garbage"), None);
        assert_eq!(parse_devtools_active_port("41234\n"), None);
    }

    #[test]
    fn builds_header_only_when_both_cookies_for_host() {
        let cookies = vec![
            json!({"name": "FedAuth", "value": "F", "domain": "contoso.sharepoint.com"}),
            json!({"name": "rtFa", "value": "R", "domain": ".sharepoint.com"}),
            json!({"name": "FedAuth", "value": "X", "domain": "fabrikam.sharepoint.com"}),
        ];
        assert_eq!(
            session_cookie_header(&cookies, "contoso.sharepoint.com").as_deref(),
            Some("FedAuth=F; rtFa=R")
        );
        assert_eq!(
            session_cookie_header(&cookies[..1], "contoso.sharepoint.com"),
            None
        );
        assert_eq!(
            session_cookie_header(&cookies, "other.sharepoint.com"),
            None
        );
    }

    #[test]
    fn cookie_domain_match_requires_label_boundary() {
        assert!(cookie_domain_matches(
            ".sharepoint.com",
            "contoso.sharepoint.com"
        ));
        assert!(cookie_domain_matches(
            "Contoso.SharePoint.com",
            "contoso.sharepoint.com"
        ));
        assert!(!cookie_domain_matches(
            "point.com",
            "contoso.sharepoint.com.evil"
        ));
        assert!(!cookie_domain_matches(
            "harepoint.com",
            "contoso.sharepoint.com"
        ));
    }

    #[test]
    fn launch_args_bind_devtools_to_loopback_and_isolate_profile() {
        let args = launch_args(Path::new("/tmp/p"), "https://contoso.sharepoint.com/");
        assert!(args.contains(&"--remote-debugging-address=127.0.0.1".to_string()));
        assert!(args.contains(&"--user-data-dir=/tmp/p".to_string()));
        assert_eq!(args.last().unwrap(), "https://contoso.sharepoint.com/");
    }

    #[test]
    fn find_in_path_picks_first_existing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chromium"), "").unwrap();
        let found = find_in_path(dir.path().to_str().unwrap(), CANDIDATES).unwrap();
        assert_eq!(found, dir.path().join("chromium"));
    }

    #[test]
    fn explicit_browser_override_wins() {
        assert_eq!(
            find_browser(Some("/opt/edge")).unwrap(),
            PathBuf::from("/opt/edge")
        );
    }

    #[test]
    fn timeout_error_names_last_rejection() {
        let msg = |e: CliError| match e {
            CliError::Auth(m) => m,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(
            msg(timeout_error(None)),
            "timed out waiting for browser sign-in"
        );
        assert_eq!(
            msg(timeout_error(Some("SharePoint 403: Access denied"))),
            "timed out waiting for browser sign-in (last check: SharePoint 403: Access denied)"
        );
        let with_hint = format!(
            "SharePoint 302: redirected to sign-in{}",
            crate::rest::RELOGIN_HINT
        );
        assert_eq!(
            msg(timeout_error(Some(&with_hint))),
            "timed out waiting for browser sign-in (last check: SharePoint 302: redirected to sign-in)"
        );
    }

    #[test]
    fn other_io_errors_are_not_cancellation() {
        let err = tungstenite::Error::Io(std::io::ErrorKind::PermissionDenied.into());
        match ws_error("read", err) {
            CliError::Auth(m) => assert!(m.starts_with("browser DevTools read"), "{m}"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn abrupt_socket_close_is_login_cancelled() {
        let reset = tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        );
        for err in [
            reset,
            tungstenite::Error::ConnectionClosed,
            tungstenite::Error::Io(std::io::ErrorKind::ConnectionReset.into()),
            tungstenite::Error::Io(std::io::ErrorKind::UnexpectedEof.into()),
        ] {
            match ws_error("read", err) {
                CliError::Auth(m) => assert!(m.starts_with("login cancelled"), "{m}"),
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}
