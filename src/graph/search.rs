//! Drive-scoped search via Graph's `/drives/{id}/root/search(q='…')`.
//!
//! The `find` command uses this and adds client-side glob filtering on top
//! when `--name <glob>` is given.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::drives::DriveItem;
use super::{GraphClient, PagedResponse};
use crate::error::{CliError, Result};

pub struct SearchResult {
    pub items: Vec<DriveItem>,
    pub next: Option<String>,
}

pub async fn search(
    graph: &GraphClient,
    drive_id: &str,
    query: &str,
    page_token: Option<&str>,
) -> Result<SearchResult> {
    let api = match page_token {
        Some(t) => decode_page_token(t)?,
        None => build_search_url(&format!("drives/{drive_id}"), query),
    };
    let page: PagedResponse<DriveItem> = graph.get_json(&api).await?;
    Ok(SearchResult {
        items: page.value,
        next: page.next_link.as_deref().map(encode_page_token),
    })
}

/// Build the Graph API path for a drive search request.
///
/// The `drive_path` is a bare path prefix such as `"drives/{id}"`.
/// OData single-quote escaping is applied first (doubling `'` to `''`),
/// then the result is percent-encoded so reserved URL characters like
/// `&`, `#`, `=`, and space are never interpolated raw into the URL.
pub(super) fn build_search_url(drive_path: &str, query: &str) -> String {
    let odata_escaped = query.replace('\'', "''");
    let encoded = url_encode_query(&odata_escaped);
    format!("/{drive_path}/root/search(q='{encoded}')")
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
pub fn glob_matches(pattern: &str, name: &str) -> bool {
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
        let url = build_search_url("drives/D1", "foo & bar=baz#frag");
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
        let url = build_search_url("drives/D1", "Bob's");
        // OData: single quote doubled, then the doubled single quote is percent-encoded.
        // ' becomes '' in OData, and '' becomes %27%27 in URL encoding.
        assert!(
            url.contains("Bob%27%27s"),
            "single quote must be OData-escaped and percent-encoded; got {url}"
        );
    }

    #[test]
    fn build_search_url_plain_alphanumeric_is_unchanged() {
        let url = build_search_url("drives/D1", "quarterly-report_2025.xlsx");
        assert!(
            url.contains("quarterly-report_2025.xlsx"),
            "unreserved chars must not be encoded; got {url}"
        );
    }
}
