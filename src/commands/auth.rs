//! `sharepoint auth login | logout | status`

use chrono::{Duration, Utc};

use crate::auth::{device_code, require_client_id, token_cache};
use crate::cli::{AuthCmd, Runtime};
use crate::config;
use crate::error::{CliError, Result};

pub async fn run(rt: &Runtime, cmd: AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Login => login(rt).await,
        AuthCmd::Logout => logout(rt).await,
        AuthCmd::Status => status(rt).await,
    }
}

async fn login(rt: &Runtime) -> Result<()> {
    // `read_only` does not gate login: it only protects against config-file
    // writes; the token cache is operational state needed for any read.
    let tenant = rt.cfg.tenant_id.clone().ok_or_else(|| {
        CliError::Input(
            "no tenant configured; run `sharepoint init` or pass --tenant <domain-or-guid>".into(),
        )
    })?;
    let client_id = require_client_id(&rt.cfg)?;
    let scope = device_code::default_scope(rt.cfg.read_only);

    let http = reqwest::Client::builder()
        .user_agent(format!("sharepoint-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest");

    let dc =
        device_code::request_device_code(&http, &rt.cfg.login_endpoint, &tenant, &client_id, scope)
            .await?;

    rt.out.print_required_prompt(&format!(
        "To sign in, open {}\nand enter code: {}",
        dc.verification_uri, dc.user_code
    ));

    let resp = device_code::poll_for_token(
        &http,
        &rt.cfg.login_endpoint,
        &tenant,
        &client_id,
        &dc.device_code,
        dc.interval,
        dc.expires_in,
    )
    .await?;

    let claims = device_code::decode_id_token(&resp.id_token)?;

    // Canonicalize the configured tenant to the authoritative GUID from the id
    // token. The user may have entered a domain (e.g. contoso.onmicrosoft.com);
    // the cache key uses claims.tid, so the configured tenant must match.
    if rt.cfg.tenant_id.as_deref() != Some(claims.tid.as_str()) {
        config::write_profile_tenant_id(&rt.config_path, &rt.cfg.profile_name, &claims.tid)?;
    }

    let key = token_cache::cache_key(&claims.tid, &client_id, &claims.oid);
    let entry = token_cache::CacheEntry {
        account: token_cache::Account {
            username: claims.preferred_username.clone(),
            name: Some(claims.name.clone()),
            tenant_id: claims.tid.clone(),
            oid: claims.oid.clone(),
        },
        access_token: resp.access_token,
        access_token_expires_at: Utc::now() + Duration::seconds(resp.expires_in as i64),
        refresh_token: Some(resp.refresh_token),
        scopes: resp.scope.split(' ').map(String::from).collect(),
    };
    token_cache::upsert(&rt.cache_path, &key, entry)?;

    rt.out
        .print_message(&format!("Signed in as {}", claims.preferred_username));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "username": claims.preferred_username,
            "name": claims.name,
            "tenant_id": claims.tid,
        }));
    }
    Ok(())
}

async fn logout(rt: &Runtime) -> Result<()> {
    let tenant = rt
        .cfg
        .tenant_id
        .clone()
        .ok_or_else(|| CliError::Input("no tenant configured".into()))?;
    let client_id = require_client_id(&rt.cfg)?;
    let cache = token_cache::load(&rt.cache_path)?;
    let prefix = format!("{tenant}:{client_id}:");
    let keys: Vec<String> = cache
        .entries
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .cloned()
        .collect();
    let mut removed = 0;
    for k in keys {
        if token_cache::remove(&rt.cache_path, &k)? {
            removed += 1;
        }
    }
    rt.out
        .print_message(&format!("Removed {removed} cached account(s)"));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({"removed": removed}));
    }
    Ok(())
}

async fn status(rt: &Runtime) -> Result<()> {
    let cache = token_cache::load(&rt.cache_path)?;
    if rt.out.json {
        let accounts: Vec<_> = cache
            .entries
            .iter()
            .map(|(key, entry)| {
                serde_json::json!({
                    "key": key,
                    "username": entry.account.username,
                    "name": entry.account.name,
                    "tenant_id": entry.account.tenant_id,
                    "oid": entry.account.oid,
                    "expires_at": entry.access_token_expires_at.to_rfc3339(),
                    "scopes": entry.scopes,
                })
            })
            .collect();
        rt.out
            .print_json(&serde_json::json!({"accounts": accounts}));
    } else if cache.entries.is_empty() {
        rt.out
            .print_message("No cached accounts. Run `sharepoint auth login`.");
    } else {
        for entry in cache.entries.values() {
            rt.out.print_data(&format!(
                "{:30}  expires {}",
                entry.account.username,
                entry.access_token_expires_at.to_rfc3339()
            ));
        }
    }
    Ok(())
}
