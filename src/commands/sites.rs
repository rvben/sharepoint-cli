use crate::cli::{Runtime, SitesCmd};
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime, _cmd: SitesCmd) -> Result<()> {
    Err(CliError::Other("sites not implemented yet".into()))
}
