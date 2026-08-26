//! `sharepoint doctor` — one concise view of setup, safety, credentials, and access.

use chrono::Utc;
use serde::Serialize;

use crate::auth::{AuthContext, token_cache};
use crate::cli::Runtime;
use crate::error::Result;
use crate::graph::GraphClient;

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
    checks.push(Check {
        name: "write_safety",
        status: "pass",
        detail: if rt.cfg.read_only {
            "read-only mode is enabled".into()
        } else {
            "remote writes are enabled".into()
        },
    });

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

    let healthy = checks.iter().all(|check| check.status != "fail");
    if rt.out.json {
        rt.out
            .print_json(&serde_json::json!({"checks": checks, "healthy": healthy}));
    } else {
        for check in &checks {
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
