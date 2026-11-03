//! `sharepoint doctor` — one concise view of setup, safety, credentials, and access.

use chrono::Utc;
use serde::Serialize;

use crate::auth::{AuthContext, session_store, token_cache};
use crate::cli::Runtime;
use crate::config::BackendKind;
use crate::error::Result;
use crate::graph::GraphClient;
use crate::rest::RestClient;

#[derive(Serialize)]
struct Check {
    name: &'static str,
    status: &'static str,
    detail: String,
}

pub async fn run(rt: &Runtime, offline: bool) -> Result<()> {
    let mut checks = Vec::new();
    checks.push(Check {
        name: "configuration",
        status: if rt.config_path.is_file() {
            "pass"
        } else {
            "fail"
        },
        detail: if rt.config_path.is_file() {
            rt.config_path.display().to_string()
        } else {
            format!(
                "{} is missing; run `sharepoint init`",
                rt.config_path.display()
            )
        },
    });
    // Tenant and client ID are only needed for Graph's device-code sign-in.
    if rt.cfg.backend == BackendKind::Graph {
        checks.push(Check {
            name: "profile",
            status: if rt.cfg.tenant_id.is_some() && rt.cfg.client_id.is_some() {
                "pass"
            } else {
                "fail"
            },
            detail: format!(
                "{} (tenant: {}, client: {})",
                rt.cfg.profile_name,
                present(rt.cfg.tenant_id.is_some()),
                present(rt.cfg.client_id.is_some())
            ),
        });
    }
    checks.push(Check {
        name: "write_safety",
        status: "pass",
        detail: if rt.cfg.read_only {
            "read-only mode is enabled".into()
        } else {
            "remote writes are enabled".into()
        },
    });

    if rt.cfg.backend == BackendKind::Rest {
        rest_checks(rt, offline, &mut checks).await;
        return finish(rt, &checks);
    }

    let cache = token_cache::load(&rt.cache_path)?;
    let matching = match (&rt.cfg.tenant_id, &rt.cfg.client_id) {
        (Some(tenant), Some(client)) => {
            let prefix = format!("{tenant}:{client}:");
            cache
                .entries
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };
    let fresh = matching
        .iter()
        .filter(|entry| entry.access_token_expires_at > Utc::now())
        .count();
    checks.push(Check {
        name: "credential_cache",
        status: if matching.is_empty() { "fail" } else { "pass" },
        detail: if matching.is_empty() {
            "no matching account; run `sharepoint auth login`".into()
        } else {
            format!(
                "{} account(s), {fresh} with a fresh access token",
                matching.len()
            )
        },
    });

    if offline {
        checks.push(Check {
            name: "microsoft_graph",
            status: "skip",
            detail: "network check skipped".into(),
        });
    } else if rt.cfg.tenant_id.is_some() && rt.cfg.client_id.is_some() && !matching.is_empty() {
        let graph = GraphClient::new(AuthContext::new(rt.cfg.clone(), rt.cache_path.clone()));
        match graph
            .get_json::<serde_json::Value>("/me?$select=id,displayName,userPrincipalName")
            .await
        {
            Ok(identity) => checks.push(Check {
                name: "microsoft_graph",
                status: "pass",
                detail: identity["userPrincipalName"]
                    .as_str()
                    .or_else(|| identity["displayName"].as_str())
                    .unwrap_or("signed-in account")
                    .to_string(),
            }),
            Err(error) => checks.push(Check {
                name: "microsoft_graph",
                status: "fail",
                detail: error.to_string(),
            }),
        }
    } else {
        checks.push(Check {
            name: "microsoft_graph",
            status: "skip",
            detail: "skipped until setup and sign-in are complete".into(),
        });
    }

    finish(rt, &checks)
}

/// Host, stored browser session and a live `currentuser` call for the rest backend.
async fn rest_checks(rt: &Runtime, offline: bool, checks: &mut Vec<Check>) {
    let host = rt.cfg.rest.host.clone();
    checks.push(Check {
        name: "sharepoint_host",
        status: if host.is_some() { "pass" } else { "fail" },
        detail: host
            .clone()
            .unwrap_or_else(|| "set `sharepoint_host` in the profile or SHAREPOINT_HOST".into()),
    });
    let Some(host) = host else { return };

    let session_result = session_store::get(&rt.session_path, &host);
    let from_env = rt.cfg.rest.cookie_override.is_some();
    let has_cookie = matches!(session_result, Ok(Some(_))) || from_env;
    checks.push(Check {
        name: "browser_session",
        status: if has_cookie { "pass" } else { "fail" },
        detail: if from_env {
            "from SHAREPOINT_COOKIE".into()
        } else {
            match &session_result {
                Ok(Some(s)) => format!("captured {}", s.captured_at.to_rfc3339()),
                Ok(None) => "no session; run `sharepoint auth login`".into(),
                Err(e) => format!("sessions.json is corrupt: {e}"),
            }
        },
    });

    let (status, detail) = if offline {
        ("skip", "network check skipped".to_string())
    } else if !has_cookie {
        ("skip", "skipped until sign-in is complete".to_string())
    } else {
        let user = match RestClient::from_config(&rt.cfg, &rt.session_path) {
            Ok(client) => client.current_user().await,
            Err(error) => Err(error),
        };
        match user {
            Ok(u) => ("pass", if u.email.is_empty() { u.title } else { u.email }),
            Err(error) => ("fail", error.to_string()),
        }
    };
    checks.push(Check {
        name: "sharepoint_rest",
        status,
        detail,
    });
}

fn finish(rt: &Runtime, checks: &[Check]) -> Result<()> {
    let healthy = checks.iter().all(|check| check.status != "fail");
    if rt.out.json {
        rt.out
            .print_json(&serde_json::json!({"checks": checks, "healthy": healthy}));
    } else {
        for check in checks {
            rt.out.print_data(&format!(
                "{:<20} {:<5} {}",
                check.name, check.status, check.detail
            ));
        }
    }
    Ok(())
}

fn present(value: bool) -> &'static str {
    if value { "set" } else { "missing" }
}
