//! File content download over SharePoint REST.

use futures_util::StreamExt as _;
use tokio::io::{AsyncWrite, AsyncWriteExt as _};

use super::drives::server_relative;
use super::{RestClient, odata_path_literal};
use crate::error::{CliError, Result};
use crate::graph::drives::Drive;
use crate::graph::sites::Site;

pub(crate) async fn download_to_writer<W: AsyncWrite + Unpin>(
    client: &RestClient,
    site: &Site,
    drive: &Drive,
    path: &str,
    writer: &mut W,
) -> Result<u64> {
    if path.trim_matches('/').is_empty() {
        return Err(CliError::Input("cannot download the drive root".into()));
    }
    let url = client.site_api(
        &site.web_url,
        &format!(
            "/_api/web/GetFileByServerRelativePath(decodedurl='{}')/$value",
            odata_path_literal(&server_relative(drive, path)?)
        ),
    )?;
    let resp = client.get(&url).await?;
    let mut stream = resp.bytes_stream();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| CliError::Http(format!("download stream: {e}")))?;
        writer
            .write_all(&bytes)
            .await
            .map_err(|e| CliError::Other(format!("write download: {e}")))?;
        total += bytes.len() as u64;
    }
    writer
        .flush()
        .await
        .map_err(|e| CliError::Other(format!("flush download: {e}")))?;
    Ok(total)
}
