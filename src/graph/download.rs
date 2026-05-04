//! Streaming downloads.
//!
//! `download_to_writer` follows Graph's redirect to the pre-authenticated
//! storage URL and streams the body to the caller's `AsyncWrite`. Used by
//! `files download` (writes to file or stdout via `-`).

use futures_util::StreamExt;
use reqwest::Method;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::GraphClient;
use crate::error::{CliError, Result};

/// Stream the contents of a drive item to `writer`. Returns total bytes written.
pub async fn download_to_writer<W: AsyncWrite + Unpin>(
    graph: &GraphClient,
    drive_id: &str,
    path: &str,
    writer: &mut W,
) -> Result<u64> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(CliError::Input("cannot download the drive root".into()));
    }
    let encoded = super::drives::encode_path_segments(trimmed);
    let api_path = format!("/drives/{drive_id}/root:/{encoded}:/content");
    let resp = graph.send(Method::GET, &api_path, None).await?;
    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
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
