use std::collections::HashSet;

use crate::error::{CliError, Result};

/// A repeated URL cannot advance a collection. Ignore fragments, which are
/// never sent to the server, when detecting continuation cycles.
pub(crate) fn record_page(seen: &mut HashSet<String>, url: &str) -> Result<()> {
    let mut page =
        url::Url::parse(url).map_err(|_| CliError::Protocol("invalid pagination URL".into()))?;
    page.set_fragment(None);
    if !seen.insert(page.to_string()) {
        return Err(CliError::Protocol(
            "pagination repeated a continuation URL".into(),
        ));
    }
    Ok(())
}

/// Size of the complete result set, when this response alone establishes it.
///
/// A response that started at the first item (`resumed == false`) and reached
/// the end (`has_next == false`) holds every item, so its length is the total.
/// Otherwise the items outside this response are uncounted and the total is
/// unknown, which callers report as `null` rather than the page size.
pub(crate) fn observed_total(resumed: bool, has_next: bool, returned: usize) -> Option<usize> {
    (!resumed && !has_next).then_some(returned)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OffsetCursor {
    scope: String,
    offset: usize,
}

pub(crate) fn offset(token: Option<&str>, scope: &str) -> Result<usize> {
    use base64::Engine as _;
    let Some(token) = token else { return Ok(0) };
    let invalid = || CliError::Input("invalid page token for this collection".into());
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| invalid())?;
    let cursor: OffsetCursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if cursor.scope != scope {
        return Err(invalid());
    }
    Ok(cursor.offset)
}

pub(crate) fn next_offset(scope: &str, offset: usize, total: usize) -> Option<String> {
    use base64::Engine as _;
    (offset < total).then(|| {
        let cursor = OffsetCursor {
            scope: scope.to_owned(),
            offset,
        };
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&cursor).expect("serialize offset cursor"))
    })
}
