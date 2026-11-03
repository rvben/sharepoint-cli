//! `sharepoint auth login | logout | status`

use chrono::{Duration, Utc};

use crate::auth::{
    AuthContext, browser, device_code, require_client_id, session_store, token_cache,
};
use crate::cli::{AuthCmd, Runtime};
use crate::config::{self, BackendKind};
use crate::error::{CliError, Result};
use crate::graph::GraphClient;
use crate::rest::RestClient;

pub async fn run(rt: &Runtime, cmd: AuthCmd) -> Result<()> {
    match (rt.cfg.backend, cmd) {
        (BackendKind::Rest, AuthCmd::Login) => rest_login(rt).await,
        (BackendKind::Rest, AuthCmd::Logout) => rest_logout(rt),
        (
            BackendKind::Rest,
            AuthCmd::Status {
                offline, fields, ..
            },
        ) => rest_status(rt, offline, &fields).await,
        (BackendKind::Graph, AuthCmd::Login) => login(rt).await,
        (BackendKind::Graph, AuthCmd::Logout) => logout(rt).await,
        (
            BackendKind::Graph,
            AuthCmd::Status {
                offline,
                limit,
                page,
                fields,
            },
        ) => status(rt, offline, limit, page.as_deref(), &fields).await,
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

async fn status(
    rt: &Runtime,
    offline: bool,
    limit: usize,
    _page: Option<&str>,
    fields: &[String],
) -> Result<()> {
    let cache = token_cache::load(&rt.cache_path)?;

    // Build the full account list.
    let mut all_accounts: Vec<_> = cache
        .entries
        .iter()
        .map(|(key, entry)| {
            let mut obj = serde_json::json!({
                "key": key,
                "username": entry.account.username,
                "name": entry.account.name,
                "tenant_id": entry.account.tenant_id,
                "oid": entry.account.oid,
                "expires_at": entry.access_token_expires_at.to_rfc3339(),
                "scopes": entry.scopes,
            });
            if !fields.is_empty()
                && let serde_json::Value::Object(ref mut map) = obj
            {
                map.retain(|k, _| fields.iter().any(|f| f == k));
            }
            obj
        })
        .collect();

    let total = all_accounts.len();
    all_accounts.truncate(limit);
    let identity = if offline || total == 0 {
        None
    } else {
        Some(
            GraphClient::new(AuthContext::new(rt.cfg.clone(), rt.cache_path.clone()))
                .get_json::<serde_json::Value>("/me?$select=id,displayName,userPrincipalName")
                .await?,
        )
    };

    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "total": total,
            "next": serde_json::Value::Null,
            "items": all_accounts,
            "verified": identity.is_some(),
            "identity": identity,
        }));
    } else {
        // Always emit at least the column header so stdout is non-empty in text mode.
        rt.out
            .print_data(&format!("{:30}  {}", "ACCOUNT", "EXPIRES"));
        for obj in &all_accounts {
            let username = obj["username"].as_str().unwrap_or("");
            let expires = obj["expires_at"].as_str().unwrap_or("");
            rt.out.print_data(&format!("{:30}  {}", username, expires));
        }
        if total == 0 {
            rt.out
                .print_message("No cached accounts. Run `sharepoint auth login`.");
        }
    }
    Ok(())
}

async fn rest_login(rt: &Runtime) -> Result<()> {
    let host = rt.cfg.rest.require_host()?.to_string();
    let endpoint = rt.cfg.rest.endpoint()?;
    let browser_path = browser::find_browser(rt.cfg.rest.browser.as_deref())?;
    rt.out.print_required_prompt(&format!(
        "Opening {} — sign in to https://{host}/ in the new window (timeout {}s).",
        browser_path.display(),
        browser::LOGIN_TIMEOUT.as_secs()
    ));

    // Each candidate cookie pair is probed against `currentuser`; an Auth
    // error just means SSO has not finished yet, so keep polling (its message
    // is reported if the login eventually times out).
    let cookie = browser::capture_session(
        &browser_path,
        &format!("https://{host}/"),
        &host,
        |candidate| {
            let endpoint = endpoint.clone();
            let host = host.clone();
            async move {
                let client = RestClient::with_host(&endpoint, &host, &candidate)?;
                match client.current_user().await {
                    Ok(_) => Ok(browser::Probe::Accepted),
                    Err(CliError::Auth(reason)) => Ok(browser::Probe::Rejected(reason)),
                    Err(e) => Err(e),
                }
            }
        },
    )
    .await?;

    // Re-read identity with the accepted cookie (the probe closure can't move it out).
    let client = RestClient::with_host(&endpoint, &host, &cookie)?;
    let username = client
        .current_user()
        .await
        .ok()
        .map(|u| if u.email.is_empty() { u.title } else { u.email });

    session_store::upsert(
        &rt.session_path,
        &host,
        session_store::Session {
            cookie,
            captured_at: Utc::now(),
            username: username.clone(),
        },
    )?;

    let who = username.as_deref().unwrap_or("unknown user");
    rt.out
        .print_message(&format!("Signed in to {host} as {who}"));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "backend": "rest",
            "host": host,
            "username": username,
        }));
    }
    Ok(())
}

fn rest_logout(rt: &Runtime) -> Result<()> {
    let host = rt.cfg.rest.require_host()?;
    let removed = usize::from(session_store::remove(&rt.session_path, host)?);
    rt.out
        .print_message(&format!("Removed {removed} browser session(s) for {host}"));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "backend": "rest",
            "host": host,
            "removed": removed,
        }));
    }
    Ok(())
}

async fn rest_status(rt: &Runtime, offline: bool, fields: &[String]) -> Result<()> {
    let host = rt.cfg.rest.require_host()?.to_string();
    let session = session_store::get(&rt.session_path, &host)?;
    let mut items = Vec::new();
    if let Some(s) = &session {
        let mut obj = serde_json::json!({
            "host": host,
            "username": s.username,
            "captured_at": s.captured_at.to_rfc3339(),
        });
        if !fields.is_empty()
            && let serde_json::Value::Object(ref mut map) = obj
        {
            map.retain(|k, _| fields.iter().any(|f| f == k));
        }
        items.push(obj);
    }
    let from_env = rt.cfg.rest.cookie_override.is_some();
    let user = if offline || (session.is_none() && !from_env) {
        None
    } else {
        Some(
            RestClient::from_config(&rt.cfg, &rt.session_path)?
                .current_user()
                .await?,
        )
    };
    let identity = user.as_ref().map(|u| {
        serde_json::json!({
            "name": u.title,
            "email": u.email,
            "login_name": u.login_name,
        })
    });

    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "backend": "rest",
            "total": items.len(),
            "next": serde_json::Value::Null,
            "items": items,
            "verified": identity.is_some(),
            "identity": identity,
        }));
    } else {
        // Always emit at least the column header so stdout is non-empty in text mode.
        rt.out.print_data(&format!("{:30}  {}", "HOST", "CAPTURED"));
        if let Some(s) = &session {
            rt.out
                .print_data(&format!("{:30}  {}", host, s.captured_at.to_rfc3339()));
        }
        if from_env {
            rt.out
                .print_message("Using the session cookie from SHAREPOINT_COOKIE.");
        } else if session.is_none() {
            rt.out
                .print_message("No browser session. Run `sharepoint auth login`.");
        }
        if let Some(u) = &user {
            let who = if u.email.is_empty() {
                &u.title
            } else {
                &u.email
            };
            rt.out.print_message(&format!("Verified as {who}"));
        }
    }
    Ok(())
}
