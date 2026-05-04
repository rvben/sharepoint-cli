use crate::cli::{ConfigCmd, Runtime};
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime, _cmd: ConfigCmd) -> Result<()> {
    Err(CliError::Other("config not implemented yet".into()))
}
