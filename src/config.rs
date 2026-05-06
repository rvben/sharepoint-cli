//! Configuration: profile-based TOML at `~/.config/sharepoint/config.toml`,
//! merged with `SHAREPOINT_*` env vars and CLI flags.
//!
//! There is intentionally no separate `[default]` section. The active profile
//! is whichever block matches `[profile.<name>]`; the literal name `default`
//! plays the special-default role.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};

pub const ENV_PROFILE: &str = "SHAREPOINT_PROFILE";
pub const ENV_TENANT: &str = "SHAREPOINT_TENANT_ID";
pub const ENV_CLIENT_ID: &str = "SHAREPOINT_CLIENT_ID";
pub const ENV_DEFAULT_SITE: &str = "SHAREPOINT_DEFAULT_SITE";
pub const ENV_READ_ONLY: &str = "SHAREPOINT_READ_ONLY";
pub const ENV_ACCESS_TOKEN: &str = "SHAREPOINT_ACCESS_TOKEN";
pub const ENV_REFRESH_TOKEN: &str = "SHAREPOINT_REFRESH_TOKEN";
pub const ENV_DEBUG_HTTP: &str = "SHAREPOINT_DEBUG_HTTP";
pub const ENV_GRAPH_ENDPOINT: &str = "MICROSOFT_GRAPH_ENDPOINT";
pub const ENV_LOGIN_ENDPOINT: &str = "MICROSOFT_LOGIN_ENDPOINT";

pub const DEFAULT_PROFILE: &str = "default";
pub const DEFAULT_GRAPH_ENDPOINT: &str = "https://graph.microsoft.com/v1.0";
pub const DEFAULT_LOGIN_ENDPOINT: &str = "https://login.microsoftonline.com";

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ConfigFile {
    #[serde(default)]
    pub profile: BTreeMap<String, Profile>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Profile {
    pub tenant_id: Option<String>,
    pub client_id: Option<String>,
    pub default_site: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub sites: BTreeMap<String, String>,
}

/// Fully resolved runtime settings (after merging file + env + flags).
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub profile_name: String,
    pub tenant_id: Option<String>,
    pub client_id: Option<String>,
    pub default_site: Option<String>,
    pub read_only: bool,
    pub site_aliases: BTreeMap<String, String>,
    pub graph_endpoint: String,
    pub login_endpoint: String,
    pub debug_http: bool,
    pub access_token_override: Option<String>,
    pub refresh_token_seed: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| CliError::Other("could not determine config dir".into()))?;
    Ok(base.join("sharepoint").join("config.toml"))
}

pub fn token_cache_path() -> Result<PathBuf> {
    let base =
        dirs::cache_dir().ok_or_else(|| CliError::Other("could not determine cache dir".into()))?;
    Ok(base.join("sharepoint").join("tokens.json"))
}

pub fn load_file(path: &Path) -> Result<ConfigFile> {
    if !path.exists() {
        return Ok(ConfigFile::default());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::Other(format!("read {}: {e}", path.display())))?;
    let cfg: ConfigFile = toml::from_str(&text)
        .map_err(|e| CliError::Input(format!("parse {}: {e}", path.display())))?;
    Ok(cfg)
}

pub fn save_file(path: &Path, cfg: &ConfigFile) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| CliError::Other(format!("mkdir {}: {e}", parent.display())))?;

    let body = toml::to_string_pretty(cfg)
        .map_err(|e| CliError::Other(format!("serialize config: {e}")))?;

    // Write to a tempfile in the same directory, then rename into place so a
    // mid-write crash never leaves a truncated or partially-written config.
    let mut tmp = tempfile::Builder::new()
        .prefix(".config-")
        .suffix(".toml.tmp")
        .tempfile_in(parent)
        .map_err(|e| CliError::Other(format!("tempfile in {}: {e}", parent.display())))?;
    tmp.write_all(body.as_bytes())
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
    Ok(())
}

/// Update a profile's tenant_id in the file and persist atomically.
pub fn write_profile_tenant_id(path: &Path, profile: &str, tenant_id: &str) -> Result<()> {
    let mut file = load_file(path)?;
    let entry = file.profile.entry(profile.to_string()).or_default();
    entry.tenant_id = Some(tenant_id.to_string());
    save_file(path, &file)
}

fn parse_bool_env(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Merge a `ConfigFile` with environment variables and explicit CLI flags.
///
/// Precedence (highest first): explicit flag → env var → profile field → built-in default.
pub fn resolve(
    file: &ConfigFile,
    profile_flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedConfig> {
    let profile_name = profile_flag
        .map(str::to_owned)
        .or_else(|| env(ENV_PROFILE))
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string());

    let profile = file.profile.get(&profile_name).cloned().unwrap_or_default();

    let tenant_id = env(ENV_TENANT).or(profile.tenant_id);
    let client_id = env(ENV_CLIENT_ID).or(profile.client_id);
    let default_site = env(ENV_DEFAULT_SITE).or(profile.default_site);

    let read_only = env(ENV_READ_ONLY)
        .map(|v| parse_bool_env(&v))
        .unwrap_or(profile.read_only);

    let graph_endpoint =
        env(ENV_GRAPH_ENDPOINT).unwrap_or_else(|| DEFAULT_GRAPH_ENDPOINT.to_string());
    let login_endpoint =
        env(ENV_LOGIN_ENDPOINT).unwrap_or_else(|| DEFAULT_LOGIN_ENDPOINT.to_string());
    let debug_http = env(ENV_DEBUG_HTTP)
        .map(|v| parse_bool_env(&v))
        .unwrap_or(false);

    Ok(ResolvedConfig {
        profile_name,
        tenant_id,
        client_id,
        default_site,
        read_only,
        site_aliases: profile.sites,
        graph_endpoint,
        login_endpoint,
        debug_http,
        access_token_override: env(ENV_ACCESS_TOKEN),
        refresh_token_seed: env(ENV_REFRESH_TOKEN),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn missing_profile_yields_empty_resolved() {
        let file = ConfigFile::default();
        let r = resolve(&file, None, &empty_env).unwrap();
        assert_eq!(r.profile_name, "default");
        assert!(r.tenant_id.is_none());
        assert!(!r.read_only);
        assert_eq!(r.graph_endpoint, DEFAULT_GRAPH_ENDPOINT);
    }

    #[test]
    fn profile_fields_resolve_when_no_env() {
        let mut file = ConfigFile::default();
        let p = Profile {
            tenant_id: Some("contoso.onmicrosoft.com".into()),
            default_site: Some("Marketing".into()),
            read_only: true,
            ..Default::default()
        };
        file.profile.insert("default".into(), p);
        let r = resolve(&file, None, &empty_env).unwrap();
        assert_eq!(r.tenant_id.as_deref(), Some("contoso.onmicrosoft.com"));
        assert_eq!(r.default_site.as_deref(), Some("Marketing"));
        assert!(r.read_only);
    }

    #[test]
    fn env_overrides_profile() {
        let mut file = ConfigFile::default();
        let p = Profile {
            tenant_id: Some("from-file".into()),
            ..Default::default()
        };
        file.profile.insert("default".into(), p);
        let env = |k: &str| match k {
            ENV_TENANT => Some("from-env".to_string()),
            _ => None,
        };
        let r = resolve(&file, None, &env).unwrap();
        assert_eq!(r.tenant_id.as_deref(), Some("from-env"));
    }

    #[test]
    fn flag_overrides_env_for_profile_name() {
        let env = |k: &str| match k {
            ENV_PROFILE => Some("from-env".to_string()),
            _ => None,
        };
        let r = resolve(&ConfigFile::default(), Some("from-flag"), &env).unwrap();
        assert_eq!(r.profile_name, "from-flag");
    }

    #[test]
    fn read_only_env_recognizes_truthy_values() {
        for raw in ["1", "true", "TRUE", "yes", "on"] {
            let env = |k: &str| match k {
                ENV_READ_ONLY => Some(raw.to_string()),
                _ => None,
            };
            let r = resolve(&ConfigFile::default(), None, &env).unwrap();
            assert!(r.read_only, "expected read_only for {raw:?}");
        }
    }

    #[test]
    fn read_only_env_off_for_falsy_values() {
        for raw in ["0", "false", "no", "", "off"] {
            let env = |k: &str| match k {
                ENV_READ_ONLY => Some(raw.to_string()),
                _ => None,
            };
            let r = resolve(&ConfigFile::default(), None, &env).unwrap();
            assert!(!r.read_only, "expected !read_only for {raw:?}");
        }
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = ConfigFile::default();
        let mut sites = BTreeMap::new();
        sites.insert(
            "Marketing".into(),
            "https://contoso.sharepoint.com/sites/Marketing".into(),
        );
        let p = Profile {
            tenant_id: Some("contoso.onmicrosoft.com".into()),
            sites,
            ..Default::default()
        };
        cfg.profile.insert("default".into(), p);
        save_file(&path, &cfg).unwrap();
        let loaded = load_file(&path).unwrap();
        let p = loaded.profile.get("default").unwrap();
        assert_eq!(p.tenant_id.as_deref(), Some("contoso.onmicrosoft.com"));
        assert_eq!(
            p.sites.get("Marketing").map(String::as_str),
            Some("https://contoso.sharepoint.com/sites/Marketing")
        );
    }

    #[test]
    fn missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let cfg = load_file(&path).unwrap();
        assert!(cfg.profile.is_empty());
    }

    #[test]
    fn save_file_does_not_leave_temp_artifacts_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sharepoint").join("config.toml");
        let mut file = ConfigFile::default();
        file.profile
            .entry("default".to_string())
            .or_default()
            .tenant_id = Some("11111111-1111-1111-1111-111111111111".to_string());
        save_file(&path, &file).unwrap();
        // Round-trip works.
        let reloaded = load_file(&path).unwrap();
        assert_eq!(
            reloaded
                .profile
                .get("default")
                .and_then(|p| p.tenant_id.as_deref()),
            Some("11111111-1111-1111-1111-111111111111"),
        );
        // No leftover temp files in the parent.
        let parent = path.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != "config.toml")
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }
}
