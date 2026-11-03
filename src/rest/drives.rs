//! Document libraries and folder listing over SharePoint REST.
//!
//! Items are mapped onto the Graph `DriveItem` shape so `canonical_json`
//! works unchanged. `parent_reference.path` is set to `rest:/<path-in-library>`
//! so `derive_full_path` (which splits on `:/`) yields the library-relative path.

use serde::{Deserialize, Deserializer};

use super::{RestClient, odata_path_literal};
use crate::error::{CliError, Result};
use crate::graph::drives::{Drive, DriveItem, File, Folder, ListChildrenResult, ParentReference};
use crate::graph::sites::Site;

#[derive(Deserialize)]
struct ListsResponse {
    value: Vec<List>,
}

#[derive(Deserialize)]
struct List {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Title")]
    title: String,
    #[serde(rename = "RootFolder")]
    root_folder: RootFolder,
}

#[derive(Deserialize)]
struct RootFolder {
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
}

#[derive(Deserialize)]
struct Collection<T> {
    value: Vec<T>,
}

#[derive(Deserialize)]
struct RestFolder {
    #[serde(rename = "UniqueId")]
    unique_id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
    #[serde(rename = "TimeCreated", default)]
    time_created: Option<String>,
    #[serde(rename = "TimeLastModified", default)]
    time_last_modified: Option<String>,
}

#[derive(Deserialize)]
struct RestFile {
    #[serde(rename = "UniqueId")]
    unique_id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ServerRelativeUrl")]
    server_relative_url: String,
    #[serde(rename = "Length", default, deserialize_with = "de_lenient_u64")]
    length: u64,
    #[serde(rename = "ETag", default)]
    etag: Option<String>,
    #[serde(rename = "TimeCreated", default)]
    time_created: Option<String>,
    #[serde(rename = "TimeLastModified", default)]
    time_last_modified: Option<String>,
}

/// SharePoint returns `Length` as a JSON string; accept string or number.
fn de_lenient_u64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum N {
        S(String),
        U(u64),
    }
    match N::deserialize(d)? {
        N::U(n) => Ok(n),
        N::S(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

const FOLDER_SELECT: &str = "UniqueId,Name,ServerRelativeUrl,TimeCreated,TimeLastModified";
const FILE_SELECT: &str =
    "UniqueId,Name,ServerRelativeUrl,Length,ETag,TimeCreated,TimeLastModified";

pub(crate) async fn list_drives(client: &RestClient, site: &Site) -> Result<Vec<Drive>> {
    let url = client.site_api(
        &site.web_url,
        "/_api/web/lists?$filter=BaseTemplate%20eq%20101%20and%20Hidden%20eq%20false&$select=Id,Title,RootFolder/ServerRelativeUrl&$expand=RootFolder",
    )?;
    let resp: ListsResponse = client.get_json(&url).await?;
    Ok(resp
        .value
        .into_iter()
        .map(|l| Drive {
            id: l.id,
            name: l.title,
            drive_type: "documentLibrary".into(),
            root_folder: Some(l.root_folder.server_relative_url),
        })
        .collect())
}

pub(crate) async fn find_drive_by_name(
    client: &RestClient,
    site: &Site,
    name: &str,
) -> Result<Drive> {
    let drives = list_drives(client, site).await?;
    drives
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
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

/// Server-relative URL for `path` inside `drive`.
pub(crate) fn server_relative(drive: &Drive, path: &str) -> Result<String> {
    let root = drive
        .root_folder
        .as_deref()
        .ok_or_else(|| CliError::Other("library root folder unknown".into()))?
        .trim_end_matches('/');
    let rel = path.trim_matches('/');
    Ok(if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    })
}

/// Library-relative parent path (`/A/B`) of a server-relative item URL.
fn parent_in_library(drive: &Drive, item_server_relative: &str) -> String {
    let root = drive
        .root_folder
        .as_deref()
        .unwrap_or("")
        .trim_end_matches('/');
    let rel = item_server_relative
        .strip_prefix(root)
        .unwrap_or(item_server_relative);
    match rel.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// The library root contains the hidden system folder "Forms". Decided from
/// the folder's own URL (not from how the listing was requested) so a
/// mid-folder `--page` cursor sees the same items as the first page did.
fn is_library_forms_folder(drive: &Drive, f: &RestFolder) -> bool {
    f.name == "Forms" && parent_in_library(drive, &f.server_relative_url).is_empty()
}

fn parent_ref(drive: &Drive, server_relative_url: &str) -> Option<ParentReference> {
    Some(ParentReference {
        path: format!("rest:{}", parent_in_library(drive, server_relative_url)),
    })
}

fn folder_item(client: &RestClient, drive: &Drive, f: RestFolder) -> DriveItem {
    DriveItem {
        parent_reference: parent_ref(drive, &f.server_relative_url),
        web_url: Some(client.public_url(&f.server_relative_url)),
        id: f.unique_id,
        name: f.name,
        size: 0,
        etag: None,
        created: f.time_created,
        modified: f.time_last_modified,
        folder: Some(Folder {}),
        file: None,
        download_url: None,
    }
}

fn file_item(client: &RestClient, drive: &Drive, f: RestFile) -> DriveItem {
    DriveItem {
        parent_reference: parent_ref(drive, &f.server_relative_url),
        web_url: Some(client.public_url(&f.server_relative_url)),
        id: f.unique_id,
        name: f.name,
        size: f.length,
        etag: f.etag,
        created: f.time_created,
        modified: f.time_last_modified,
        folder: None,
        file: Some(File::default()),
        download_url: None,
    }
}

fn folder_api(client: &RestClient, site: &Site, server_relative: &str) -> Result<String> {
    client.site_api(
        &site.web_url,
        &format!(
            "/_api/web/GetFolderByServerRelativePath(decodedurl='{}')",
            odata_path_literal(server_relative)
        ),
    )
}

/// One "page" of children. REST returns all folders and files of a folder in
/// one response each, so `next_url` is always `None`. `fetched_url` is the
/// folder URL; passing it back as `page_url` re-lists the same folder (used
/// by mid-page `--limit` cursors).
pub(crate) async fn list_children(
    client: &RestClient,
    site: &Site,
    drive: &Drive,
    path: &str,
    page_url: Option<&str>,
) -> Result<ListChildrenResult> {
    let folder_url = match page_url {
        Some(u) => u.to_string(),
        None => folder_api(client, site, &server_relative(drive, path)?)?,
    };
    let folders: Collection<RestFolder> = client
        .get_json(&format!("{folder_url}/Folders?$select={FOLDER_SELECT}"))
        .await?;
    let files: Collection<RestFile> = client
        .get_json(&format!("{folder_url}/Files?$select={FILE_SELECT}"))
        .await?;
    let mut items: Vec<DriveItem> = folders
        .value
        .into_iter()
        .filter(|f| !is_library_forms_folder(drive, f))
        .map(|f| folder_item(client, drive, f))
        .collect();
    items.extend(files.value.into_iter().map(|f| file_item(client, drive, f)));
    Ok(ListChildrenResult {
        items,
        next_url: None,
        fetched_url: folder_url,
    })
}

pub(crate) async fn list_children_recursive(
    client: &RestClient,
    site: &Site,
    drive: &Drive,
    path: &str,
) -> Result<Vec<DriveItem>> {
    let mut out = Vec::new();
    let mut stack = vec![path.trim_end_matches('/').to_string()];
    while let Some(p) = stack.pop() {
        let page = list_children(client, site, drive, &p, None).await?;
        for item in page.items {
            if item.folder.is_some() {
                stack.push(format!("{p}/{}", item.name));
            }
            out.push(item);
        }
    }
    Ok(out)
}

/// Metadata for a file, or for a folder when no file exists at `path`.
pub(crate) async fn get_item(
    client: &RestClient,
    site: &Site,
    drive: &Drive,
    path: &str,
) -> Result<DriveItem> {
    let sr = server_relative(drive, path)?;
    let file_url = client.site_api(
        &site.web_url,
        &format!(
            "/_api/web/GetFileByServerRelativePath(decodedurl='{}')?$select={FILE_SELECT}",
            odata_path_literal(&sr)
        ),
    )?;
    match client.get_json::<RestFile>(&file_url).await {
        Ok(f) => Ok(file_item(client, drive, f)),
        Err(CliError::NotFound(_)) => {
            let url = format!("{}?$select={FOLDER_SELECT}", folder_api(client, site, &sr)?);
            let f: RestFolder = client.get_json(&url).await?;
            Ok(folder_item(client, drive, f))
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive() -> Drive {
        Drive {
            id: "L1".into(),
            name: "Documents".into(),
            drive_type: "documentLibrary".into(),
            root_folder: Some("/sites/M/Shared Documents".into()),
        }
    }

    fn folder(name: &str, server_relative_url: &str) -> RestFolder {
        RestFolder {
            unique_id: "F".into(),
            name: name.into(),
            server_relative_url: server_relative_url.into(),
            time_created: None,
            time_last_modified: None,
        }
    }

    #[test]
    fn server_relative_joins_root_and_path() {
        assert_eq!(
            server_relative(&drive(), "").unwrap(),
            "/sites/M/Shared Documents"
        );
        assert_eq!(
            server_relative(&drive(), "/A/b.txt").unwrap(),
            "/sites/M/Shared Documents/A/b.txt"
        );
    }

    #[test]
    fn parent_in_library_strips_root() {
        assert_eq!(
            parent_in_library(&drive(), "/sites/M/Shared Documents/A/B/c.txt"),
            "/A/B"
        );
        assert_eq!(
            parent_in_library(&drive(), "/sites/M/Shared Documents/c.txt"),
            ""
        );
    }

    #[test]
    fn only_root_forms_folder_is_hidden() {
        assert!(is_library_forms_folder(
            &drive(),
            &folder("Forms", "/sites/M/Shared Documents/Forms")
        ));
        assert!(!is_library_forms_folder(
            &drive(),
            &folder("Forms", "/sites/M/Shared Documents/A/Forms")
        ));
        assert!(!is_library_forms_folder(
            &drive(),
            &folder("Plans", "/sites/M/Shared Documents/Plans")
        ));
    }

    #[test]
    fn file_length_accepts_string() {
        let f: RestFile = serde_json::from_str(
            r#"{"UniqueId":"U","Name":"a","ServerRelativeUrl":"/x/a","Length":"42"}"#,
        )
        .unwrap();
        assert_eq!(f.length, 42);
    }

    #[test]
    fn canonical_path_matches_graph_shape() {
        let c = RestClient::with_host("http://127.0.0.1:1", "contoso.sharepoint.com", "c").unwrap();
        let f: RestFile = serde_json::from_str(
            r#"{"UniqueId":"U","Name":"c.txt","ServerRelativeUrl":"/sites/M/Shared Documents/A/c.txt","Length":"1"}"#,
        )
        .unwrap();
        let item = file_item(&c, &drive(), f);
        let site = Site {
            id: "W".into(),
            display_name: "M".into(),
            web_url: "https://contoso.sharepoint.com/sites/M".into(),
        };
        let v = crate::graph::drives::canonical_json(&item, &site, &drive(), false);
        assert_eq!(v["path"], "/A/c.txt");
        assert_eq!(
            v["web_url"],
            "https://contoso.sharepoint.com/sites/M/Shared Documents/A/c.txt"
        );
    }
}
