//! Site lookup and discovery over SharePoint REST.
//!
//! There is no "followed sites" equivalent here: `sites list` without a query
//! returns every site the user can see via search (`contentclass:STS_Site`).

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{RestClient, SEARCH_ROW_LIMIT, percent_encode_keep_slash, quoted, search_page};
use crate::error::{CliError, Result};
use crate::graph::sites::{Site, SiteListResult, SiteListSource};
use crate::reference::SiteRef;

#[derive(Deserialize)]
struct Web {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Title", default)]
    title: String,
    #[serde(rename = "Url")]
    url: String,
}

pub(crate) async fn get_by_url(client: &RestClient, site_url: &str) -> Result<Site> {
    let url = client.site_api(site_url, "/_api/web?$select=Id,Title,Url")?;
    let web: Web = client.get_json(&url).await?;
    Ok(Site {
        id: web.id,
        display_name: web.title,
        web_url: web.url,
    })
}

pub(crate) fn search_url(client: &RestClient, query: Option<&str>) -> String {
    let text = match query {
        Some(q) => format!("({q}) contentclass:STS_Site"),
        None => "contentclass:STS_Site".to_string(),
    };
    client.root_api(&format!(
        "/_api/search/query?querytext={}&selectproperties=%27Title,Path,WebId%27&rowlimit={SEARCH_ROW_LIMIT}&startrow=0",
        percent_encode_keep_slash(&quoted(&text))
    ))
}

pub(crate) async fn list(
    client: &RestClient,
    query: Option<&str>,
    page_url: Option<&str>,
) -> Result<SiteListResult> {
    let url = page_url.map_or_else(|| search_url(client, query), str::to_owned);
    let page = search_page(client, &url).await?;
    let items = page
        .rows
        .into_iter()
        .filter_map(|r| {
            Some(Site {
                id: r.get("WebId").cloned().unwrap_or_default(),
                display_name: r.get("Title").cloned().unwrap_or_default(),
                web_url: r.get("Path")?.clone(),
            })
        })
        .collect();
    Ok(SiteListResult {
        items,
        next_url: page.next_url,
        fetched_url: url,
        source: SiteListSource::Search,
    })
}

/// Same resolution order as the Graph backend: URL → default → alias → search.
pub(crate) async fn resolve(
    client: &RestClient,
    site_ref: &SiteRef,
    aliases: &BTreeMap<String, String>,
    default_site: Option<&str>,
) -> Result<Site> {
    match site_ref {
        SiteRef::Url(u) => get_by_url(client, u).await,
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
            Box::pin(resolve(client, &nested, aliases, None)).await
        }
        SiteRef::Name(name) => {
            if let Some((_, v)) = aliases.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                return get_by_url(client, v).await;
            }
            let page = list(client, Some(name), None).await?;
            let site = page
                .items
                .iter()
                .find(|s| s.display_name.eq_ignore_ascii_case(name))
                .or_else(|| page.items.first())
                .ok_or_else(|| CliError::NotFound(format!("site '{name}' not found")))?;
            get_by_url(client, &site.web_url).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_quotes_query_and_puts_startrow_last() {
        let c = RestClient::with_host("http://127.0.0.1:1", "contoso.sharepoint.com", "c").unwrap();
        let u = search_url(&c, Some("O'Brien"));
        assert!(
            u.contains("querytext=%27%28O%27%27Brien%29%20contentclass%3ASTS_Site%27"),
            "{u}"
        );
        assert!(u.ends_with("startrow=0"), "{u}");
    }
}
