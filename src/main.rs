use std::process::ExitCode;

use clap::Parser;

use sharepoint_cli::cli::{self, Cli};
use sharepoint_cli::error::{CliError, exit_codes};
use sharepoint_cli::output::{OutputConfig, OutputFormat};

/// Rewrite argv before passing to clap so that `files download --output PATH`
/// maps `--output` to `--path`.
///
/// `files download` predates the spec-conformant `--output FORMAT` global flag.
/// Its `--output` means "destination file path" (or `-` for stdout), which
/// conflicts with the global format selector. We rewrite it here so clap sees
/// `--path` instead, keeping the `--output -` contract from the test suite
/// while the global `--output auto|text|json` continues to work on all other
/// subcommands.
fn rewrite_argv(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut result: Vec<std::ffi::OsString> = args.collect();
    // Find the position of `files` followed by `download` (ignoring flags in
    // between, which shouldn't exist in practice but guards against edge cases).
    let files_pos = result.iter().position(|a| a == "files");
    let download_pos = files_pos.and_then(|fp| {
        result[fp + 1..]
            .iter()
            .position(|a| a == "download")
            .map(|rel| fp + 1 + rel)
    });
    if let Some(dl_pos) = download_pos {
        // Rewrite `--output` that appears after `download` (and its REF arg)
        // to `--path` so clap routes it to FilesCmd::Download.path.
        for arg in &mut result[dl_pos + 1..] {
            if arg == "--output" {
                *arg = "--path".into();
            }
        }
    }
    result
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse_from(rewrite_argv(std::env::args_os())) {
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
            // Real parse errors use auto-format (JSON when piped).
            let out = OutputConfig::new(OutputFormat::Auto, false);
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
    let out = OutputConfig::new(cli.output, cli.quiet);
    match cli::run(cli).await {
        Ok(()) => ExitCode::from(exit_codes::SUCCESS as u8),
        Err(err) => ExitCode::from(out.render_error(&err) as u8),
    }
}
