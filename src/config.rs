//! Configuration: profile-based TOML in the native config directory (or
//! `XDG_CONFIG_HOME`), merged with `SHAREPOINT_*` env vars and CLI flags.
//!
//! There is intentionally no separate `[default]` section. Profiles live under
//! `[profile.<name>]`; `active_profile` selects the default when no override is
//! provided, and the literal name `default` remains the fallback.

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
pub const ENV_BACKEND: &str = "SHAREPOINT_BACKEND";
pub const ENV_SHAREPOINT_HOST: &str = "SHAREPOINT_HOST";
pub const ENV_REST_ENDPOINT: &str = "SHAREPOINT_REST_ENDPOINT";
pub const ENV_COOKIE: &str = "SHAREPOINT_COOKIE";
pub const ENV_BROWSER: &str = "SHAREPOINT_BROWSER";

pub const DEFAULT_PROFILE: &str = "default";
pub const DEFAULT_GRAPH_ENDPOINT: &str = "https://graph.microsoft.com/v1.0";
pub const DEFAULT_LOGIN_ENDPOINT: &str = "https://login.microsoftonline.com";

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharepoint_host: Option<String>,
}

/// Which API family a profile talks to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// Microsoft Graph with device-code OAuth (requires an Entra app).
    #[default]
    Graph,
    /// SharePoint REST (`/_api`) with browser-session cookies.
    Rest,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Graph => "graph",
            BackendKind::Rest => "rest",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "graph" => Ok(BackendKind::Graph),
            "rest" => Ok(BackendKind::Rest),
            other => Err(CliError::Input(format!(
                "unknown backend '{other}'; expected 'graph' or 'rest'"
            ))),
        }
    }
}

/// Settings used only by the SharePoint REST backend.
#[derive(Default, Clone)]
pub struct RestConfig {
    /// Tenant host, e.g. `contoso.sharepoint.com`.
    pub host: Option<String>,
    /// Base URL override (tests point this at a mock server).
    pub endpoint_override: Option<String>,
    /// Cookie header override; bypasses the session store.
    pub cookie_override: Option<String>,
    /// Browser executable for `auth login`.
    pub browser: Option<String>,
}

impl std::fmt::Debug for RestConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestConfig")
            .field("host", &self.host)
            .field("endpoint_override", &self.endpoint_override)
            .field(
                "cookie_override",
                &self.cookie_override.as_ref().map(|_| "<redacted>"),
            )
            .field("browser", &self.browser)
            .finish()
    }
}

impl RestConfig {
    /// Base URL all REST calls are sent to, without a trailing slash.
    pub fn endpoint(&self) -> Result<String> {
        if let Some(e) = &self.endpoint_override {
            return Ok(e.trim_end_matches('/').to_string());
        }
        let host = self.require_host()?;
        Ok(format!("https://{host}"))
    }

    /// The tenant host; required to look up a stored session.
    pub fn require_host(&self) -> Result<&str> {
        self.host.as_deref().ok_or_else(|| {
            CliError::Input(
                "the rest backend needs a SharePoint host; set `sharepoint_host` in the profile or SHAREPOINT_HOST".into(),
            )
        })
    }
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
    pub backend: BackendKind,
    pub rest: RestConfig,
}

pub fn config_path() -> Result<PathBuf> {
    let base = xdg_dir("XDG_CONFIG_HOME")
        .or_else(dirs::config_dir)
        .ok_or_else(|| CliError::Other("could not determine config dir".into()))?;
    Ok(base.join("sharepoint").join("config.toml"))
}

pub fn token_cache_path() -> Result<PathBuf> {
    let base = xdg_dir("XDG_CACHE_HOME")
        .or_else(dirs::cache_dir)
        .ok_or_else(|| CliError::Other("could not determine cache dir".into()))?;
    Ok(base.join("sharepoint").join("tokens.json"))
}

pub fn session_store_path() -> Result<PathBuf> {
    let base = xdg_dir("XDG_CACHE_HOME")
        .or_else(dirs::cache_dir)
        .ok_or_else(|| CliError::Other("could not determine cache dir".into()))?;
    Ok(base.join("sharepoint").join("sessions.json"))
}

/// Honor explicit XDG overrides on every OS, retaining native defaults.
/// Empty and relative paths are ignored, as required by the XDG convention.
fn xdg_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
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

/// Normalize a SharePoint host: trim whitespace, strip a leading scheme
/// (`https://`/`http://`, case-insensitive) and trailing slash, lower-case
/// the result, and treat an empty result as `None`.
pub(crate) fn normalize_host(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower.as_str());
    let without_trailing_slash = without_scheme.trim_end_matches('/');
    if without_trailing_slash.is_empty() {
        None
    } else {
        Some(without_trailing_slash.to_string())
    }
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
        .or_else(|| file.active_profile.clone())
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

    let backend = match env(ENV_BACKEND) {
        Some(v) => BackendKind::parse(&v)?,
        None => profile.backend.unwrap_or_default(),
    };
    let rest = RestConfig {
        host: env(ENV_SHAREPOINT_HOST)
            .as_deref()
            .and_then(normalize_host)
            .or_else(|| profile.sharepoint_host.as_deref().and_then(normalize_host)),
        endpoint_override: env(ENV_REST_ENDPOINT),
        cookie_override: env(ENV_COOKIE),
        browser: env(ENV_BROWSER),
    };

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
        backend,
        rest,
    })
}

pub fn profile_summaries(file: &ConfigFile) -> Vec<serde_json::Value> {
    file.profile
        .iter()
        .map(|(name, profile)| {
            serde_json::json!({
                "name": name,
                "active": file.active_profile.as_deref().unwrap_or(DEFAULT_PROFILE) == name,
                "tenant_id": profile.tenant_id,
                "client_id": profile.client_id,
                "default_site": profile.default_site,
                "read_only": profile.read_only,
                "backend": profile.backend.map(BackendKind::as_str),
                "sharepoint_host": profile.sharepoint_host,
            })
        })
        .collect()
}

pub fn use_profile(path: &Path, name: &str) -> Result<()> {
    let mut file = load_file(path)?;
    if !file.profile.contains_key(name) {
        return Err(CliError::NotFound(format!("profile '{name}'")));
    }
    file.active_profile = Some(name.into());
    save_file(path, &file)
}

pub fn remove_profile(path: &Path, name: &str) -> Result<Option<Profile>> {
    let mut file = load_file(path)?;
    let removed = file.profile.remove(name);
    if removed.is_some() {
        if file.active_profile.as_deref() == Some(name) {
            file.active_profile = file.profile.keys().next().cloned();
        }
        save_file(path, &file)?;
    }
    Ok(removed)
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
    fn active_profile_is_used_below_explicit_and_environment_overrides() {
        let mut file = ConfigFile {
            active_profile: Some("active".into()),
            ..ConfigFile::default()
        };
        file.profile.insert(
            "active".into(),
            Profile {
                tenant_id: Some("active-tenant".into()),
                ..Profile::default()
            },
        );
        let resolved = resolve(&file, None, &empty_env).unwrap();
        assert_eq!(resolved.profile_name, "active");
        assert_eq!(resolved.tenant_id.as_deref(), Some("active-tenant"));

        let env = |key: &str| (key == ENV_PROFILE).then(|| "environment".to_string());
        assert_eq!(
            resolve(&file, None, &env).unwrap().profile_name,
            "environment"
        );
        assert_eq!(
            resolve(&file, Some("explicit"), &env).unwrap().profile_name,
            "explicit"
        );
    }

    #[test]
    fn use_and_remove_profile_update_active_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut file = ConfigFile::default();
        file.profile.insert("alpha".into(), Profile::default());
        file.profile.insert("beta".into(), Profile::default());
        save_file(&path, &file).unwrap();

        use_profile(&path, "beta").unwrap();
        assert_eq!(
            load_file(&path).unwrap().active_profile.as_deref(),
            Some("beta")
        );

        assert!(remove_profile(&path, "beta").unwrap().is_some());
        let updated = load_file(&path).unwrap();
        assert_eq!(updated.active_profile.as_deref(), Some("alpha"));
        assert!(!updated.profile.contains_key("beta"));
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

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn backend_defaults_to_graph() {
        let cfg = resolve(&ConfigFile::default(), None, &env_from(&[])).unwrap();
        assert_eq!(cfg.backend, BackendKind::Graph);
        assert!(cfg.rest.host.is_none());
    }

    #[test]
    fn backend_rest_from_profile() {
        let mut file = ConfigFile::default();
        file.profile.insert(
            "default".into(),
            Profile {
                backend: Some(BackendKind::Rest),
                sharepoint_host: Some("contoso.sharepoint.com".into()),
                ..Profile::default()
            },
        );
        let cfg = resolve(&file, None, &env_from(&[])).unwrap();
        assert_eq!(cfg.backend, BackendKind::Rest);
        assert_eq!(
            cfg.rest.endpoint().unwrap(),
            "https://contoso.sharepoint.com"
        );
    }

    #[test]
    fn rest_env_overrides_win() {
        let cfg = resolve(
            &ConfigFile::default(),
            None,
            &env_from(&[
                (ENV_BACKEND, "rest"),
                (ENV_SHAREPOINT_HOST, "contoso.sharepoint.com"),
                (ENV_REST_ENDPOINT, "http://127.0.0.1:9999/"),
                (ENV_COOKIE, "FedAuth=F; rtFa=R"),
            ]),
        )
        .unwrap();
        assert_eq!(cfg.backend, BackendKind::Rest);
        assert_eq!(cfg.rest.endpoint().unwrap(), "http://127.0.0.1:9999");
        assert_eq!(
            cfg.rest.cookie_override.as_deref(),
            Some("FedAuth=F; rtFa=R")
        );
    }

    #[test]
    fn invalid_backend_env_is_input_error() {
        let err = resolve(
            &ConfigFile::default(),
            None,
            &env_from(&[(ENV_BACKEND, "soap")]),
        )
        .unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
    }

    #[test]
    fn rest_endpoint_requires_host() {
        let err = RestConfig::default().endpoint().unwrap_err();
        assert!(matches!(err, CliError::Input(_)));
    }

    #[test]
    fn normalize_host_strips_scheme_slash_and_lowercases() {
        assert_eq!(
            normalize_host("https://Contoso.sharepoint.com/"),
            Some("contoso.sharepoint.com".to_string())
        );
        assert_eq!(normalize_host("  "), None);
    }

    #[test]
    fn rest_backend_toml_round_trips_through_resolve_and_summaries() {
        let toml = r#"
            [profile.default]
            backend = "rest"
            sharepoint_host = "contoso.sharepoint.com"
        "#;
        let file: ConfigFile = toml::from_str(toml).unwrap();
        let cfg = resolve(&file, None, &env_from(&[])).unwrap();
        assert_eq!(cfg.backend, BackendKind::Rest);
        assert_eq!(cfg.rest.host.as_deref(), Some("contoso.sharepoint.com"));

        let summaries = profile_summaries(&file);
        assert_eq!(summaries[0]["backend"], "rest");
    }

    #[test]
    fn rest_config_debug_redacts_cookie() {
        let cfg = RestConfig {
            host: Some("contoso.sharepoint.com".into()),
            cookie_override: Some("FedAuth=super-secret; rtFa=also-secret".into()),
            ..Default::default()
        };
        let debug = format!("{cfg:?}");
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("also-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn sharepoint_host_env_overrides_profile() {
        let mut file = ConfigFile::default();
        file.profile.insert(
            "default".into(),
            Profile {
                sharepoint_host: Some("from-profile.sharepoint.com".into()),
                ..Profile::default()
            },
        );
        let cfg = resolve(
            &file,
            None,
            &env_from(&[(ENV_SHAREPOINT_HOST, "from-env.sharepoint.com")]),
        )
        .unwrap();
        assert_eq!(cfg.rest.host.as_deref(), Some("from-env.sharepoint.com"));
    }
}
