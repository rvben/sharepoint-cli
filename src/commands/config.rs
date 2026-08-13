//! `sharepoint config show | path`

use crate::cli::{ConfigCmd, Runtime};
use crate::error::Result;

pub async fn run(rt: &Runtime, cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Show => show(rt).await,
        ConfigCmd::Path => path_only(rt).await,
    }
}

async fn show(rt: &Runtime) -> Result<()> {
    let value = serde_json::json!({
        "profile": rt.cfg.profile_name,
        "tenant_id": rt.cfg.tenant_id,
        "client_id": rt.cfg.client_id,
        "default_site": rt.cfg.default_site,
        "read_only": rt.cfg.read_only,
        "site_aliases": rt.cfg.site_aliases,
        "graph_endpoint": rt.cfg.graph_endpoint,
        "login_endpoint": rt.cfg.login_endpoint,
        "config_path": rt.config_path.display().to_string(),
        "cache_path": rt.cache_path.display().to_string(),
        // Tokens deliberately omitted — they are bearer secrets.
    });
    if rt.out.json {
        rt.out.print_json(&value);
    } else {
        let masked = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("-")
        };
        rt.out.print_data(&format!(
            "Profile: {}\nTenant: {}\nClient ID: {}\nDefault site: {}\nRead only: {}\nConfig: {}\nToken cache: {}",
            masked("profile"),
            masked("tenant_id"),
            masked("client_id"),
            masked("default_site"),
            value["read_only"],
            masked("config_path"),
            masked("cache_path")
        ));
    }
    Ok(())
}

async fn path_only(rt: &Runtime) -> Result<()> {
    rt.out.print_data(&rt.config_path.display().to_string());
    Ok(())
}
