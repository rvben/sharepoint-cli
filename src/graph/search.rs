//! Drive-scoped search via Graph's `/drives/{id}/root/search(q='…')`.
//!
//! The `find` command uses this and adds client-side glob filtering on top
//! when `--name <glob>` is given.
//!
//! Note: only single-quote doubling (OData escaping) is applied to the query
//! string. Characters like `&`, `#`, or non-ASCII are sent as-is.

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
        None => {
            // Graph spec: search(q='<query>'). Single-quote escaping: double the quote.
            let escaped = query.replace('\'', "''");
            format!("/drives/{drive_id}/root/search(q='{escaped}')")
        }
    };
    let page: PagedResponse<DriveItem> = graph.get_json(&api).await?;
    Ok(SearchResult {
        items: page.value,
        next: page.next_link.as_deref().map(encode_page_token),
    })
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
}
