//! Browser-session cookies for the SharePoint REST backend, keyed by host.
//!
//! Stored at `~/.cache/sharepoint/sessions.json` with mode 0600, alongside
//! the Graph token cache. The cookie header is a bearer secret: never log it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionStore {
    #[serde(default)]
    pub sessions: BTreeMap<String, Session>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    /// `FedAuth=…; rtFa=…`
    pub cookie: String,
    pub captured_at: DateTime<Utc>,
    #[serde(default)]
    pub username: Option<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("cookie", &"<redacted>")
            .field("captured_at", &self.captured_at)
            .field("username", &self.username)
            .finish()
    }
}

pub fn load(path: &Path) -> Result<SessionStore> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionStore::default());
        }
        Err(e) => return Err(CliError::Other(format!("read {}: {e}", path.display()))),
    };
    serde_json::from_str(&text)
        .map_err(|e| CliError::Other(format!("parse {}: {e}", path.display())))
}

fn save(path: &Path, store: &SessionStore) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| CliError::Other(format!("mkdir {}: {e}", parent.display())))?;
    let body = serde_json::to_vec_pretty(store)
        .map_err(|e| CliError::Other(format!("serialize sessions: {e}")))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".sessions-")
        .suffix(".json.tmp")
        .tempfile_in(parent)
        .map_err(|e| CliError::Other(format!("tempfile in {}: {e}", parent.display())))?;
    tmp.write_all(&body)
        .map_err(|e| CliError::Other(format!("write tempfile: {e}")))?;
    tmp.flush()
        .map_err(|e| CliError::Other(format!("flush tempfile: {e}")))?;
    set_mode_0600(tmp.path())?;
    tmp.persist(path)
        .map_err(|e| CliError::Other(format!("persist tempfile: {e}")))?;
    Ok(())
}

#[cfg(unix)]
fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| CliError::Other(format!("chmod 0600 {}: {e}", path.display())))
}

#[cfg(not(unix))]
fn set_mode_0600(_path: &Path) -> Result<()> {
    // Windows ACLs are out of scope; the file is in %LOCALAPPDATA%\sharepoint\
    // which is per-user by default. Documenting this in CONTRIBUTING.md is enough.
    Ok(())
}

pub fn upsert(path: &Path, host: &str, session: Session) -> Result<()> {
    let mut store = load(path)?;
    store.sessions.insert(host.to_ascii_lowercase(), session);
    save(path, &store)
}

pub fn get(path: &Path, host: &str) -> Result<Option<Session>> {
    Ok(load(path)?
        .sessions
        .get(&host.to_ascii_lowercase())
        .cloned())
}

/// Remove one host's session; returns whether anything was removed.
pub fn remove(path: &Path, host: &str) -> Result<bool> {
    let mut store = load(path)?;
    let removed = store.sessions.remove(&host.to_ascii_lowercase()).is_some();
    if removed {
        save(path, &store)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(cookie: &str) -> Session {
        Session {
            cookie: cookie.into(),
            captured_at: Utc::now(),
            username: Some("u@contoso.com".into()),
        }
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            get(&dir.path().join("s.json"), "contoso.sharepoint.com")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn upsert_get_remove_roundtrip_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        upsert(&p, "Contoso.SharePoint.com", session("FedAuth=F; rtFa=R")).unwrap();
        let got = get(&p, "contoso.sharepoint.com").unwrap().unwrap();
        assert_eq!(got.cookie, "FedAuth=F; rtFa=R");
        assert!(remove(&p, "CONTOSO.sharepoint.com").unwrap());
        assert!(!remove(&p, "contoso.sharepoint.com").unwrap());
    }

    #[test]
    fn session_debug_redacts_cookie() {
        let debug = format!("{:?}", session("FedAuth=super-secret; rtFa=also-secret"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("also-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        upsert(&p, "h", session("c")).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
