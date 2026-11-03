//! `sharepoint files ls | stat | download | find`

use tokio::fs::File;
use tokio::io::AsyncWriteExt as _;

use crate::backend::Backend;
use crate::cli::{FilesCmd, Runtime};
use crate::error::{CliError, Result};
use crate::graph::drives::{Drive, canonical_json};
use crate::graph::sites::Site;
use crate::graph::{decode_cursor, encode_cursor, page_cursor};
use crate::output::terminal_width;
use crate::reference::{ParsedRef, parse};

/// Resolved reference: everything the sub-commands need to call the backend.
struct Resolved {
    site: Site,
    drive: Drive,
    parsed: ParsedRef,
}

/// Pagination options shared by list-style commands.
struct PageOpts<'a> {
    limit: usize,
    all: bool,
    page: Option<&'a str>,
    fields: &'a [String],
}

/// Parse `reference`, build auth+client, resolve site and drive.
async fn resolve(rt: &Runtime, reference: &str) -> Result<(Backend, Resolved)> {
    let default_site_set = rt.cfg.default_site.is_some();
    let parsed = parse(reference, default_site_set)?;

    let backend = Backend::from_runtime(rt)?;

    let site = backend
        .resolve_site(
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

    let drive = backend.find_drive(&site, library).await?;

    Ok((
        backend,
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
            fields,
        } => {
            ls(
                rt,
                &reference,
                recursive,
                limit,
                all,
                page.as_deref(),
                &fields,
            )
            .await
        }

        FilesCmd::Stat { reference } => stat(rt, &reference).await,

        FilesCmd::Download {
            reference,
            path,
            overwrite,
        } => download(rt, &reference, path.as_deref(), overwrite).await,

        FilesCmd::Find {
            reference,
            query,
            name,
            limit,
            all,
            page,
            fields,
        } => {
            find(
                rt,
                &reference,
                query.as_deref(),
                name.as_deref(),
                PageOpts {
                    limit,
                    all,
                    page: page.as_deref(),
                    fields: &fields,
                },
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
    fields: &[String],
) -> Result<()> {
    if recursive && (limit.is_some() || all || page.is_some()) {
        return Err(CliError::Input(
            "`--recursive` cannot be combined with `--limit`/`--all`/`--page`; it always returns the full tree".into(),
        ));
    }

    let (backend, r) = resolve(rt, reference).await?;

    if recursive {
        let items = backend
            .list_children_recursive(&r.site, &r.drive, &r.parsed.path)
            .await?;

        if rt.out.json {
            let json_items: Vec<_> = items
                .iter()
                .map(|it| filter_fields(canonical_json(it, &r.site, &r.drive, false), fields))
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
        let endpoint = backend.cursor_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items: Vec<_> = Vec::new();

    let mut seen = std::collections::HashSet::new();
    let out_cursor = 'outer: loop {
        let pageres = backend
            .list_children(&r.site, &r.drive, &r.parsed.path, current_url.as_deref())
            .await?;
        crate::pagination::record_page(&mut seen, &pageres.fetched_url)?;

        for (idx, it) in pageres.items.iter().enumerate() {
            if idx < skip {
                continue;
            }
            items.push(it.clone());
            if !all && items.len() >= effective_limit {
                break 'outer page_cursor(
                    &pageres.fetched_url,
                    pageres.next_url,
                    idx + 1,
                    pageres.items.len(),
                );
            }
        }
        skip = 0;

        match pageres.next_url {
            None => break None,
            Some(url) => current_url = Some(url),
        }
    };

    let next_token = out_cursor.as_ref().map(encode_cursor);

    if rt.out.json {
        let json_items: Vec<_> = items
            .iter()
            .map(|it| filter_fields(canonical_json(it, &r.site, &r.drive, false), fields))
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
    let (backend, r) = resolve(rt, reference).await?;
    let item = backend.get_item(&r.site, &r.drive, &r.parsed.path).await?;
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
    let (backend, r) = resolve(rt, reference).await?;

    // Derive the target filename when --path is not given.
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
        let bytes = backend
            .download_to_writer(&r.site, &r.drive, &r.parsed.path, &mut stdout)
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

    // Stage beside the destination so committing the completed download stays
    // on the same filesystem. Dropping the tempfile cleans up failed transfers.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".sharepoint-download-")
        .tempfile_in(parent)
        .map_err(|e| CliError::Other(format!("stage download for '{target}': {e}")))?;
    let mut file = File::from_std(
        staged
            .reopen()
            .map_err(|e| CliError::Other(format!("open staged download: {e}")))?,
    );
    let n = backend
        .download_to_writer(&r.site, &r.drive, &r.parsed.path, &mut file)
        .await?;
    file.sync_all()
        .await
        .map_err(|e| CliError::Other(format!("sync download: {e}")))?;
    drop(file);
    // persist_noclobber also protects files created while the transfer ran;
    // an existence check alone would allow a race to overwrite user data.
    let committed = if overwrite {
        staged.persist(path)
    } else {
        staged.persist_noclobber(path)
    };
    committed.map_err(|e| {
        if !overwrite && e.error.kind() == std::io::ErrorKind::AlreadyExists {
            CliError::Input(format!(
                "'{target}' already exists; use --overwrite to replace it"
            ))
        } else {
            CliError::Other(format!("save download to '{target}': {}", e.error))
        }
    })?;
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
    opts: PageOpts<'_>,
) -> Result<()> {
    let PageOpts {
        limit,
        all,
        page,
        fields,
    } = opts;
    let (backend, r) = resolve(rt, reference).await?;
    // Permissive default query when only --name is given (Graph requires a query).
    let q = query.unwrap_or("*");

    // Decode the incoming page token to a (url, skip) cursor.
    let (mut current_url, mut skip) = if let Some(token) = page {
        let endpoint = backend.cursor_endpoint().await;
        let cursor = decode_cursor(&endpoint, token)?;
        (cursor.next, cursor.skip)
    } else {
        (None, 0)
    };

    let mut items = Vec::new();

    let mut seen = std::collections::HashSet::new();
    let out_cursor = 'outer: loop {
        let res = backend.search(&r.drive, q, current_url.as_deref()).await?;
        crate::pagination::record_page(&mut seen, &res.fetched_url)?;

        for (idx, it) in res.items.iter().enumerate() {
            if idx < skip {
                continue;
            }
            // Apply optional glob filter without counting filtered items toward skip.
            if let Some(g) = name_glob
                && !crate::graph::search::glob_matches(g, &it.name)
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
    let total = items.len();
    if rt.out.json {
        let json_items: Vec<_> = items
            .iter()
            .map(|it| filter_fields(canonical_json(it, &r.site, &r.drive, false), fields))
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

/// Filter a JSON object to only include the specified fields.
/// When `fields` is empty, return the object unchanged.
fn filter_fields(mut value: serde_json::Value, fields: &[String]) -> serde_json::Value {
    if fields.is_empty() {
        return value;
    }
    if let serde_json::Value::Object(ref mut map) = value {
        map.retain(|k, _| fields.iter().any(|f| f == k));
    }
    value
}
