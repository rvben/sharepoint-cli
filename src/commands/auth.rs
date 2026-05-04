use crate::cli::{AuthCmd, Runtime};
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime, _cmd: AuthCmd) -> Result<()> {
    Err(CliError::Other("auth not implemented yet".into()))
}
