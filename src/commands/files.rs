use crate::cli::{FilesCmd, Runtime};
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime, _cmd: FilesCmd) -> Result<()> {
    Err(CliError::Other("files not implemented yet".into()))
}
