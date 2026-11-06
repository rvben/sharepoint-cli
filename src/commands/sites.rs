//! `sharepoint sites list | use`

use crate::backend::Backend;
use crate::cli::{Runtime, SitesCmd};
use crate::config;
use crate::error::{CliError, Result};
use crate::graph::{decode_cursor, encode_cursor, page_cursor, sites};

pub async fn run(rt: &Runtime, cmd: SitesCmd) -> Result<()> {
    match cmd {
        SitesCmd::List {
            query,
            limit,
            all,
            page,
            fields,
        } => list(rt, query.as_deref(), limit, all, page.as_deref(), &fields).await,
        SitesCmd::Use { site } => use_site(rt, &site).await,
    }
}

async fn list(
    rt: &Runtime,
    query: Option<&str>,
    limit: usize,
    all: bool,
    page: Option<&str>,
    fields: &[String],
) -> Result<()> {
    let backend = Backend::from_runtime(rt)?;

    // Decode the incoming page token to a (url, skip) cursor.
    let (mut current_url, mut skip) = if let Some(token) = page {
        let endpoint = backend.cursor_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items = Vec::new();
    let mut source_label: &str;

    let mut seen = std::collections::HashSet::new();
    let out_cursor = 'outer: loop {
        let res = backend.list_sites(query, current_url.as_deref()).await?;
        crate::pagination::record_page(&mut seen, &res.fetched_url)?;
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
                break 'outer page_cursor(&res.fetched_url, res.next_url, idx + 1, res.items.len());
            }
        }
        skip = 0;

        if all {
            match res.next_url {
                Some(url) => current_url = Some(url),
                None => break None,
            }
        } else {
            break res.next_url.map(|url| crate::graph::Cursor {
                next: Some(url),
                skip: 0,
            });
        }
    };

    let next_token = out_cursor.as_ref().map(encode_cursor);
    let total =
        crate::pagination::observed_total(page.is_some(), next_token.is_some(), items.len());
    if rt.out.json {
        let json_items: Vec<_> = items
            .iter()
            .map(|s| {
                filter_fields(
                    serde_json::json!({
                        "id": s.id,
                        "name": s.display_name,
                        "url": s.web_url,
                    }),
                    fields,
                )
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
            .print_message(&format!("({} site(s), source={source_label})", items.len()));
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
