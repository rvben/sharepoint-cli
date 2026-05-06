use std::process::ExitCode;

use clap::Parser;

use sharepoint_cli::cli::{self, Cli};
use sharepoint_cli::error::{CliError, exit_codes};
use sharepoint_cli::output::OutputConfig;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // Honor clap's exit semantics for --help / --version: clap has
            // already printed formatted help/version to its own stream; just
            // propagate its exit code and return.
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                e.print().ok();
                let code: u8 = if e.use_stderr() { 2 } else { 0 };
                return ExitCode::from(code);
            }
            // Real parse errors go through the JSON-error-on-stdout contract so
            // agents can parse a single stream regardless of TTY state.
            let out = OutputConfig::new(false, false);
            let msg = e.to_string();
            let first_line = msg
                .lines()
                .next()
                .unwrap_or("invalid arguments")
                .trim_start_matches("error: ")
                .to_string();
            let exit = out.render_error(&CliError::Input(first_line));
            return ExitCode::from(exit as u8);
        }
    };
    let out = OutputConfig::new(cli.json, cli.quiet);
    match cli::run(cli).await {
        Ok(()) => ExitCode::from(exit_codes::SUCCESS as u8),
        Err(err) => ExitCode::from(out.render_error(&err) as u8),
    }
}
