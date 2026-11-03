//! Library-scoped search via the SharePoint search REST API (KQL).

use super::{RestClient, SEARCH_ROW_LIMIT, percent_encode_keep_slash, quoted, search_page};
use crate::error::Result;
use crate::graph::drives::{Drive, DriveItem, File, ParentReference};
use crate::graph::search::SearchResult;

pub(crate) fn build_url(client: &RestClient, drive: &Drive, query: &str) -> String {
    let library_url = client.public_url(drive.root_folder.as_deref().unwrap_or(""));
    // Parenthesise the user query so an `OR` inside it cannot escape the library scope.
    let kql = format!("({query}) path:\"{library_url}\" IsDocument:1");
    client.root_api(&format!(
        "/_api/search/query?querytext={}&selectproperties=%27Title,Path,FileName,Size,LastModifiedTime,UniqueId%27&rowlimit={SEARCH_ROW_LIMIT}&startrow=0",
        percent_encode_keep_slash(&quoted(&kql))
    ))
}

pub(crate) async fn search(
    client: &RestClient,
    drive: &Drive,
    query: &str,
    page_url: Option<&str>,
) -> Result<SearchResult> {
    let url = page_url.map_or_else(|| build_url(client, drive, query), str::to_owned);
    let page = search_page(client, &url).await?;
    let library_prefix = format!(
        "{}/",
        client.public_url(drive.root_folder.as_deref().unwrap_or(""))
    );
    let items = page
        .rows
        .into_iter()
        .filter_map(|r| {
            let full = r.get("Path")?.clone();
            let name = r
                .get("FileName")
                .cloned()
                .unwrap_or_else(|| full.rsplit('/').next().unwrap_or_default().to_string());
            // Drop rows outside the library rather than emit a garbled path.
            let in_lib = strip_prefix_ignore_case(&full, &library_prefix)?;
            let parent = in_lib
                .rsplit_once('/')
                .map(|(p, _)| format!("/{p}"))
                .unwrap_or_default();
            Some(DriveItem {
                id: r
                    .get("UniqueId")
                    .map(|id| id.trim_matches(|c| c == '{' || c == '}').to_string())
                    .unwrap_or_default(),
                name,
                size: r.get("Size").and_then(|s| s.parse().ok()).unwrap_or(0),
                etag: None,
                web_url: Some(full),
                created: None,
                modified: r.get("LastModifiedTime").cloned().map(normalize_modified),
                parent_reference: Some(ParentReference {
                    path: format!("rest:{parent}"),
                }),
                folder: None,
                file: Some(File::default()),
                download_url: None,
            })
        })
        .collect();
    Ok(SearchResult {
        items,
        next_url: page.next_url,
        fetched_url: url,
    })
}

/// `value` without `prefix`, compared ASCII case-insensitively; `None` if it doesn't match.
fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

/// Search returns timestamps like `2016-05-16T15:59:28.0000000Z`; strip an
/// all-zero fractional-seconds suffix so they match the `TimeLastModified`
/// format the non-search REST paths produce.
fn normalize_modified(value: String) -> String {
    match value.split_once('.') {
        Some((head, tail))
            if tail.ends_with('Z') && tail[..tail.len() - 1].bytes().all(|b| b == b'0') =>
        {
            format!("{head}Z")
        }
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_scopes_to_library() {
        let c = RestClient::with_host("http://127.0.0.1:1", "contoso.sharepoint.com", "c").unwrap();
        let d = Drive {
            id: "L".into(),
            name: "Documents".into(),
            drive_type: "documentLibrary".into(),
            root_folder: Some("/sites/M/Shared Documents".into()),
        };
        let u = build_url(&c, &d, "budget");
        assert!(
            u.starts_with("http://127.0.0.1:1/_api/search/query?querytext=%27%28budget%29%20path%3A%22https%3A//contoso.sharepoint.com/sites/M/Shared%20Documents%22"),
            "{u}"
        );
        assert!(u.ends_with("startrow=0"));
    }

    #[test]
    fn normalize_modified_strips_all_zero_fraction() {
        assert_eq!(
            normalize_modified("2016-05-16T15:59:28.0000000Z".to_string()),
            "2016-05-16T15:59:28Z"
        );
    }

    #[test]
    fn normalize_modified_keeps_non_zero_fraction() {
        assert_eq!(
            normalize_modified("2016-05-16T15:59:28.1234567Z".to_string()),
            "2016-05-16T15:59:28.1234567Z"
        );
    }
}
