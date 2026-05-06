//! `sharepoint files ls | stat | download | find`

use tokio::fs::File;
use tokio::io::AsyncWriteExt as _;

use crate::auth::AuthContext;
use crate::cli::{FilesCmd, Runtime};
use crate::error::{CliError, Result};
use crate::graph::drives::{Drive, canonical_json};
use crate::graph::sites::Site;
use crate::graph::{Cursor, GraphClient, decode_cursor, drives, encode_cursor, search};
use crate::output::terminal_width;
use crate::reference::{ParsedRef, parse};

/// Resolved reference: everything the sub-commands need to call Graph.
struct Resolved {
    site: Site,
    drive: Drive,
    parsed: ParsedRef,
}

/// Parse `reference`, build auth+client, resolve site and drive.
async fn resolve(rt: &Runtime, reference: &str) -> Result<(GraphClient, Resolved)> {
    let default_site_set = rt.cfg.default_site.is_some();
    let parsed = parse(reference, default_site_set)?;

    let auth = AuthContext::new(rt.cfg.clone(), rt.cache_path.clone());
    let graph = GraphClient::new(auth);

    let site = crate::graph::sites::resolve(
        &graph,
        &parsed.site,
        &rt.cfg.site_aliases,
        rt.cfg.default_site.as_deref(),
    )
    .await?;

    let library = parsed.library.as_deref().ok_or_else(|| {
        CliError::Input(format!(
            "reference '{reference}' has no library; use 'Site:Library/path'"
        ))
    })?;

    let drive = drives::find_drive_by_name(&graph, &site.id, library).await?;

    Ok((
        graph,
        Resolved {
            site,
            drive,
            parsed,
        },
    ))
}

pub async fn run(rt: &Runtime, cmd: FilesCmd) -> Result<()> {
    match cmd {
        FilesCmd::Ls {
            reference,
            recursive,
            limit,
            all,
            page,
        } => ls(rt, &reference, recursive, limit, all, page.as_deref()).await,

        FilesCmd::Stat { reference } => stat(rt, &reference).await,

        FilesCmd::Download {
            reference,
            output,
            overwrite,
        } => download(rt, &reference, output.as_deref(), overwrite).await,

        FilesCmd::Find {
            reference,
            query,
            name,
            limit,
            all,
            page,
        } => {
            find(
                rt,
                &reference,
                query.as_deref(),
                name.as_deref(),
                limit,
                all,
                page.as_deref(),
            )
            .await
        }
    }
}

const DEFAULT_LIMIT: usize = 200;

async fn ls(
    rt: &Runtime,
    reference: &str,
    recursive: bool,
    limit: Option<usize>,
    all: bool,
    page: Option<&str>,
) -> Result<()> {
    if recursive && (limit.is_some() || all || page.is_some()) {
        return Err(CliError::Input(
            "`--recursive` cannot be combined with `--limit`/`--all`/`--page`; it always returns the full tree".into(),
        ));
    }

    let (graph, r) = resolve(rt, reference).await?;

    if recursive {
        let items = drives::list_children_recursive(&graph, &r.drive.id, &r.parsed.path).await?;

        if rt.out.json {
            let json_items: Vec<_> = items
                .iter()
                .map(|it| canonical_json(it, &r.site, &r.drive, false))
                .collect();
            rt.out.print_json(&serde_json::json!({
                "total": json_items.len(),
                "next": null,
                "items": json_items,
            }));
        } else {
            let name_w = terminal_width().saturating_sub(40).max(20);
            rt.out.print_data(&format!(
                "{:<name_w$}  {:<6}  {:>10}  {:<16}",
                "NAME", "KIND", "SIZE", "MODIFIED"
            ));
            for it in &items {
                let kind = if it.folder.is_some() {
                    "folder"
                } else {
                    "file"
                };
                let modified = it.modified.as_deref().unwrap_or("");
                let modified_short = modified
                    .replace('T', " ")
                    .chars()
                    .take(16)
                    .collect::<String>();
                rt.out.print_data(&format!(
                    "{:<name_w$}  {:<6}  {:>10}  {:<16}",
                    it.name, kind, it.size, modified_short
                ));
            }
            rt.out.print_message(&format!("({} item(s))", items.len()));
        }
        return Ok(());
    }

    // Paginated single-level listing.
    let effective_limit = limit.unwrap_or(DEFAULT_LIMIT);
    let (mut current_url, mut skip) = if let Some(token) = page {
        let endpoint = graph.graph_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items: Vec<_> = Vec::new();

    let out_cursor: Option<Cursor> = 'outer: loop {
        let pageres =
            drives::list_children(&graph, &r.drive.id, &r.parsed.path, current_url.as_deref())
                .await?;

        for (idx, it) in pageres.items.iter().enumerate() {
            if idx < skip {
                continue;
            }
            items.push(it.clone());
            if !all && items.len() >= effective_limit {
                // Mid-page: point the cursor back at the same page with updated skip.
                let consumed_in_page = idx + 1;
                break 'outer Some(Cursor {
                    next: Some(pageres.fetched_url),
                    skip: consumed_in_page,
                });
            }
        }
        skip = 0;

        match pageres.next_url {
            None => break None,
            Some(url) if !all && items.len() >= effective_limit => {
                break Some(Cursor {
                    next: Some(url),
                    skip: 0,
                });
            }
            Some(url) => current_url = Some(url),
        }
    };

    let next_token = out_cursor.as_ref().map(encode_cursor);

    if rt.out.json {
        let json_items: Vec<_> = items
            .iter()
            .map(|it| canonical_json(it, &r.site, &r.drive, false))
            .collect();
        rt.out.print_json(&serde_json::json!({
            "total": json_items.len(),
            "next": next_token,
            "items": json_items,
        }));
    } else {
        let name_w = terminal_width().saturating_sub(40).max(20);
        rt.out.print_data(&format!(
            "{:<name_w$}  {:<6}  {:>10}  {:<16}",
            "NAME", "KIND", "SIZE", "MODIFIED"
        ));
        for it in &items {
            let kind = if it.folder.is_some() {
                "folder"
            } else {
                "file"
            };
            let modified = it.modified.as_deref().unwrap_or("");
            let modified_short = modified
                .replace('T', " ")
                .chars()
                .take(16)
                .collect::<String>();
            rt.out.print_data(&format!(
                "{:<name_w$}  {:<6}  {:>10}  {:<16}",
                it.name, kind, it.size, modified_short
            ));
        }
        rt.out.print_message(&format!("({} item(s))", items.len()));
    }
    Ok(())
}

async fn stat(rt: &Runtime, reference: &str) -> Result<()> {
    let (graph, r) = resolve(rt, reference).await?;
    let item = drives::get_item_with_download_url(&graph, &r.drive.id, &r.parsed.path).await?;
    let v = canonical_json(&item, &r.site, &r.drive, true);
    // stat is data-heavy: emit canonical JSON in both modes (human readers
    // benefit from the structured form when inspecting metadata).
    rt.out.print_json(&v);
    Ok(())
}

async fn download(
    rt: &Runtime,
    reference: &str,
    output: Option<&str>,
    overwrite: bool,
) -> Result<()> {
    let (graph, r) = resolve(rt, reference).await?;

    // Derive the target filename from the path when --output is not given.
    let derived;
    let target: &str = match output {
        Some(o) => o,
        None => {
            derived = r
                .parsed
                .path
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("download")
                .to_string();
            &derived
        }
    };

    if target == "-" {
        let mut stdout = tokio::io::stdout();
        let bytes = crate::graph::download::download_to_writer(
            &graph,
            &r.drive.id,
            &r.parsed.path,
            &mut stdout,
        )
        .await?;
        stdout.flush().await.ok();
        rt.out
            .print_message(&format!("{bytes} bytes written to stdout"));
        return Ok(());
    }

    let path = std::path::Path::new(target);
    if path.exists() && !overwrite {
        return Err(CliError::Input(format!(
            "'{target}' already exists; use --overwrite to replace it"
        )));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            CliError::Other(format!("create directory '{}': {e}", parent.display()))
        })?;
    }

    let mut file = File::create(path)
        .await
        .map_err(|e| CliError::Other(format!("create '{target}': {e}")))?;

    let n =
        crate::graph::download::download_to_writer(&graph, &r.drive.id, &r.parsed.path, &mut file)
            .await?;
    rt.out
        .print_message(&format!("Downloaded {n} byte(s) to {}", path.display()));
    if rt.out.json {
        rt.out.print_json(&serde_json::json!({
            "path": path.display().to_string(),
            "bytes": n,
        }));
    }
    Ok(())
}

async fn find(
    rt: &Runtime,
    reference: &str,
    query: Option<&str>,
    name_glob: Option<&str>,
    limit: usize,
    all: bool,
    page: Option<&str>,
) -> Result<()> {
    let (graph, r) = resolve(rt, reference).await?;
    // Permissive default query when only --name is given (Graph requires a query).
    let q = query.unwrap_or("*");

    // Decode the incoming page token to a (url, skip) cursor.
    let (mut current_url, mut skip) = if let Some(token) = page {
        let endpoint = graph.graph_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items = Vec::new();

    let out_cursor: Option<Cursor> = 'outer: loop {
        let res = search::search(&graph, &r.drive.id, q, current_url.as_deref()).await?;

        for (idx, it) in res.items.iter().enumerate() {
            if idx < skip {
                continue;
            }
            // Apply optional glob filter without counting filtered items toward skip.
            if let Some(g) = name_glob
                && !search::glob_matches(g, &it.name)
            {
                continue;
            }
            items.push(it.clone());
            if !all && items.len() >= limit {
                // Mid-page: note how many raw items from this page we consumed.
                // `skip` is a raw-item index, not a post-filter count. Resume correctness
                // relies on the glob filter being stateless: re-applying it to the same
                // items on the resumed page yields identical filter decisions, so no item
                // is double-emitted or skipped.
                let consumed_raw = idx + 1;
                break 'outer Some(Cursor {
                    next: Some(res.fetched_url),
                    skip: consumed_raw,
                });
            }
        }
        skip = 0;

        if all {
            if res.next_url.is_none() {
                break None;
            }
            current_url = res.next_url;
        } else {
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
            .map(|it| canonical_json(it, &r.site, &r.drive, false))
            .collect();
        rt.out.print_json(&serde_json::json!({
            "total": total,
            "next": next_token,
            "items": json_items,
        }));
    } else {
        for it in &items {
            rt.out.print_data(&it.name);
        }
        rt.out.print_message(&format!("({total} match(es))"));
    }
    Ok(())
}
