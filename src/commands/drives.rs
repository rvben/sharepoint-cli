//! `sharepoint drives list <site-ref>`

use crate::auth::AuthContext;
use crate::cli::{DrivesCmd, Runtime};
use crate::error::Result;
use crate::graph::{GraphClient, drives, sites};
use crate::reference::SiteRef;

pub async fn run(rt: &Runtime, cmd: DrivesCmd) -> Result<()> {
    match cmd {
        DrivesCmd::List { site, limit, all } => list(rt, &site, limit, all).await,
    }
}

async fn list(rt: &Runtime, site_input: &str, limit: usize, all: bool) -> Result<()> {
    let auth = AuthContext::new(rt.cfg.clone(), rt.cache_path.clone());
    let graph = GraphClient::new(auth);

    // The site argument can be a URL, an alias name, or "default".
    let site_ref = if site_input == "default" {
        SiteRef::Default
    } else if site_input.starts_with("http://") || site_input.starts_with("https://") {
        SiteRef::Url(site_input.to_string())
    } else {
        SiteRef::Name(site_input.to_string())
    };

    let site = sites::resolve(
        &graph,
        &site_ref,
        &rt.cfg.site_aliases,
        rt.cfg.default_site.as_deref(),
    )
    .await?;
    let mut all_drives = drives::list_drives(&graph, &site.id).await?;
    let total = all_drives.len();
    if !all && all_drives.len() > limit {
        all_drives.truncate(limit);
    }

    if rt.out.json {
        let items: Vec<_> = all_drives
            .iter()
            .map(|d| {
                serde_json::json!({
                    "id": d.id,
                    "name": d.name,
                    "drive_type": d.drive_type,
                    "site": {"id": site.id, "name": site.display_name, "url": site.web_url},
                })
            })
            .collect();
        rt.out.print_json(&serde_json::json!({
            "total": total,
            "next": null,
            "items": items,
        }));
    } else {
        for d in &all_drives {
            rt.out
                .print_data(&format!("{:30}  {:18}  {}", d.name, d.drive_type, d.id));
        }
        rt.out
            .print_message(&format!("({total} drive(s) on {})", site.display_name));
    }
    Ok(())
}
