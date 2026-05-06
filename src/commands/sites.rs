//! `sharepoint sites list | use`

use crate::auth::AuthContext;
use crate::cli::{Runtime, SitesCmd};
use crate::config;
use crate::error::{CliError, Result};
use crate::graph::{Cursor, GraphClient, decode_cursor, encode_cursor, sites};

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

    // Decode the incoming page token to a (url, skip) cursor.
    let (mut current_url, mut skip) = if let Some(token) = page {
        let endpoint = graph.graph_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items = Vec::new();
    let mut source_label: &str;

    let out_cursor: Option<Cursor> = 'outer: loop {
        let res = sites::list(&graph, query, current_url.as_deref()).await?;
        source_label = match res.source {
            sites::SiteListSource::Followed => "followed",
            sites::SiteListSource::Search => "search",
        };

        for (idx, s) in res.items.iter().enumerate() {
            if idx < skip {
                continue;
            }
            items.push(s.clone());
            if !all && items.len() >= limit {
                // Mid-page: cursor points back at the same URL with updated skip.
                let consumed_in_page = idx + 1;
                break 'outer Some(Cursor {
                    next: Some(res.fetched_url),
                    skip: consumed_in_page,
                });
            }
        }
        skip = 0;

        if all {
            if res.next_url.is_none() {
                // Exhausted.
                break None;
            }
            current_url = res.next_url;
        } else {
            // Not --all: emit next cursor pointing at the Graph nextLink.
            break res.next_url.map(|url| Cursor {
                next: Some(url),
                skip: 0,
            });
        }
    };

    let next_token = out_cursor.as_ref().map(encode_cursor);
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
