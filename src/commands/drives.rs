use crate::cli::{DrivesCmd, Runtime};
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime, _cmd: DrivesCmd) -> Result<()> {
    Err(CliError::Other("drives not implemented yet".into()))
}
