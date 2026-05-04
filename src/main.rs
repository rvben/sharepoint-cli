use std::process::ExitCode;

use clap::Parser;

use sharepoint_cli::cli::{self, Cli};
use sharepoint_cli::error::exit_codes;
use sharepoint_cli::output::OutputConfig;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let out = OutputConfig::new(cli.json, cli.quiet);
    match cli::run(cli).await {
        Ok(()) => ExitCode::from(exit_codes::SUCCESS as u8),
        Err(err) => ExitCode::from(out.render_error(&err) as u8),
    }
}
