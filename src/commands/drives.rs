//! `sharepoint drives list <site-ref>`

use crate::backend::Backend;
use crate::cli::{DrivesCmd, Runtime};
use crate::error::{CliError, Result};
use crate::reference::SiteRef;

pub async fn run(rt: &Runtime, cmd: DrivesCmd) -> Result<()> {
    match cmd {
        DrivesCmd::List {
            site,
            limit,
            all,
            fields,
        } => list(rt, &site, limit, all, &fields).await,
    }
}

async fn list(
    rt: &Runtime,
    site_input: &str,
    limit: usize,
    all: bool,
    fields: &[String],
) -> Result<()> {
    let backend = Backend::from_runtime(rt)?;

    // The site argument can be a URL, an alias name, "default", or spo://Site.
    let site_ref = if site_input == "default" {
        SiteRef::Default
    } else if site_input.starts_with("http://") || site_input.starts_with("https://") {
        SiteRef::Url(site_input.to_string())
    } else if let Some(rest) = site_input.strip_prefix("spo://") {
        // Accept bare spo://SiteName (and spo://SiteName/... with trailing segments ignored).
        let name = rest
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CliError::Input(
                    "spo:// URI is missing a site name (expected spo://SiteName)".into(),
                )
            })?;
        SiteRef::Name(name.to_string())
    } else {
        SiteRef::Name(site_input.to_string())
    };

    let site = backend
        .resolve_site(
            &site_ref,
            &rt.cfg.site_aliases,
            rt.cfg.default_site.as_deref(),
        )
        .await?;
    let mut all_drives = backend.list_drives(&site).await?;
    let total = all_drives.len();
    if !all && all_drives.len() > limit {
        all_drives.truncate(limit);
    }

    if rt.out.json {
        let items: Vec<_> = all_drives
            .iter()
            .map(|d| {
                filter_fields(
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "drive_type": d.drive_type,
                        "site": {"id": site.id, "name": site.display_name, "url": site.web_url},
                    }),
                    fields,
                )
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

/// Filter a JSON object to only the specified fields; pass through if `fields` is empty.
fn filter_fields(mut value: serde_json::Value, fields: &[String]) -> serde_json::Value {
    if fields.is_empty() {
        return value;
    }
    if let serde_json::Value::Object(ref mut map) = value {
        map.retain(|k, _| fields.iter().any(|f| f == k));
    }
    value
}
