use crate::cli::Runtime;
use crate::error::{CliError, Result};

pub async fn run(_rt: &Runtime) -> Result<()> {
    Err(CliError::Other("init not implemented yet".into()))
}
