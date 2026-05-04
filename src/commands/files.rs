//! `sharepoint files ls | stat | download | find`

use std::io::Write as _;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::fs::File;
use tokio::io::{AsyncWrite, AsyncWriteExt as _};

use crate::auth::AuthContext;
use crate::cli::{FilesCmd, Runtime};
use crate::error::{CliError, Result};
use crate::graph::drives::{Drive, canonical_json};
use crate::graph::sites::Site;
use crate::graph::{GraphClient, drives, search};
use crate::output::terminal_width;
use crate::reference::{ParsedRef, parse};

/// Synchronous-stdout adapter: wraps `std::io::Stdout` to satisfy `AsyncWrite`.
///
/// This is safe for the download-to-stdout path because stdout writes are
/// inherently blocking (no async kernel interface on most platforms). Each
/// `poll_write` call acquires the global stdout lock, writes synchronously, and
/// returns `Poll::Ready`. This matches the behaviour of Tokio's built-in
/// `tokio::io::stdout()` (which is only available when the `io-std` feature is
/// enabled, and that feature is not in our dependency tree).
struct StdoutWriter(std::io::Stdout);

impl AsyncWrite for StdoutWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(self.get_mut().0.write(buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(self.get_mut().0.flush())
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

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

async fn ls(
    rt: &Runtime,
    reference: &str,
    recursive: bool,
    limit: usize,
    all: bool,
    page: Option<&str>,
) -> Result<()> {
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
    let mut items: Vec<_> = Vec::new();
    let mut next = page.map(str::to_owned);
    loop {
        let pageres =
            drives::list_children(&graph, &r.drive.id, &r.parsed.path, next.as_deref()).await?;
        for it in pageres.items {
            items.push(it);
            if !all && items.len() >= limit {
                break;
            }
        }
        if !all || pageres.next.is_none() {
            next = if all { None } else { pageres.next };
            break;
        }
        next = pageres.next;
    }
    let next_token = next;

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
        let mut stdout = StdoutWriter(std::io::stdout());
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
    let q = query.unwrap_or("*");
    let (graph, r) = resolve(rt, reference).await?;

    let res = search::search(&graph, &r.drive.id, q, page).await?;
    let mut items = res.items;
    let next_token = res.next;

    // Apply glob filter before limit truncation so the user gets `limit` matching items.
    if let Some(glob) = name_glob {
        items.retain(|it| search::glob_matches(glob, &it.name));
    }
    if !all && items.len() > limit {
        items.truncate(limit);
    }

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
        let total = items.len();
        for it in &items {
            rt.out.print_data(&it.name);
        }
        rt.out.print_message(&format!("({total} match(es))"));
    }
    Ok(())
}
