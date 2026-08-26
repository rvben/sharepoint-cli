//! `sharepoint init` — guided or headless first-run setup.
//!
//! Resolves the tenant, client ID, and optional default site; writes the
//! config file; then runs the same device-code login as `sharepoint auth
//! login`. Headless callers can provide explicit values and defer login.

use std::io::{BufRead, IsTerminal, Write};

use crate::cli::{AuthCmd, InitArgs, Runtime};
use crate::commands::auth;
use crate::config;
use crate::error::{CliError, Result};

pub async fn run(rt: &Runtime, args: InitArgs) -> Result<()> {
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    // `read_only` does not gate init: init bootstraps the config file from
    // scratch, and would have nothing to protect if the file does not exist.
    if interactive {
        rt.out.print_message(&format!(
            "Let's connect the '{}' profile to SharePoint.",
            rt.cfg.profile_name
        ));
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let tenant = resolve_required(
        &mut lines,
        interactive,
        "Tenant (domain or GUID)",
        rt.cfg.tenant_id.as_deref(),
        "--tenant <domain-or-guid> or SHAREPOINT_TENANT_ID",
    )?;
    let client_id = resolve_required(
        &mut lines,
        interactive,
        "Client ID (Entra public-client app GUID)",
        rt.cfg.client_id.as_deref(),
        "--client-id <application-id> or SHAREPOINT_CLIENT_ID",
    )?;
    let configured_site = args.default_site.or_else(|| rt.cfg.default_site.clone());
    let default_site = if interactive {
        prompt_value(
            &mut lines,
            "Default site name or URL",
            configured_site.as_deref(),
            true,
        )?
    } else {
        configured_site
    };

    let profile_name = rt.cfg.profile_name.clone();
    let mut file = rt.config_file.clone();
    let entry = file.profile.entry(profile_name.clone()).or_default();
    entry.tenant_id = Some(tenant.clone());
    entry.client_id = Some(client_id.clone());
    entry.default_site = default_site.clone();
    entry.read_only = args.read_only || rt.cfg.read_only;
    config::save_file(&rt.config_path, &file)?;
    rt.out
        .print_message(&format!("Wrote {}", rt.config_path.display()));

    // Re-build runtime so the new config is loaded for the auth-login call.
    let mut updated = rt.cfg.clone();
    updated.tenant_id = Some(tenant);
    updated.client_id = Some(client_id);
    updated.default_site = default_site;
    updated.read_only = args.read_only || rt.cfg.read_only;
    let read_only = updated.read_only;
    let new_rt = Runtime {
        out: rt.out,
        cfg: updated,
        config_file: file,
        config_path: rt.config_path.clone(),
        cache_path: rt.cache_path.clone(),
    };

    if args.no_login {
        if rt.out.json {
            rt.out.print_json(&serde_json::json!({
                "profile": profile_name,
                "config_path": rt.config_path,
                "signed_in": false,
                "read_only": read_only,
                "next": "sharepoint auth login",
            }));
        } else {
            rt.out.print_data(&format!(
                "Profile '{profile_name}' saved. Next: `sharepoint auth login`."
            ));
        }
        return Ok(());
    }

    auth::run(&new_rt, AuthCmd::Login).await?;
    rt.out.print_message(
        "Ready. Run `sharepoint doctor` to verify access, then `sharepoint sites list`.",
    );
    Ok(())
}

fn resolve_required(
    lines: &mut std::io::Lines<std::io::StdinLock<'_>>,
    interactive: bool,
    label: &str,
    current: Option<&str>,
    non_interactive_help: &str,
) -> Result<String> {
    if interactive {
        return prompt_value(lines, label, current, false)?
            .ok_or_else(|| CliError::Input(format!("{label} is required")));
    }

    current
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::Input(format!(
                "{label} is required in non-interactive mode; pass {non_interactive_help}"
            ))
        })
}

fn prompt_value(
    lines: &mut std::io::Lines<std::io::StdinLock<'_>>,
    label: &str,
    current: Option<&str>,
    optional: bool,
) -> Result<Option<String>> {
    let default = current.filter(|value| !value.trim().is_empty());
    match default {
        Some(value) => eprint!("{label} [{value}]: "),
        None if optional => eprint!("{label} (optional): "),
        None => eprint!("{label}: "),
    }
    std::io::stderr().flush().ok();
    match lines.next() {
        Some(Ok(line)) => {
            let value = line.trim();
            if value.is_empty() {
                Ok(default.map(str::to_owned))
            } else {
                Ok(Some(value.to_string()))
            }
        }
        Some(Err(e)) => Err(CliError::Other(format!("read stdin: {e}"))),
        None => Ok(default.map(str::to_owned)),
    }
}
