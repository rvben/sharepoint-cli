//! Reference parsing: turns whatever the user types into a structured
//! `ParsedRef { site, library, path }` without touching the network.
//!
//! Five input forms (tried in order):
//!   1. Full SharePoint URL (incl. `Forms/AllItems.aspx?id=...`)
//!   2. `Site:Library/path`
//!   3. `:Library/path` (uses default_site from profile)
//!   4. Bare `Library/path` (only when default_site is set; resolved later)
//!   5. `spo://<site>/<library>/<path>`
//!
//! The parser is intentionally network-free; site/library lookup happens in
//! `graph::sites` / `graph::drives` after this returns.

use crate::error::{CliError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteRef {
    /// `https://contoso.sharepoint.com/sites/Marketing` or OneDrive `*-my.sharepoint.com/personal/...`
    Url(String),
    /// Display name or alias (e.g. `Marketing`); resolved against profile aliases or Graph search.
    Name(String),
    /// Use the `default_site` from config.
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRef {
    pub site: SiteRef,
    pub library: Option<String>,
    pub path: String,
}

impl ParsedRef {
    pub fn root_path(&self) -> bool {
        self.path.is_empty() || self.path == "/"
    }
}

fn percent_decode(input: &str) -> String {
    // Lightweight percent-decoder — handles `%20` etc. without pulling in another dep.
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn split_path_first(s: &str) -> (String, String) {
    let trimmed = s.trim_start_matches('/');
    match trimmed.split_once('/') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

fn normalize_path(p: &str) -> String {
    let mut out = p.trim_matches('/').to_string();
    if out.is_empty() {
        return String::new();
    }
    out.insert(0, '/');
    out
}

fn parse_url(raw: &str) -> Result<ParsedRef> {
    let parsed =
        url::Url::parse(raw).map_err(|e| CliError::Input(format!("invalid URL '{raw}': {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| CliError::Input(format!("URL missing host: {raw}")))?;

    if !host.ends_with(".sharepoint.com") {
        return Err(CliError::Input(format!(
            "host '{host}' is not a SharePoint Online host (must end with .sharepoint.com)"
        )));
    }

    // Browser copy: `.../sites/<name>/<Library>/Forms/AllItems.aspx?id=<encoded path>`.
    if let Some(("id", value)) = parsed
        .query_pairs()
        .find(|(k, _)| k == "id")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .as_ref()
        .map(|(k, v)| (k.as_str(), v.as_str()))
    {
        // Url::query_pairs() already percent-decodes the value once. Decoding
        // again would corrupt user-typed literal percent sequences.
        let decoded = value.to_string();
        // `decoded` looks like `/sites/Marketing/Shared Documents/2025/Q4/plan.pptx`.
        // Strip the `/sites/<name>/` prefix to find library + path.
        let trimmed = decoded.trim_start_matches('/');
        let mut parts = trimmed.splitn(3, '/');
        let kind = parts.next().unwrap_or(""); // "sites" or "personal"
        let site_name = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        if (kind == "sites" || kind == "personal" || kind == "teams") && !site_name.is_empty() {
            let site_url = format!("{}://{}/{}/{}", parsed.scheme(), host, kind, site_name);
            let (library, path) = split_path_first(rest);
            return Ok(ParsedRef {
                site: SiteRef::Url(site_url),
                library: if library.is_empty() {
                    None
                } else {
                    Some(library)
                },
                path: normalize_path(&path),
            });
        }
    }

    // Plain path form: `https://<host>/sites/<name>/<Library>/<path>`.
    let path = percent_decode(parsed.path());
    let trimmed = path.trim_start_matches('/');
    let mut parts = trimmed.splitn(3, '/');
    let kind = parts.next().unwrap_or("");
    let site_name = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if (kind == "sites" || kind == "personal" || kind == "teams") && !site_name.is_empty() {
        let site_url = format!("{}://{}/{}/{}", parsed.scheme(), host, kind, site_name);
        let (library, sub) = split_path_first(rest);
        return Ok(ParsedRef {
            site: SiteRef::Url(site_url),
            library: if library.is_empty() {
                None
            } else {
                Some(library)
            },
            path: normalize_path(&sub),
        });
    }

    Err(CliError::Input(format!(
        "URL does not look like a SharePoint site reference: {raw}"
    )))
}

fn parse_spo_uri(raw: &str) -> Result<ParsedRef> {
    let stripped = raw
        .strip_prefix("spo://")
        .ok_or_else(|| CliError::Input(format!("not an spo:// URI: {raw}")))?;
    let mut parts = stripped.splitn(3, '/');
    let site = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CliError::Input(format!("spo:// missing site segment: {raw}")))?;
    let library = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    let path = parts.next().unwrap_or("");
    Ok(ParsedRef {
        site: SiteRef::Name(percent_decode(site)),
        library: library.map(|s| percent_decode(&s)),
        path: normalize_path(&percent_decode(path)),
    })
}

/// Parse a reference string. `default_site_set` lets the parser distinguish
/// the bare `Library/path` form from an unrelated identifier.
pub fn parse(raw: &str, default_site_set: bool) -> Result<ParsedRef> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(CliError::Input("empty reference".into()));
    }

    if raw.starts_with("spo://") {
        return parse_spo_uri(raw);
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return parse_url(raw);
    }

    // `:Library/path` — default-site form
    if let Some(rest) = raw.strip_prefix(':') {
        let (library, path) = split_path_first(rest);
        if library.is_empty() {
            return Err(CliError::Input(
                "':Library/path' form needs a library name after the colon".into(),
            ));
        }
        return Ok(ParsedRef {
            site: SiteRef::Default,
            library: Some(percent_decode(&library)),
            path: normalize_path(&percent_decode(&path)),
        });
    }

    // `Site:Library/path` — note: must occur BEFORE `Library/path` so colons take priority.
    if let Some((site, rest)) = raw.split_once(':')
        && !site.is_empty()
        && !rest.is_empty()
    {
        let (library, path) = split_path_first(rest);
        if library.is_empty() {
            return Err(CliError::Input(format!("'{raw}' has no library after ':'")));
        }
        return Ok(ParsedRef {
            site: SiteRef::Name(percent_decode(site)),
            library: Some(percent_decode(&library)),
            path: normalize_path(&percent_decode(&path)),
        });
    }

    // Bare `Library/path` — only when default_site is configured.
    if default_site_set {
        let (library, path) = split_path_first(raw);
        if library.is_empty() {
            return Err(CliError::Input(format!("could not parse reference: {raw}")));
        }
        return Ok(ParsedRef {
            site: SiteRef::Default,
            library: Some(percent_decode(&library)),
            path: normalize_path(&percent_decode(&path)),
        });
    }

    Err(CliError::Input(format!(
        "ambiguous reference '{raw}': set a default_site or use 'Site:Library/path'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_https_url() {
        let r = parse(
            "https://contoso.sharepoint.com/sites/Marketing/Shared Documents/2025/Q4.pptx",
            false,
        )
        .unwrap();
        assert!(matches!(r.site, SiteRef::Url(_)));
        assert_eq!(r.library.as_deref(), Some("Shared Documents"));
        assert_eq!(r.path, "/2025/Q4.pptx");
    }

    #[test]
    fn parses_browser_form_with_id_query() {
        let r = parse(
            "https://contoso.sharepoint.com/sites/Marketing/Shared%20Documents/Forms/AllItems.aspx?id=%2Fsites%2FMarketing%2FShared%20Documents%2F2025%2FQ4%2Eplan%2Epptx&p=1",
            false,
        )
        .unwrap();
        assert_eq!(r.library.as_deref(), Some("Shared Documents"));
        assert_eq!(r.path, "/2025/Q4.plan.pptx");
    }

    #[test]
    fn parses_onedrive_personal_url() {
        let r = parse(
            "https://contoso-my.sharepoint.com/personal/alice_contoso_com/Documents/file.txt",
            false,
        )
        .unwrap();
        match r.site {
            SiteRef::Url(u) => assert!(u.contains("/personal/alice_contoso_com")),
            _ => panic!("expected URL site"),
        }
        assert_eq!(r.library.as_deref(), Some("Documents"));
    }

    #[test]
    fn rejects_non_sharepoint_url() {
        assert!(parse("https://example.com/x", false).is_err());
    }

    #[test]
    fn parses_site_colon_library_path_form() {
        let r = parse("Marketing:Shared Documents/2025/plan.pptx", false).unwrap();
        assert_eq!(r.site, SiteRef::Name("Marketing".into()));
        assert_eq!(r.library.as_deref(), Some("Shared Documents"));
        assert_eq!(r.path, "/2025/plan.pptx");
    }

    #[test]
    fn parses_default_site_colon_form() {
        let r = parse(":Documents/file.txt", false).unwrap();
        assert_eq!(r.site, SiteRef::Default);
        assert_eq!(r.library.as_deref(), Some("Documents"));
        assert_eq!(r.path, "/file.txt");
    }

    #[test]
    fn bare_library_path_requires_default_site() {
        assert!(parse("Documents/file.txt", false).is_err());
        let r = parse("Documents/file.txt", true).unwrap();
        assert_eq!(r.site, SiteRef::Default);
        assert_eq!(r.library.as_deref(), Some("Documents"));
        assert_eq!(r.path, "/file.txt");
    }

    #[test]
    fn parses_spo_uri() {
        let r = parse("spo://Marketing/Shared%20Documents/2025/plan.pptx", false).unwrap();
        assert_eq!(r.site, SiteRef::Name("Marketing".into()));
        assert_eq!(r.library.as_deref(), Some("Shared Documents"));
        assert_eq!(r.path, "/2025/plan.pptx");
    }

    #[test]
    fn empty_reference_errors() {
        assert!(parse("", false).is_err());
        assert!(parse("   ", false).is_err());
    }

    #[test]
    fn library_only_path_normalizes_to_root() {
        let r = parse(":Documents", false).unwrap();
        assert!(r.root_path());
        assert_eq!(r.path, "");
    }

    #[test]
    fn percent_escapes_in_colon_form_path_are_decoded() {
        let r = parse(":Documents/Hello%20World.txt", false).unwrap();
        assert_eq!(r.path, "/Hello World.txt");
    }

    #[test]
    fn parses_id_query_with_literal_percent() {
        // `Url::query_pairs()` decodes exactly one layer of percent-encoding. When
        // the source contains double-encoded sequences (e.g. SharePoint encodes a
        // space as `%20`, then a transport layer encodes the `%` to `%25`,
        // producing `%2520`), one decode pass yields `%20` — which is part of
        // SharePoint's own path and must NOT be decoded again.
        let r = parse(
            "https://contoso.sharepoint.com/sites/Marketing/Forms/AllItems.aspx?id=%2Fsites%2FMarketing%2FShared%2520Documents%2Ffile%2520name.pptx",
            false,
        )
        .unwrap();
        assert_eq!(
            r.site,
            SiteRef::Url("https://contoso.sharepoint.com/sites/Marketing".into())
        );
        assert_eq!(r.library.as_deref(), Some("Shared%20Documents"));
        assert_eq!(r.path, "/file%20name.pptx");
    }

    #[test]
    fn site_colon_library_decodes_percent_escapes() {
        let r = parse("My%20Site:My%20Library/foo.txt", false).unwrap();
        assert_eq!(r.site, SiteRef::Name("My Site".into()));
        assert_eq!(r.library.as_deref(), Some("My Library"));
        assert_eq!(r.path, "/foo.txt");
    }

    #[test]
    fn default_site_colon_form_decodes_percent_escapes() {
        let r = parse(":My%20Library/foo%20bar.txt", false).unwrap();
        assert_eq!(r.site, SiteRef::Default);
        assert_eq!(r.library.as_deref(), Some("My Library"));
        assert_eq!(r.path, "/foo bar.txt");
    }

    #[test]
    fn bare_library_path_decodes_percent_escapes() {
        let r = parse("My%20Library/foo%20bar.txt", true).unwrap();
        assert_eq!(r.site, SiteRef::Default);
        assert_eq!(r.library.as_deref(), Some("My Library"));
        assert_eq!(r.path, "/foo bar.txt");
    }
}
