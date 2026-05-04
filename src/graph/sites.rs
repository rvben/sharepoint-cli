//! Site discovery: followed sites (no query) and keyword search (with query).
//!
//! Resolution flow used by command code:
//!  1. SiteRef::Url      → `/sites/{hostname}:/{path}` lookup, returns Site.
//!  2. SiteRef::Name     → check profile aliases first, then `/sites?search=`.
//!  3. SiteRef::Default  → use ResolvedConfig.default_site, recurse.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::GraphClient;
use crate::error::{CliError, Result};
use crate::reference::SiteRef;

#[derive(Debug, Clone, Deserialize)]
pub struct Site {
    pub id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(rename = "webUrl")]
    pub web_url: String,
    #[serde(rename = "name", default)]
    pub url_segment: String,
}

#[derive(Debug, Clone)]
pub enum SiteListSource {
    Followed,
    Search,
}

pub struct SiteListResult {
    pub items: Vec<Site>,
    pub next: Option<String>,
    pub source: SiteListSource,
}

/// Without `query`: returns the user's followed sites.
/// With `query`: keyword search across the tenant.
pub async fn list(
    graph: &GraphClient,
    query: Option<&str>,
    page_token: Option<&str>,
) -> Result<SiteListResult> {
    let (path, source) = match (query, page_token) {
        (_, Some(token)) => {
            let decoded = decode_page_token(token)?;
            let source = source_for_path(&decoded);
            (decoded, source)
        }
        (None, None) => ("/me/followedSites".to_string(), SiteListSource::Followed),
        (Some(q), None) => (
            format!("/sites?search={}", urlencoding(q)),
            SiteListSource::Search,
        ),
    };
    let page: super::PagedResponse<Site> = graph.get_json(&path).await?;
    Ok(SiteListResult {
        items: page.value,
        next: page.next_link.as_deref().map(encode_page_token),
        source,
    })
}

/// Resolve a site by URL (`/sites/{hostname}:/{path}`).
pub async fn get_by_url(graph: &GraphClient, url: &str) -> Result<Site> {
    let parsed = url::Url::parse(url)
        .map_err(|e| CliError::Input(format!("invalid site URL '{url}': {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| CliError::Input(format!("site URL has no host: {url}")))?;
    let path = parsed.path().trim_start_matches('/');
    let api_path = format!("/sites/{host}:/{path}");
    graph.get_json::<Site>(&api_path).await
}

/// Resolve a site by name (alias map first, then `/sites?search=`).
pub async fn resolve(
    graph: &GraphClient,
    site_ref: &SiteRef,
    aliases: &BTreeMap<String, String>,
    default_site: Option<&str>,
) -> Result<Site> {
    match site_ref {
        SiteRef::Url(u) => get_by_url(graph, u).await,
        SiteRef::Default => {
            let raw = default_site.ok_or_else(|| {
                CliError::Input(
                    "this reference uses the default site but none is configured".into(),
                )
            })?;
            let nested = if raw.starts_with("http://") || raw.starts_with("https://") {
                SiteRef::Url(raw.to_string())
            } else {
                SiteRef::Name(raw.to_string())
            };
            Box::pin(resolve(graph, &nested, aliases, None)).await
        }
        SiteRef::Name(name) => {
            let lower = name.to_ascii_lowercase();
            for (k, v) in aliases {
                if k.to_ascii_lowercase() == lower {
                    return get_by_url(graph, v).await;
                }
            }
            let path = format!("/sites?search={}", urlencoding(name));
            let page: super::PagedResponse<Site> = graph.get_json(&path).await?;
            let exact = page
                .value
                .iter()
                .find(|s| s.display_name.eq_ignore_ascii_case(name))
                .cloned();
            exact
                .or_else(|| page.value.into_iter().next())
                .ok_or_else(|| CliError::NotFound(format!("site '{name}' not found")))
        }
    }
}

fn urlencoding(input: &str) -> String {
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

fn encode_page_token(next_link: &str) -> String {
    URL_SAFE_NO_PAD.encode(next_link.as_bytes())
}

fn decode_page_token(token: &str) -> Result<String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token.as_bytes())
        .map_err(|e| CliError::Input(format!("invalid --page token: {e}")))?;
    String::from_utf8(bytes).map_err(|e| CliError::Input(format!("invalid --page token: {e}")))
}

/// Derive the list source from a decoded page-token path.
fn source_for_path(path: &str) -> SiteListSource {
    if path.contains("/me/followedSites") {
        SiteListSource::Followed
    } else {
        SiteListSource::Search
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_token_round_trips() {
        let original = "https://graph.microsoft.com/v1.0/sites?$skiptoken=ABC";
        let encoded = encode_page_token(original);
        let decoded = decode_page_token(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn url_encoding_handles_spaces() {
        assert_eq!(urlencoding("Marketing Plan"), "Marketing%20Plan");
    }

    #[test]
    fn url_encoding_preserves_unreserved() {
        assert_eq!(urlencoding("a.b-c_d~e"), "a.b-c_d~e");
    }

    #[test]
    fn source_for_path_followed_sites() {
        // A page token from a followed-sites continuation must resolve to Followed.
        let path = "/me/followedSites?$skiptoken=XYZ";
        assert!(matches!(source_for_path(path), SiteListSource::Followed));

        // A page token from a search continuation must resolve to Search.
        let path = "/sites?search=intranet&$skiptoken=ABC";
        assert!(matches!(source_for_path(path), SiteListSource::Search));
    }
}
