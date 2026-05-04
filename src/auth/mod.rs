//! Authentication subsystem (device-code flow only in v0.1).
//!
//! `AuthContext` is the runtime entrypoint: every Graph call goes through
//! `access_token()`, which automatically refreshes when the cached token is
//! within 60 seconds of expiring.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::sync::Mutex;

use crate::config::ResolvedConfig;
use crate::error::{CliError, Result};

pub mod device_code;
pub mod token_cache;

/// Default Entra app `client_id` shipped in the binary.
/// Replace this with the GUID captured in Prereq B before publishing.
pub const DEFAULT_CLIENT_ID: &str = "REPLACE_WITH_REAL_CLIENT_ID";

const REFRESH_MARGIN_SECS: i64 = 60;

#[derive(Clone)]
pub struct AuthContext {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    cfg: ResolvedConfig,
    cache_path: PathBuf,
    http: reqwest::Client,
}

impl AuthContext {
    pub fn new(cfg: ResolvedConfig, cache_path: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("sharepoint-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                cfg,
                cache_path,
                http,
            })),
        }
    }

    pub fn client_id(&self) -> String {
        let inner = self.inner.try_lock();
        inner
            .ok()
            .and_then(|g| g.cfg.client_id.clone())
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
    }

    /// Get a non-expired access token, refreshing if necessary.
    /// Honors `SHAREPOINT_ACCESS_TOKEN` as a CI escape hatch (no refresh).
    pub async fn access_token(&self) -> Result<String> {
        let guard = self.inner.lock().await;

        if let Some(t) = guard.cfg.access_token_override.clone() {
            return Ok(t);
        }

        let tenant = guard.cfg.tenant_id.clone().ok_or_else(|| {
            CliError::Auth(
                "no tenant_id configured; run `sharepoint init` or set SHAREPOINT_TENANT_ID".into(),
            )
        })?;
        let client_id = guard
            .cfg
            .client_id
            .clone()
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());

        let cache = token_cache::load(&guard.cache_path)?;

        // Pick an entry matching this tenant+client_id (the only ones we can use).
        let prefix = format!("{tenant}:{client_id}:");
        let (key, entry) = cache
            .entries
            .iter()
            .find(|(k, _)| k.starts_with(&prefix))
            .map(|(k, e)| (k.clone(), e.clone()))
            .ok_or_else(|| {
                CliError::Auth(
                    "no cached credentials for this tenant; run `sharepoint auth login`".into(),
                )
            })?;

        if entry.access_token_expires_at - Utc::now() > Duration::seconds(REFRESH_MARGIN_SECS) {
            return Ok(entry.access_token);
        }

        // Refresh.
        let rt = entry.refresh_token.as_deref().ok_or_else(|| {
            CliError::Auth("cached entry has no refresh_token; run `sharepoint auth login`".into())
        })?;
        let scope = device_code::default_scope(guard.cfg.read_only);
        let resp = device_code::refresh(
            &guard.http,
            &guard.cfg.login_endpoint,
            &tenant,
            &client_id,
            rt,
            scope,
        )
        .await?;

        let new_entry = token_cache::CacheEntry {
            account: entry.account.clone(),
            access_token: resp.access_token.clone(),
            access_token_expires_at: Utc::now() + Duration::seconds(resp.expires_in as i64),
            refresh_token: Some(resp.refresh_token),
            scopes: resp.scope.split(' ').map(String::from).collect(),
        };
        token_cache::upsert(&guard.cache_path, &key, new_entry)?;
        Ok(resp.access_token)
    }

    pub async fn http(&self) -> reqwest::Client {
        self.inner.lock().await.http.clone()
    }

    pub async fn config(&self) -> ResolvedConfig {
        self.inner.lock().await.cfg.clone()
    }

    pub async fn cache_path(&self) -> PathBuf {
        self.inner.lock().await.cache_path.clone()
    }

    /// Seed the cache from `SHAREPOINT_REFRESH_TOKEN` (CI use case). The next
    /// `access_token()` call will refresh and persist the rotated token.
    /// `oid_for_seed` is a synthetic identifier — the real one comes back on first refresh.
    pub async fn seed_from_env_refresh_token(&self, refresh_token: &str) -> Result<()> {
        let guard = self.inner.lock().await;
        let tenant = guard
            .cfg
            .tenant_id
            .clone()
            .ok_or_else(|| CliError::Auth("seed: tenant_id required".into()))?;
        let client_id = guard
            .cfg
            .client_id
            .clone()
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
        let key = token_cache::cache_key(&tenant, &client_id, "seeded");
        let entry = token_cache::CacheEntry {
            account: token_cache::Account {
                username: "seeded".into(),
                name: Some("seeded".to_string()),
                tenant_id: tenant,
                oid: "seeded".into(),
            },
            access_token: String::new(),
            access_token_expires_at: Utc::now() - Duration::seconds(1),
            refresh_token: Some(refresh_token.to_string()),
            scopes: vec![],
        };
        token_cache::upsert(&guard.cache_path, &key, entry)?;
        // Drop guard so caller can immediately call access_token().
        drop(guard);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn access_token_uses_env_override_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::ResolvedConfig {
            profile_name: "default".into(),
            tenant_id: Some("contoso".into()),
            client_id: Some("client-1".into()),
            default_site: None,
            read_only: false,
            site_aliases: Default::default(),
            graph_endpoint: "https://graph.example".into(),
            login_endpoint: "https://login.example".into(),
            debug_http: false,
            access_token_override: Some("ENV-TOKEN".into()),
            refresh_token_seed: None,
        };
        let ctx = AuthContext::new(cfg, dir.path().join("tokens.json"));
        assert_eq!(ctx.access_token().await.unwrap(), "ENV-TOKEN");
    }

    #[tokio::test]
    async fn access_token_errors_when_no_cache_and_no_env() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::ResolvedConfig {
            profile_name: "default".into(),
            tenant_id: Some("contoso".into()),
            client_id: Some("client-1".into()),
            default_site: None,
            read_only: false,
            site_aliases: Default::default(),
            graph_endpoint: "https://graph.example".into(),
            login_endpoint: "https://login.example".into(),
            debug_http: false,
            access_token_override: None,
            refresh_token_seed: None,
        };
        let ctx = AuthContext::new(cfg, dir.path().join("tokens.json"));
        let err = ctx.access_token().await.unwrap_err();
        assert!(matches!(err, CliError::Auth(_)));
    }
}
