//! Drive-scoped search via Graph's `/drives/{id}/root/search(q='…')`.
//!
//! The `find` command uses this and adds client-side glob filtering on top
//! when `--name <glob>` is given.

use super::drives::{DriveItem, encode_path_segments};
use super::{GraphClient, PagedResponse};
use crate::error::Result;
use crate::reference::percent_decode;

pub(crate) struct SearchResult {
    pub(crate) items: Vec<DriveItem>,
    /// The raw `@odata.nextLink` URL returned by Graph, if there are more results.
    pub(crate) next_url: Option<String>,
    /// The URL that was actually fetched. Used by callers that need to encode a
    /// mid-page cursor.
    pub(crate) fetched_url: String,
}

/// Execute a search scoped to `folder` (library-relative, `""` or `/` for the
/// whole drive). `page_url` is the full Graph URL to fetch; when `None` the
/// first-page search URL is built from `drive_id`, `folder` and `query`.
pub(crate) async fn search(
    graph: &GraphClient,
    drive_id: &str,
    folder: &str,
    query: &str,
    page_url: Option<&str>,
) -> Result<SearchResult> {
    let api = match page_url {
        Some(url) => url.to_string(),
        None => build_search_url(drive_id, folder, query),
    };
    // Resolve to a full absolute URL so the cursor stored in `fetched_url`
    // is always a complete URL (required by the host-validation check on decode).
    let absolute_url = graph.url(&api).await;
    let page: PagedResponse<DriveItem> = graph.get_json(&absolute_url).await?;
    let items = page
        .value
        .into_iter()
        .filter(|item| !known_outside(item, folder))
        .collect();
    Ok(SearchResult {
        items,
        next_url: page.next_link,
        fetched_url: absolute_url,
    })
}

/// Whether `item`'s parent path proves it lies outside `folder`.
///
/// The search request is already scoped to the folder; this check guards
/// against the service widening that scope. Items whose `parentReference`
/// carries no path cannot be placed, so they are left to the server's scope.
fn known_outside(item: &DriveItem, folder: &str) -> bool {
    let folder = folder.trim_matches('/');
    if folder.is_empty() {
        return false;
    }
    let Some((_, parent)) = item
        .parent_reference
        .as_ref()
        .and_then(|p| p.path.split_once(':'))
    else {
        return false;
    };
    let parent = percent_decode(parent.trim_matches('/')).to_ascii_lowercase();
    let folder = folder.to_ascii_lowercase();
    !(parent == folder || parent.starts_with(&format!("{folder}/")))
}

/// Build the Graph API path for a search under `folder` of drive `drive_id`.
///
/// OData single-quote escaping is applied first (doubling `'` to `''`),
/// then the result is percent-encoded so reserved URL characters like
/// `&`, `#`, `=`, and space are never interpolated raw into the URL.
pub(super) fn build_search_url(drive_id: &str, folder: &str, query: &str) -> String {
    let odata_escaped = query.replace('\'', "''");
    let encoded = url_encode_query(&odata_escaped);
    let folder = folder.trim_matches('/');
    let item = if folder.is_empty() {
        "root".to_string()
    } else {
        format!("root:/{}:", encode_path_segments(folder))
    };
    format!("/drives/{drive_id}/{item}/search(q='{encoded}')")
}

/// Percent-encode a string using the RFC 3986 unreserved character set,
/// leaving only `A-Z a-z 0-9 - _ . ~` unencoded.
fn url_encode_query(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => write!(out, "%{b:02X}").unwrap(),
        }
    }
    out
}

/// Shell-style glob match. `*` matches any run of characters, `?` matches one.
/// Case-insensitive.
pub(crate) fn glob_matches(pattern: &str, name: &str) -> bool {
    let p = pattern.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    glob_inner(p.as_bytes(), n.as_bytes())
}

fn glob_inner(pat: &[u8], s: &[u8]) -> bool {
    // Iterative DP avoids stack overflow on long patterns.
    let m = pat.len();
    let n = s.len();
    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;
    for i in 1..=m {
        if pat[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=m {
        for j in 1..=n {
            if pat[i - 1] == b'*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if pat[i - 1] == b'?' || pat[i - 1] == s[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[m][n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basic_matches() {
        assert!(glob_matches("*.pptx", "Q4-plan.pptx"));
        assert!(glob_matches("Q?-*.xlsx", "Q4-summary.xlsx"));
        assert!(!glob_matches("*.pdf", "report.docx"));
    }

    #[test]
    fn glob_is_case_insensitive() {
        assert!(glob_matches("*.PPTX", "plan.pptx"));
        assert!(glob_matches("Plan.*", "PLAN.pptx"));
    }

    #[test]
    fn glob_handles_empty_pattern() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "x"));
        assert!(glob_matches("*", "anything"));
    }

    #[test]
    fn build_search_url_percent_encodes_reserved_characters() {
        let url = build_search_url("D1", "", "foo & bar=baz#frag");
        // Reserved characters must be percent-encoded.
        assert!(
            !url.contains(" & "),
            "spaces and & must be encoded; got {url}"
        );
        assert!(!url.contains("=baz"), "= must be encoded; got {url}");
        assert!(!url.contains("#frag"), "# must be encoded; got {url}");
        assert!(
            url.contains("foo%20%26%20bar%3Dbaz%23frag"),
            "expected encoded form; got {url}"
        );
    }

    #[test]
    fn build_search_url_odata_escapes_single_quotes() {
        let url = build_search_url("D1", "", "Bob's");
        // OData: single quote doubled, then the doubled single quote is percent-encoded.
        // ' becomes '' in OData, and '' becomes %27%27 in URL encoding.
        assert!(
            url.contains("Bob%27%27s"),
            "single quote must be OData-escaped and percent-encoded; got {url}"
        );
    }

    #[test]
    fn build_search_url_plain_alphanumeric_is_unchanged() {
        let url = build_search_url("D1", "", "quarterly-report_2025.xlsx");
        assert!(
            url.contains("quarterly-report_2025.xlsx"),
            "unreserved chars must not be encoded; got {url}"
        );
    }
}
