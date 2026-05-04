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
    rt.out.print_json(&value);
    Ok(())
}

async fn path_only(rt: &Runtime) -> Result<()> {
    rt.out.print_data(&rt.config_path.display().to_string());
    Ok(())
}
