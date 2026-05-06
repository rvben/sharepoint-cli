//! Authentication subsystem (device-code flow only in v0.1).
//!
//! `AuthContext` is the runtime entrypoint: every Graph call goes through
//! `access_token()`, which automatically refreshes when the cached token is
//! within 60 seconds of expiring.
//!
//! # Lock discipline
//!
//! The `Mutex` protects only in-memory configuration (`cfg`, `cache_path`,
//! `http`) and the `refreshing` guard flag. It is never held across file I/O
//! or network calls to avoid serializing all Graph requests behind a
//! potentially-stalled refresh.
//!
//! # Single-flight refresh
//!
//! When the cached token is stale, exactly one caller becomes the designated
//! refresher (`refreshing = true`). All other concurrent callers detect the
//! flag, release the lock, and wait on the shared `Notify`. When the refresh
//! completes (success or failure) the refresher wakes all waiters, which then
//! re-enter the loop and read the now-updated cache from disk.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::sync::{Mutex, Notify};

use crate::config::ResolvedConfig;
use crate::error::{CliError, Result};

pub mod device_code;
pub mod token_cache;

const REFRESH_MARGIN_SECS: i64 = 60;

/// Pull `client_id` out of resolved config or return a helpful Auth error.
/// Used by every codepath that needs to talk to the Entra token endpoints.
pub fn require_client_id(cfg: &ResolvedConfig) -> Result<String> {
    cfg.client_id.clone().ok_or_else(|| {
        CliError::Auth(
            "client_id is required: register an Entra public-client app and set it via \
             --client-id, SHAREPOINT_CLIENT_ID, or `sharepoint init`"
                .into(),
        )
    })
}

/// In-memory mutable state — the only thing the mutex protects.
struct State {
    cfg: ResolvedConfig,
    cache_path: PathBuf,
    http: reqwest::Client,
    /// True while a refresh network call is in flight; other callers should
    /// wait on the accompanying `Notify` rather than issuing a duplicate call.
    refreshing: bool,
}

#[derive(Clone)]
pub struct AuthContext {
    /// Tuple of (mutex-guarded state, notify for single-flight coordination).
    inner: Arc<(Mutex<State>, Notify)>,
}

impl AuthContext {
    pub fn new(cfg: ResolvedConfig, cache_path: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("sharepoint-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        let state = State {
            cfg,
            cache_path,
            http,
            refreshing: false,
        };
        Self {
            inner: Arc::new((Mutex::new(state), Notify::new())),
        }
    }

    pub async fn client_id(&self) -> Result<String> {
        require_client_id(&self.inner.0.lock().await.cfg)
    }

    /// Get a non-expired access token, refreshing if necessary.
    ///
    /// Honors `SHAREPOINT_ACCESS_TOKEN` as a CI escape hatch (no refresh).
    ///
    /// When `SHAREPOINT_REFRESH_TOKEN` is set in the environment (propagated
    /// via `ResolvedConfig::refresh_token_seed`) and no valid cached token
    /// exists, the refresh token is used to bootstrap an access token via the
    /// OAuth refresh exchange.  This supports headless/CI usage where a
    /// long-lived refresh token is injected at runtime rather than stored in a
    /// persistent cache.  If the bootstrap refresh exchange fails (e.g. the
    /// token has been revoked), the error is surfaced immediately with auth
    /// exit code 3 — there is no silent fallback to interactive device-code
    /// flow, because the caller explicitly provided a token.
    pub async fn access_token(&self) -> Result<String> {
        let (mutex, notify) = &*self.inner;

        // Fast path: static env override; no disk or network needed.
        {
            let guard = mutex.lock().await;
            if let Some(t) = guard.cfg.access_token_override.clone() {
                return Ok(t);
            }
        }

        // Whether we have already seeded the cache from `SHAREPOINT_REFRESH_TOKEN`
        // this call.  Guards against an infinite loop in case the seeded entry
        // is somehow still stale after one refresh attempt.
        let mut env_seed_applied = false;

        loop {
            // --- Phase 1: snapshot config (lock held briefly) ---
            let (tenant_opt, client_id_opt, cache_path, http, login_endpoint, read_only, rt_seed) = {
                let guard = mutex.lock().await;
                (
                    guard.cfg.tenant_id.clone(),
                    guard.cfg.client_id.clone(),
                    guard.cache_path.clone(),
                    guard.http.clone(),
                    guard.cfg.login_endpoint.clone(),
                    guard.cfg.read_only,
                    guard.cfg.refresh_token_seed.clone(),
                )
                // lock released here
            };

            let tenant = tenant_opt.ok_or_else(|| {
                CliError::Auth(
                    "no tenant_id configured; run `sharepoint init` or set SHAREPOINT_TENANT_ID"
                        .into(),
                )
            })?;
            let client_id = client_id_opt.ok_or_else(|| {
                CliError::Auth(
                    "client_id is required: register an Entra public-client app and set it via \
                     --client-id, SHAREPOINT_CLIENT_ID, or `sharepoint init`"
                        .into(),
                )
            })?;

            // --- Phase 2: read token cache from disk (no lock held) ---
            let cache = token_cache::load(&cache_path)?;
            let prefix = format!("{tenant}:{client_id}:");
            let cache_hit = cache
                .entries
                .iter()
                .find(|(k, _)| k.starts_with(&prefix))
                .map(|(k, e)| (k.clone(), e.clone()));

            // When no cache entry exists, try bootstrapping from the env-var
            // refresh token (CI/headless use case).  Only attempt once per
            // `access_token()` call to avoid an infinite retry loop.
            let (key, entry) = match cache_hit {
                Some(pair) => pair,
                None => {
                    if !env_seed_applied && let Some(rt) = rt_seed {
                        // Seed the cache with an expired placeholder so the
                        // normal refresh path below can exchange it for real tokens.
                        let seed_key = token_cache::cache_key(&tenant, &client_id, "seeded");
                        let seed_entry = token_cache::CacheEntry {
                            account: token_cache::Account {
                                username: "seeded".into(),
                                name: Some("seeded".to_string()),
                                tenant_id: tenant.clone(),
                                oid: "seeded".into(),
                            },
                            access_token: String::new(),
                            access_token_expires_at: Utc::now() - Duration::seconds(1),
                            refresh_token: Some(rt),
                            scopes: vec![],
                        };
                        token_cache::upsert(&cache_path, &seed_key, seed_entry)?;
                        env_seed_applied = true;
                        continue; // Re-enter the loop to pick up the seeded entry.
                    }
                    return Err(CliError::Auth(
                        "no cached credentials for this tenant; run `sharepoint auth login`".into(),
                    ));
                }
            };

            // Token still fresh — return immediately.
            if entry.access_token_expires_at - Utc::now() > Duration::seconds(REFRESH_MARGIN_SECS) {
                return Ok(entry.access_token);
            }

            // --- Phase 3: claim the refresh slot or wait for another caller ---
            {
                let mut guard = mutex.lock().await;
                if guard.refreshing {
                    // Another caller is already refreshing. Release lock and wait.
                    drop(guard);
                    notify.notified().await;
                    // Re-enter the outer loop to re-read the (now-updated) cache.
                    continue;
                }
                // We are the designated refresher.
                guard.refreshing = true;
                // lock released here
            }

            // --- Phase 4: refresh network call (no lock held) ---
            let rt_owned = match entry.refresh_token.clone() {
                Some(rt) => rt,
                None => {
                    // Release the single-flight guard before returning.
                    let mut guard = mutex.lock().await;
                    guard.refreshing = false;
                    notify.notify_waiters();
                    drop(guard);
                    return Err(CliError::Auth(
                        "cached entry has no refresh_token; run `sharepoint auth login`".into(),
                    ));
                }
            };
            let scope = device_code::default_scope(read_only);
            let refresh_result = device_code::refresh(
                &http,
                &login_endpoint,
                &tenant,
                &client_id,
                &rt_owned,
                scope,
            )
            .await;

            // --- Phase 5: commit result (lock held briefly) ---
            {
                let mut guard = mutex.lock().await;
                guard.refreshing = false;
                notify.notify_waiters();
                // lock released here
            }

            let resp = refresh_result?;
            let access_token = resp.access_token.clone();

            // --- Phase 6: persist rotated token to disk (no lock held) ---
            let new_entry = token_cache::CacheEntry {
                account: entry.account.clone(),
                access_token: resp.access_token,
                access_token_expires_at: Utc::now() + Duration::seconds(resp.expires_in as i64),
                refresh_token: Some(resp.refresh_token),
                scopes: resp.scope.split(' ').map(String::from).collect(),
            };
            // Best-effort: if the write fails, callers still get the fresh token
            // this time; the next call will attempt a disk refresh again.
            let _ = token_cache::upsert(&cache_path, &key, new_entry);

            return Ok(access_token);
        }
    }

    pub async fn http(&self) -> reqwest::Client {
        self.inner.0.lock().await.http.clone()
    }

    pub async fn config(&self) -> ResolvedConfig {
        self.inner.0.lock().await.cfg.clone()
    }

    pub async fn cache_path(&self) -> PathBuf {
        self.inner.0.lock().await.cache_path.clone()
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
