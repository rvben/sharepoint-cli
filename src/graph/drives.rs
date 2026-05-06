//! Drive (document library) lookup, item listing, and canonical-shape mapping.

use std::fmt::Write as _;

use serde::Deserialize;

use super::{GraphClient, PagedResponse};
use crate::error::{CliError, Result};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Drive {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(rename = "driveType", default)]
    pub(crate) drive_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DriveItem {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) size: u64,
    #[serde(rename = "eTag", default)]
    pub(crate) etag: Option<String>,
    #[serde(rename = "webUrl", default)]
    pub(crate) web_url: Option<String>,
    #[serde(rename = "createdDateTime", default)]
    pub(crate) created: Option<String>,
    #[serde(rename = "lastModifiedDateTime", default)]
    pub(crate) modified: Option<String>,
    #[serde(rename = "parentReference", default)]
    pub(crate) parent_reference: Option<ParentReference>,
    #[serde(default)]
    pub(crate) folder: Option<Folder>,
    #[serde(default)]
    pub(crate) file: Option<File>,
    /// Pre-authenticated short-lived URL — only populated by `/driveItem`
    /// `?select=...&expand=...` when explicitly requested. We never include
    /// it in canonical_json() output unless the caller asks for `stat`.
    #[serde(rename = "@microsoft.graph.downloadUrl", default)]
    pub(crate) download_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct ParentReference {
    #[serde(rename = "path", default)]
    pub(crate) path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Folder {}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct File {
    #[serde(default)]
    pub(crate) hashes: Hashes,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct Hashes {
    #[serde(rename = "quickXorHash", default)]
    pub(crate) quick_xor: Option<String>,
    #[serde(rename = "sha1Hash", default)]
    pub(crate) sha1: Option<String>,
}

pub(crate) async fn list_drives(graph: &GraphClient, site_id: &str) -> Result<Vec<Drive>> {
    let first_path = format!("/sites/{site_id}/drives");
    graph.page_all(&first_path).await
}

pub(crate) async fn find_drive_by_name(
    graph: &GraphClient,
    site_id: &str,
    name: &str,
) -> Result<Drive> {
    let drives = list_drives(graph, site_id).await?;
    let lower = name.to_ascii_lowercase();
    drives
        .iter()
        .find(|d| d.name.to_ascii_lowercase() == lower)
        .cloned()
        .ok_or_else(|| {
            let available = drives
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            CliError::NotFound(format!(
                "drive (library) '{name}' not found on this site. Available: {available}"
            ))
        })
}

/// Get an item with the `@microsoft.graph.downloadUrl` field populated.
pub(crate) async fn get_item_with_download_url(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
) -> Result<DriveItem> {
    let api_base = if path.is_empty() || path == "/" {
        format!("/drives/{drive_id}/root")
    } else {
        let trimmed = path.trim_start_matches('/');
        let encoded = encode_path_segments(trimmed);
        format!("/drives/{drive_id}/root:/{encoded}")
    };
    let select = "id,name,size,eTag,webUrl,createdDateTime,lastModifiedDateTime,parentReference,folder,file,@microsoft.graph.downloadUrl";
    let api = format!("{api_base}?$select={select}");
    graph.get_json(&api).await
}

pub(crate) struct ListChildrenResult {
    pub(crate) items: Vec<DriveItem>,
    /// The raw `@odata.nextLink` URL returned by Graph, if there are more items.
    pub(crate) next_url: Option<String>,
    /// The URL that was actually fetched (the `page_url` argument resolved to a
    /// full URL). Used by callers that need to encode a mid-page cursor.
    pub(crate) fetched_url: String,
}

/// Fetch one page of children. `page_url` is the full Graph URL to fetch;
/// when `None` the default first-page path is derived from `drive_id` and `path`.
pub(crate) async fn list_children(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
    page_url: Option<&str>,
) -> Result<ListChildrenResult> {
    let api = match page_url {
        Some(url) => url.to_string(),
        None => {
            if path.is_empty() || path == "/" {
                format!("/drives/{drive_id}/root/children")
            } else {
                let trimmed = path.trim_start_matches('/');
                let encoded = encode_path_segments(trimmed);
                format!("/drives/{drive_id}/root:/{encoded}:/children")
            }
        }
    };
    // Resolve to a full absolute URL so the cursor stored in `fetched_url`
    // is always a complete URL (required by the host-validation check on decode).
    let absolute_url = graph.url(&api).await;
    let page: PagedResponse<DriveItem> = graph.get_json(&absolute_url).await?;
    Ok(ListChildrenResult {
        items: page.value,
        next_url: page.next_link,
        fetched_url: absolute_url,
    })
}

pub(crate) async fn list_children_recursive(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
) -> Result<Vec<DriveItem>> {
    let mut out = Vec::new();
    let mut stack = vec![path.to_string()];
    while let Some(p) = stack.pop() {
        let mut next_url: Option<String> = None;
        loop {
            let page = list_children(graph, drive_id, &p, next_url.as_deref()).await?;
            for item in page.items {
                if item.folder.is_some() {
                    let child_path = item_path(&p, &item.name);
                    stack.push(child_path);
                }
                out.push(item);
            }
            next_url = page.next_url;
            if next_url.is_none() {
                break;
            }
        }
    }
    Ok(out)
}

fn item_path(parent: &str, name: &str) -> String {
    if parent.is_empty() || parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}

/// Canonical-shape JSON per spec (every list/show command emits this shape).
pub(crate) fn canonical_json(
    item: &DriveItem,
    site: &super::sites::Site,
    drive: &Drive,
    include_download_url: bool,
) -> serde_json::Value {
    let kind = if item.folder.is_some() {
        "folder"
    } else {
        "file"
    };
    let path = derive_full_path(item);

    let mut hash = serde_json::Map::new();
    if let Some(file) = &item.file {
        if let Some(qx) = &file.hashes.quick_xor {
            hash.insert("quickXor".into(), serde_json::Value::String(qx.clone()));
        }
        if let Some(s) = &file.hashes.sha1 {
            hash.insert("sha1".into(), serde_json::Value::String(s.clone()));
        }
    }

    let mut map = serde_json::Map::new();
    map.insert("id".into(), serde_json::Value::String(item.id.clone()));
    map.insert("name".into(), serde_json::Value::String(item.name.clone()));
    map.insert("path".into(), serde_json::Value::String(path));
    map.insert(
        "site".into(),
        serde_json::json!({
            "id": site.id,
            "name": site.display_name,
            "url": site.web_url,
        }),
    );
    map.insert(
        "drive".into(),
        serde_json::json!({
            "id": drive.id,
            "name": drive.name,
        }),
    );
    map.insert("kind".into(), serde_json::Value::String(kind.into()));
    map.insert("size".into(), serde_json::json!(item.size));
    map.insert("etag".into(), serde_json::json!(item.etag));
    map.insert("created".into(), serde_json::json!(item.created));
    map.insert("modified".into(), serde_json::json!(item.modified));
    map.insert("web_url".into(), serde_json::json!(item.web_url));

    if !hash.is_empty() {
        map.insert("hash".into(), serde_json::Value::Object(hash));
    }
    if include_download_url && let Some(u) = &item.download_url {
        map.insert("download_url".into(), serde_json::Value::String(u.clone()));
    }

    serde_json::Value::Object(map)
}

fn derive_full_path(item: &DriveItem) -> String {
    let parent = item
        .parent_reference
        .as_ref()
        .map(|p| p.path.as_str())
        .unwrap_or("");
    // Graph parent path looks like "/drives/{id}/root:/Folder/Sub". Strip prefix.
    let suffix = parent.split_once(":/").map(|(_, b)| b).unwrap_or("");
    if suffix.is_empty() {
        format!("/{}", item.name)
    } else {
        format!("/{}/{}", suffix, item.name)
    }
}

/// Percent-encodes each path segment using the RFC 3986 unreserved character
/// set, preserving `/` separators so the full path structure is maintained.
pub(super) fn encode_path_segments(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut first = true;
    for seg in path.split('/') {
        if !first {
            out.push('/');
        }
        first = false;
        for b in seg.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => write!(out, "%{b:02X}").unwrap(),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_site() -> super::super::sites::Site {
        super::super::sites::Site {
            id: "S1".into(),
            display_name: "Marketing".into(),
            web_url: "https://contoso.sharepoint.com/sites/Marketing".into(),
        }
    }

    fn fake_drive() -> Drive {
        Drive {
            id: "D1".into(),
            name: "Documents".into(),
            drive_type: "documentLibrary".into(),
        }
    }

    #[test]
    fn canonical_includes_hash_when_file() {
        let item = DriveItem {
            id: "I1".into(),
            name: "plan.pptx".into(),
            size: 100,
            etag: Some("\"abc\"".into()),
            web_url: Some("https://example".into()),
            created: Some("2025-01-01T00:00:00Z".into()),
            modified: Some("2025-02-01T00:00:00Z".into()),
            parent_reference: Some(ParentReference {
                path: "/drives/D1/root:/Folder".into(),
            }),
            folder: None,
            file: Some(File {
                hashes: Hashes {
                    quick_xor: Some("QX".into()),
                    sha1: Some("S1".into()),
                },
            }),
            download_url: None,
        };
        let v = canonical_json(&item, &fake_site(), &fake_drive(), false);
        assert_eq!(v["kind"], "file");
        assert_eq!(v["hash"]["quickXor"], "QX");
        assert_eq!(v["hash"]["sha1"], "S1");
        assert_eq!(v["path"], "/Folder/plan.pptx");
        assert!(v.get("download_url").is_none());
    }

    #[test]
    fn canonical_includes_download_url_only_when_requested() {
        let item = DriveItem {
            id: "I1".into(),
            name: "f".into(),
            size: 0,
            etag: None,
            web_url: None,
            created: None,
            modified: None,
            parent_reference: None,
            folder: None,
            file: Some(File::default()),
            download_url: Some("https://short-lived".into()),
        };
        let with = canonical_json(&item, &fake_site(), &fake_drive(), true);
        assert_eq!(with["download_url"], "https://short-lived");
        let without = canonical_json(&item, &fake_site(), &fake_drive(), false);
        assert!(without.get("download_url").is_none());
    }

    #[test]
    fn item_path_handles_root() {
        assert_eq!(item_path("", "x"), "/x");
        assert_eq!(item_path("/", "x"), "/x");
        assert_eq!(item_path("/A/B", "x"), "/A/B/x");
    }

    #[test]
    fn encode_path_segments_handles_spaces_and_keeps_slashes() {
        assert_eq!(
            encode_path_segments("Marketing Plans/Q1 2025 Deck.pptx"),
            "Marketing%20Plans/Q1%202025%20Deck.pptx"
        );
        assert_eq!(encode_path_segments(""), "");
        assert_eq!(encode_path_segments("a/b/c"), "a/b/c");
    }
}
