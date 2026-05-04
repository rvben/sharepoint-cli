//! On-disk token cache.
//!
//! Path: `~/.cache/sharepoint/tokens.json` (mode 0600).
//! Keyed by `<tenant_id>:<client_id>:<oid>` so multiple accounts coexist.
//! Atomic writes via tempfile-and-rename so a crashed write never corrupts
//! the file or leaks the rotated refresh token alongside the previous one.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCache {
    pub version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, CacheEntry>,
}

impl Default for TokenCache {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub account: Account,
    pub access_token: String,
    pub access_token_expires_at: DateTime<Utc>,
    pub refresh_token: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub name: Option<String>,
    pub tenant_id: String,
    pub oid: String,
}

pub fn cache_key(tenant_id: &str, client_id: &str, oid: &str) -> String {
    format!("{tenant_id}:{client_id}:{oid}")
}

pub fn load(path: &Path) -> Result<TokenCache> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TokenCache::default());
        }
        Err(e) => {
            return Err(CliError::Other(format!("read {}: {e}", path.display())));
        }
    };
    let cache: TokenCache = serde_json::from_str(&text)
        .map_err(|e| CliError::Other(format!("parse {}: {e}", path.display())))?;
    Ok(cache)
}

pub fn save(path: &Path, cache: &TokenCache) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| CliError::Other(format!("mkdir {}: {e}", parent.display())))?;
    let body = serde_json::to_vec_pretty(cache)
        .map_err(|e| CliError::Other(format!("serialize tokens: {e}")))?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".tokens-")
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
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)
        .map_err(|e| CliError::Other(format!("chmod 0600 {}: {e}", path.display())))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode_0600(_path: &Path) -> Result<()> {
    // Windows ACLs are out of scope; the file is in %LOCALAPPDATA%\sharepoint\
    // which is per-user by default. Documenting this in CONTRIBUTING.md is enough.
    Ok(())
}

/// Replace one entry atomically (load → mutate → save).
pub fn upsert(path: &Path, key: &str, entry: CacheEntry) -> Result<()> {
    let mut cache = load(path)?;
    cache.entries.insert(key.to_string(), entry);
    save(path, &cache)
}

/// Remove one entry; no-op if absent.
pub fn remove(path: &Path, key: &str) -> Result<bool> {
    let mut cache = load(path)?;
    let removed = cache.entries.remove(key).is_some();
    if removed {
        save(path, &cache)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn sample_entry() -> CacheEntry {
        CacheEntry {
            account: Account {
                username: "alice@contoso.com".into(),
                name: Some("Alice Example".to_string()),
                tenant_id: "tid-123".into(),
                oid: "oid-456".into(),
            },
            access_token: "AT".into(),
            access_token_expires_at: Utc::now() + Duration::minutes(60),
            refresh_token: Some("RT".to_string()),
            scopes: vec!["openid".into(), "User.Read".into()],
        }
    }

    #[test]
    fn cache_key_format() {
        assert_eq!(cache_key("t", "c", "o"), "t:c:o");
    }

    #[test]
    fn missing_file_returns_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let cache = load(&path).unwrap();
        assert_eq!(cache.version, 1);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn upsert_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        upsert(&path, "k1", sample_entry()).unwrap();
        let loaded = load(&path).unwrap();
        let e = loaded.entries.get("k1").unwrap();
        assert_eq!(e.account.username, "alice@contoso.com");
        assert_eq!(e.refresh_token.as_deref(), Some("RT"));
    }

    #[cfg(unix)]
    #[test]
    fn save_uses_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        upsert(&path, "k1", sample_entry()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn remove_returns_true_when_present_and_false_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        upsert(&path, "k1", sample_entry()).unwrap();
        assert!(remove(&path, "k1").unwrap());
        assert!(!remove(&path, "k1").unwrap());
    }

    #[test]
    fn upsert_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mut e1 = sample_entry();
        e1.refresh_token = Some("RT-old".to_string());
        upsert(&path, "k1", e1).unwrap();
        let mut e2 = sample_entry();
        e2.refresh_token = Some("RT-new".to_string());
        upsert(&path, "k1", e2).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(
            loaded.entries.get("k1").unwrap().refresh_token.as_deref(),
            Some("RT-new")
        );
    }
}
