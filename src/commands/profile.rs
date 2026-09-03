//! `sharepoint profile list | use | remove`

use crate::auth::token_cache;
use crate::cli::{ProfileCmd, Runtime};
use crate::config;
use crate::error::{CliError, Result};

pub async fn run(rt: &Runtime, cmd: ProfileCmd, yes: bool) -> Result<()> {
    match cmd {
        ProfileCmd::List => list(rt),
        ProfileCmd::Use { name } => use_profile(rt, &name),
        ProfileCmd::Remove { name } => remove(rt, &name, yes),
    }
}

fn list(rt: &Runtime) -> Result<()> {
    let profiles = config::profile_summaries(&rt.config_file);
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "total": profiles.len(),
            "items": profiles,
        }));
    } else if profiles.is_empty() {
        rt.out
            .print_data("No profiles configured. Run `sharepoint init`.");
    } else {
        for profile in profiles {
            rt.out.print_data(&format!(
                "{} {:<20} {}",
                if profile["active"].as_bool().unwrap_or(false) {
                    "*"
                } else {
                    " "
                },
                profile["name"].as_str().unwrap_or_default(),
                profile["tenant_id"].as_str().unwrap_or("-")
            ));
        }
    }
    Ok(())
}

fn use_profile(rt: &Runtime, name: &str) -> Result<()> {
    config::use_profile(&rt.config_path, name)?;
    if rt.out.json {
        rt.out
            .print_json(&serde_json::json!({"profile": name, "active": true}));
    } else {
        rt.out
            .print_data(&format!("Active profile set to '{name}'."));
    }
    Ok(())
}

fn remove(rt: &Runtime, name: &str, yes: bool) -> Result<()> {
    if !yes {
        return Err(CliError::Input("profile removal requires --yes".into()));
    }
    let profile = config::remove_profile(&rt.config_path, name)?
        .ok_or_else(|| CliError::NotFound(format!("profile '{name}'")))?;

    if let (Some(tenant), Some(client_id)) = (profile.tenant_id, profile.client_id) {
        let cache = token_cache::load(&rt.cache_path)?;
        let prefix = format!("{tenant}:{client_id}:");
        for key in cache.entries.keys().filter(|key| key.starts_with(&prefix)) {
            token_cache::remove(&rt.cache_path, key)?;
        }
    }

    if rt.out.json {
        rt.out
            .print_json(&serde_json::json!({"profile": name, "removed": true}));
    } else {
        rt.out.print_data(&format!("Removed profile '{name}'."));
    }
    Ok(())
}
