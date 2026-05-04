//! Drive (document library) lookup, item listing, and canonical-shape mapping.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::{GraphClient, PagedResponse};
use crate::error::{CliError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Drive {
    pub id: String,
    pub name: String,
    #[serde(rename = "driveType", default)]
    pub drive_type: String,
    #[serde(rename = "webUrl", default)]
    pub web_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "eTag", default)]
    pub etag: Option<String>,
    #[serde(rename = "webUrl", default)]
    pub web_url: Option<String>,
    #[serde(rename = "createdDateTime", default)]
    pub created: Option<String>,
    #[serde(rename = "lastModifiedDateTime", default)]
    pub modified: Option<String>,
    #[serde(rename = "parentReference", default)]
    pub parent_reference: Option<ParentReference>,
    #[serde(default)]
    pub folder: Option<Folder>,
    #[serde(default)]
    pub file: Option<File>,
    /// Pre-authenticated short-lived URL — only populated by `/driveItem`
    /// `?select=...&expand=...` when explicitly requested. We never include
    /// it in canonical_json() output unless the caller asks for `stat`.
    #[serde(rename = "@microsoft.graph.downloadUrl", default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ParentReference {
    #[serde(rename = "driveId", default)]
    pub drive_id: String,
    #[serde(rename = "path", default)]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Folder {
    #[serde(rename = "childCount", default)]
    pub child_count: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct File {
    #[serde(default)]
    pub hashes: Hashes,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Hashes {
    #[serde(rename = "quickXorHash", default)]
    pub quick_xor: Option<String>,
    #[serde(rename = "sha1Hash", default)]
    pub sha1: Option<String>,
}

pub async fn list_drives(graph: &GraphClient, site_id: &str) -> Result<Vec<Drive>> {
    let path = format!("/sites/{site_id}/drives");
    let page: PagedResponse<Drive> = graph.get_json(&path).await?;
    Ok(page.value)
}

pub async fn find_drive_by_name(graph: &GraphClient, site_id: &str, name: &str) -> Result<Drive> {
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

/// `path` here is the leading-slash path within the drive root, e.g. "/folder/file.txt".
/// Empty / "/" means the drive root.
pub async fn get_item(graph: &GraphClient, drive_id: &str, path: &str) -> Result<DriveItem> {
    let api = if path.is_empty() || path == "/" {
        format!("/drives/{drive_id}/root")
    } else {
        let trimmed = path.trim_start_matches('/');
        format!("/drives/{drive_id}/root:/{trimmed}")
    };
    graph.get_json(&api).await
}

/// Get an item with the `@microsoft.graph.downloadUrl` field populated.
pub async fn get_item_with_download_url(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
) -> Result<DriveItem> {
    let api_base = if path.is_empty() || path == "/" {
        format!("/drives/{drive_id}/root")
    } else {
        let trimmed = path.trim_start_matches('/');
        format!("/drives/{drive_id}/root:/{trimmed}")
    };
    let select = "id,name,size,eTag,webUrl,createdDateTime,lastModifiedDateTime,parentReference,folder,file,@microsoft.graph.downloadUrl";
    let api = format!("{api_base}?$select={select}");
    graph.get_json(&api).await
}

pub struct ListChildrenResult {
    pub items: Vec<DriveItem>,
    pub next: Option<String>,
}

pub async fn list_children(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
    page_token: Option<&str>,
) -> Result<ListChildrenResult> {
    let api = match page_token {
        Some(t) => decode_page_token(t)?,
        None => {
            if path.is_empty() || path == "/" {
                format!("/drives/{drive_id}/root/children")
            } else {
                let trimmed = path.trim_start_matches('/');
                format!("/drives/{drive_id}/root:/{trimmed}:/children")
            }
        }
    };
    let page: PagedResponse<DriveItem> = graph.get_json(&api).await?;
    Ok(ListChildrenResult {
        items: page.value,
        next: page.next_link.as_deref().map(encode_page_token),
    })
}

pub async fn list_children_recursive(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
) -> Result<Vec<DriveItem>> {
    let mut out = Vec::new();
    let mut stack = vec![path.to_string()];
    while let Some(p) = stack.pop() {
        let mut next: Option<String> = None;
        loop {
            let page = list_children(graph, drive_id, &p, next.as_deref()).await?;
            for item in page.items {
                if item.folder.is_some() {
                    let child_path = item_path(&p, &item.name);
                    stack.push(child_path);
                }
                out.push(item);
            }
            next = page.next;
            if next.is_none() {
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
pub fn canonical_json(
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
    let mut value = serde_json::json!({
        "id": item.id,
        "name": item.name,
        "path": path,
        "site": {
            "id": site.id,
            "name": site.display_name,
            "url": site.web_url,
        },
        "drive": {
            "id": drive.id,
            "name": drive.name,
        },
        "kind": kind,
        "size": item.size,
        "etag": item.etag,
        "created": item.created,
        "modified": item.modified,
        "web_url": item.web_url,
    });
    if !hash.is_empty() {
        value
            .as_object_mut()
            .unwrap()
            .insert("hash".into(), serde_json::Value::Object(hash));
    }
    if include_download_url && let Some(u) = &item.download_url {
        value
            .as_object_mut()
            .unwrap()
            .insert("download_url".into(), serde_json::Value::String(u.clone()));
    }
    value
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

fn encode_page_token(next_link: &str) -> String {
    URL_SAFE_NO_PAD.encode(next_link.as_bytes())
}

fn decode_page_token(token: &str) -> Result<String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|e| CliError::Input(format!("invalid --page token: {e}")))?;
    String::from_utf8(bytes).map_err(|e| CliError::Input(format!("invalid --page token: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_site() -> super::super::sites::Site {
        super::super::sites::Site {
            id: "S1".into(),
            display_name: "Marketing".into(),
            web_url: "https://contoso.sharepoint.com/sites/Marketing".into(),
            url_segment: "Marketing".into(),
        }
    }

    fn fake_drive() -> Drive {
        Drive {
            id: "D1".into(),
            name: "Documents".into(),
            drive_type: "documentLibrary".into(),
            web_url: String::new(),
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
                drive_id: "D1".into(),
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
}
