//! `sharepoint sites list | use`

use crate::auth::AuthContext;
use crate::cli::{Runtime, SitesCmd};
use crate::config;
use crate::error::{CliError, Result};
use crate::graph::{GraphClient, sites};

pub async fn run(rt: &Runtime, cmd: SitesCmd) -> Result<()> {
    match cmd {
        SitesCmd::List {
            query,
            limit,
            all,
            page,
        } => list(rt, query.as_deref(), limit, all, page.as_deref()).await,
        SitesCmd::Use { site } => use_site(rt, &site).await,
    }
}

async fn list(
    rt: &Runtime,
    query: Option<&str>,
    limit: usize,
    all: bool,
    page: Option<&str>,
) -> Result<()> {
    let auth = AuthContext::new(rt.cfg.clone(), rt.cache_path.clone());
    let graph = GraphClient::new(auth);

    let mut items = Vec::new();
    let mut next_token: Option<String> = page.map(String::from);
    let mut source_label;
    loop {
        let res = sites::list(&graph, query, next_token.as_deref()).await?;
        source_label = match res.source {
            sites::SiteListSource::Followed => "followed",
            sites::SiteListSource::Search => "search",
        };
        for s in res.items {
            items.push(s);
            if !all && items.len() >= limit {
                break;
            }
        }
        if !all || res.next.is_none() {
            next_token = if all { None } else { res.next };
            break;
        }
        next_token = res.next;
    }

    let total = items.len();
    if rt.out.json {
        let json_items: Vec<_> = items
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "name": s.display_name,
                    "url": s.web_url,
                })
            })
            .collect();
        rt.out.print_json(&serde_json::json!({
            "total": total,
            "next": next_token,
            "source": source_label,
            "items": json_items,
        }));
    } else {
        for s in &items {
            rt.out
                .print_data(&format!("{:40}  {}", s.display_name, s.web_url));
        }
        rt.out
            .print_message(&format!("({total} site(s), source={source_label})"));
    }
    Ok(())
}

async fn use_site(rt: &Runtime, value: &str) -> Result<()> {
    if rt.cfg.read_only {
        return Err(CliError::ReadOnly(
            "sites use modifies the config file; not allowed in read-only mode".into(),
        ));
    }
    let mut file = rt.config_file.clone();
    let entry = file.profile.entry(rt.cfg.profile_name.clone()).or_default();
    entry.default_site = Some(value.to_string());
    config::save_file(&rt.config_path, &file)?;
    rt.out.print_message(&format!(
        "Set default_site for profile '{}' to '{}'",
        rt.cfg.profile_name, value
    ));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "profile": rt.cfg.profile_name,
            "default_site": value,
        }));
    }
    Ok(())
}
