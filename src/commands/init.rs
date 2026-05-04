//! `sharepoint init` — interactive first-run setup.
//!
//! Prompts for tenant + (optional) default site, writes the config file,
//! then runs the same device-code login as `sharepoint auth login`. Only
//! device-code authentication is supported; client-credential flows are
//! not yet implemented.

use std::io::{BufRead, Write};

use crate::cli::{AuthCmd, Runtime};
use crate::commands::auth;
use crate::config;
use crate::error::{CliError, Result};

pub async fn run(rt: &Runtime) -> Result<()> {
    if rt.out.quiet {
        return Err(CliError::Input(
            "init is interactive and cannot run with --quiet".into(),
        ));
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let tenant = prompt(
        &mut lines,
        "Tenant (domain or GUID, e.g. contoso.onmicrosoft.com): ",
    )?;
    if tenant.is_empty() {
        return Err(CliError::Input("tenant is required".into()));
    }
    let default_site = prompt(
        &mut lines,
        "Default site name or URL (optional, press enter to skip): ",
    )?;

    let profile_name = rt.cfg.profile_name.clone();
    let mut file = rt.config_file.clone();
    let entry = file.profile.entry(profile_name.clone()).or_default();
    entry.tenant_id = Some(tenant.clone());
    if !default_site.is_empty() {
        entry.default_site = Some(default_site.clone());
    }
    config::save_file(&rt.config_path, &file)?;
    rt.out
        .print_message(&format!("Wrote {}", rt.config_path.display()));

    // Re-build runtime so the new config is loaded for the auth-login call.
    let mut updated = rt.cfg.clone();
    updated.tenant_id = Some(tenant);
    updated.default_site = if default_site.is_empty() {
        file.profile
            .get(&profile_name)
            .and_then(|p| p.default_site.clone())
    } else {
        Some(default_site)
    };
    let new_rt = Runtime {
        out: rt.out,
        cfg: updated,
        config_file: file,
        config_path: rt.config_path.clone(),
        cache_path: rt.cache_path.clone(),
    };

    auth::run(&new_rt, AuthCmd::Login).await
}

fn prompt(lines: &mut std::io::Lines<std::io::StdinLock<'_>>, label: &str) -> Result<String> {
    eprint!("{label}");
    std::io::stderr().flush().ok();
    match lines.next() {
        Some(Ok(line)) => Ok(line.trim().to_string()),
        Some(Err(e)) => Err(CliError::Other(format!("read stdin: {e}"))),
        None => Ok(String::new()),
    }
}
