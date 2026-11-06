//! Backend dispatch: one enum the commands talk to, implemented by either
//! Microsoft Graph or SharePoint REST. Both return the same domain types.

use std::collections::BTreeMap;

use tokio::io::AsyncWrite;

use crate::auth::AuthContext;
use crate::cli::Runtime;
use crate::config::BackendKind;
use crate::error::Result;
use crate::graph::drives::{Drive, DriveItem, ListChildrenResult};
use crate::graph::search::SearchResult;
use crate::graph::sites::{Site, SiteListResult};
use crate::graph::{self, GraphClient};
use crate::reference::SiteRef;
use crate::rest::{self, RestClient};

pub(crate) enum Backend {
    Graph(GraphClient),
    Rest(RestClient),
}

impl Backend {
    pub(crate) fn from_runtime(rt: &Runtime) -> Result<Self> {
        Ok(match rt.cfg.backend {
            BackendKind::Graph => Backend::Graph(GraphClient::new(AuthContext::new(
                rt.cfg.clone(),
                rt.cache_path.clone(),
            ))),
            BackendKind::Rest => Backend::Rest(RestClient::from_config(&rt.cfg, &rt.session_path)?),
        })
    }

    /// Base URL page tokens must point at.
    pub(crate) async fn cursor_endpoint(&self) -> String {
        match self {
            Backend::Graph(g) => g.graph_endpoint().await,
            Backend::Rest(r) => r.endpoint().to_string(),
        }
    }

    pub(crate) async fn list_sites(
        &self,
        query: Option<&str>,
        page_url: Option<&str>,
    ) -> Result<SiteListResult> {
        match self {
            Backend::Graph(g) => graph::sites::list(g, query, page_url).await,
            Backend::Rest(r) => rest::sites::list(r, query, page_url).await,
        }
    }

    pub(crate) async fn resolve_site(
        &self,
        site_ref: &SiteRef,
        aliases: &BTreeMap<String, String>,
        default_site: Option<&str>,
    ) -> Result<Site> {
        match self {
            Backend::Graph(g) => graph::sites::resolve(g, site_ref, aliases, default_site).await,
            Backend::Rest(r) => rest::sites::resolve(r, site_ref, aliases, default_site).await,
        }
    }

    pub(crate) async fn list_drives(&self, site: &Site) -> Result<Vec<Drive>> {
        match self {
            Backend::Graph(g) => graph::drives::list_drives(g, &site.id).await,
            Backend::Rest(r) => rest::drives::list_drives(r, site).await,
        }
    }

    pub(crate) async fn find_drive(&self, site: &Site, name: &str) -> Result<Drive> {
        match self {
            Backend::Graph(g) => graph::drives::find_drive_by_name(g, &site.id, name).await,
            Backend::Rest(r) => rest::drives::find_drive_by_name(r, site, name).await,
        }
    }

    pub(crate) async fn list_children(
        &self,
        site: &Site,
        drive: &Drive,
        path: &str,
        page_url: Option<&str>,
    ) -> Result<ListChildrenResult> {
        match self {
            Backend::Graph(g) => graph::drives::list_children(g, &drive.id, path, page_url).await,
            Backend::Rest(r) => rest::drives::list_children(r, site, drive, path, page_url).await,
        }
    }

    pub(crate) async fn list_children_recursive(
        &self,
        site: &Site,
        drive: &Drive,
        path: &str,
    ) -> Result<Vec<DriveItem>> {
        match self {
            Backend::Graph(g) => graph::drives::list_children_recursive(g, &drive.id, path).await,
            Backend::Rest(r) => rest::drives::list_children_recursive(r, site, drive, path).await,
        }
    }

    pub(crate) async fn get_item(
        &self,
        site: &Site,
        drive: &Drive,
        path: &str,
    ) -> Result<DriveItem> {
        match self {
            Backend::Graph(g) => {
                graph::drives::get_item_with_download_url(g, &drive.id, path).await
            }
            Backend::Rest(r) => rest::drives::get_item(r, site, drive, path).await,
        }
    }

    pub(crate) async fn download_to_writer<W: AsyncWrite + Unpin>(
        &self,
        site: &Site,
        drive: &Drive,
        path: &str,
        writer: &mut W,
    ) -> Result<u64> {
        match self {
            Backend::Graph(g) => {
                graph::download::download_to_writer(g, &drive.id, path, writer).await
            }
            Backend::Rest(r) => {
                rest::download::download_to_writer(r, site, drive, path, writer).await
            }
        }
    }

    pub(crate) async fn search(
        &self,
        drive: &Drive,
        folder: &str,
        query: &str,
        page_url: Option<&str>,
    ) -> Result<SearchResult> {
        match self {
            Backend::Graph(g) => graph::search::search(g, &drive.id, folder, query, page_url).await,
            Backend::Rest(r) => rest::search::search(r, drive, folder, query, page_url).await,
        }
    }
}
