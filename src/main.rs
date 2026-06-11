use std::process::ExitCode;

use clap::Parser;

use sharepoint_cli::cli::{self, Cli};
use sharepoint_cli::error::{CliError, exit_codes};
use sharepoint_cli::output::{OutputConfig, OutputFormat};

/// Rewrite argv before passing to clap so that `files download` path spellings
/// (`--output PATH`, `--output=PATH`, `-o PATH`, `-oPATH`) are translated to
/// their `--path` equivalents.
///
/// `files download` predates the spec-conformant `--output auto|text|json`
/// global format flag. Its `-o`/`--output` means "destination file path" (or
/// `-` for stdout), which conflicts with the global format selector. Clap does
/// not allow a subcommand to shadow a global flag, so we rewrite the argv here
/// before clap sees it.
///
/// Rules:
/// - Only args that appear *after* the `download` token are rewritten.
/// - Everything after a bare `--` separator is left untouched (those are
///   positional pass-throughs that clap never interprets as flags).
/// - The four clap-accepted spellings that are rewritten:
///   - `--output`       (separate next-arg value)  → `--path`
///   - `--output=VALUE` (equals-attached value)     → `--path=VALUE`
///   - `-o`             (separate next-arg value)   → `--path`
///   - `-oVALUE`        (attached short value)      → `--path=VALUE`
fn rewrite_argv(args: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let args: Vec<std::ffi::OsString> = args.collect();

    // Locate `files` followed immediately-or-eventually by `download`.
    let files_pos = args.iter().position(|a| a == "files");
    let download_pos = files_pos.and_then(|fp| {
        args[fp + 1..]
            .iter()
            .position(|a| a == "download")
            .map(|rel| fp + 1 + rel)
    });

    let Some(dl_pos) = download_pos else {
        return args;
    };

    let mut result: Vec<std::ffi::OsString> = Vec::with_capacity(args.len());
    // Copy everything up to and including `download` unchanged.
    result.extend_from_slice(&args[..=dl_pos]);

    // Process the remainder, stopping rewrites once `--` is seen.
    let mut past_separator = false;
    let tail = &args[dl_pos + 1..];
    let mut i = 0;
    while i < tail.len() {
        let arg = &tail[i];

        if past_separator {
            result.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            past_separator = true;
            result.push(arg.clone());
            i += 1;
            continue;
        }

        // `--output VALUE` (bare long flag, value is next arg)
        if arg == "--output" {
            result.push("--path".into());
            i += 1;
            continue;
        }

        // `--output=VALUE` (long flag with attached value)
        if let Some(val) = arg.to_str().and_then(|s| s.strip_prefix("--output=")) {
            result.push(format!("--path={val}").into());
            i += 1;
            continue;
        }

        // `-o VALUE` (short flag, value is next arg)
        if arg == "-o" {
            result.push("--path".into());
            i += 1;
            continue;
        }

        // `-oVALUE` (short flag with attached value, e.g. `-o-` or `-o/tmp/f`)
        // Guard: `-o` alone is caught above; here `val` is non-empty.
        // Clap treats any non-empty suffix of a value-taking short flag as the
        // attached value, so `-o-` means value=`-` and `-o/tmp/f` means
        // value=`/tmp/f`. We do the same without further restrictions.
        if let Some(val) = arg
            .to_str()
            .and_then(|s| s.strip_prefix("-o"))
            .filter(|val| !val.is_empty())
        {
            result.push(format!("--path={val}").into());
            i += 1;
            continue;
        }

        result.push(arg.clone());
        i += 1;
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

#[cfg(test)]
mod tests {
    use super::rewrite_argv;

    fn argv(args: &[&str]) -> Vec<std::ffi::OsString> {
        args.iter().map(|s| s.into()).collect()
    }

    fn rewritten(args: &[&str]) -> Vec<String> {
        rewrite_argv(argv(args).into_iter())
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    // --- download scope: all four spellings are rewritten ---

    #[test]
    fn download_long_flag_separate_value() {
        // sharepoint files download REF --output -
        let got = rewritten(&["sp", "files", "download", "REF", "--output", "-"]);
        assert_eq!(got, ["sp", "files", "download", "REF", "--path", "-"]);
    }

    #[test]
    fn download_long_flag_equals_value() {
        // sharepoint files download REF --output=-
        let got = rewritten(&["sp", "files", "download", "REF", "--output=-"]);
        assert_eq!(got, ["sp", "files", "download", "REF", "--path=-"]);
    }

    #[test]
    fn download_short_flag_separate_value() {
        // sharepoint files download REF -o -
        let got = rewritten(&["sp", "files", "download", "REF", "-o", "-"]);
        assert_eq!(got, ["sp", "files", "download", "REF", "--path", "-"]);
    }

    #[test]
    fn download_short_flag_attached_value() {
        // sharepoint files download REF -o-
        let got = rewritten(&["sp", "files", "download", "REF", "-o-"]);
        assert_eq!(got, ["sp", "files", "download", "REF", "--path=-"]);
    }

    #[test]
    fn download_short_flag_attached_path() {
        // sharepoint files download REF -o/tmp/out.docx
        let got = rewritten(&["sp", "files", "download", "REF", "-o/tmp/out.docx"]);
        assert_eq!(
            got,
            ["sp", "files", "download", "REF", "--path=/tmp/out.docx"]
        );
    }

    #[test]
    fn download_long_flag_equals_real_path() {
        let got = rewritten(&["sp", "files", "download", "REF", "--output=/tmp/file.xlsx"]);
        assert_eq!(
            got,
            ["sp", "files", "download", "REF", "--path=/tmp/file.xlsx"]
        );
    }

    // --- bare `--` stops rewrites ---

    #[test]
    fn download_after_separator_not_rewritten() {
        let got = rewritten(&[
            "sp",
            "files",
            "download",
            "REF",
            "--",
            "--output",
            "something",
        ]);
        assert_eq!(
            got,
            [
                "sp",
                "files",
                "download",
                "REF",
                "--",
                "--output",
                "something"
            ]
        );
    }

    #[test]
    fn download_before_separator_rewritten_after_not() {
        let got = rewritten(&[
            "sp", "files", "download", "REF", "--output", "/a", "--", "--output", "/b",
        ]);
        assert_eq!(
            got,
            [
                "sp", "files", "download", "REF", "--path", "/a", "--", "--output", "/b"
            ]
        );
    }

    // --- non-download subcommands: no rewrite ---

    #[test]
    fn files_stat_short_flag_not_rewritten() {
        // sharepoint files stat REF -o json  (format selector, must stay)
        let got = rewritten(&["sp", "files", "stat", "REF", "-o", "json"]);
        assert_eq!(got, ["sp", "files", "stat", "REF", "-o", "json"]);
    }

    #[test]
    fn files_ls_output_not_rewritten() {
        let got = rewritten(&["sp", "files", "ls", "REF", "--output", "text"]);
        assert_eq!(got, ["sp", "files", "ls", "REF", "--output", "text"]);
    }

    #[test]
    fn global_output_before_subcommand_not_rewritten() {
        // sharepoint --output json files stat REF
        let got = rewritten(&["sp", "--output", "json", "files", "stat", "REF"]);
        assert_eq!(got, ["sp", "--output", "json", "files", "stat", "REF"]);
    }

    #[test]
    fn global_output_before_files_download_not_rewritten() {
        // sharepoint --output json files download REF  (format before, path after)
        let got = rewritten(&[
            "sp", "--output", "json", "files", "download", "REF", "--output", "-",
        ]);
        assert_eq!(
            got,
            [
                "sp", "--output", "json", "files", "download", "REF", "--path", "-"
            ]
        );
    }

    #[test]
    fn auth_status_not_rewritten() {
        let got = rewritten(&["sp", "auth", "status", "-o", "json"]);
        assert_eq!(got, ["sp", "auth", "status", "-o", "json"]);
    }

    #[test]
    fn no_files_subcommand_not_rewritten() {
        let got = rewritten(&["sp", "sites", "list", "--output", "json"]);
        assert_eq!(got, ["sp", "sites", "list", "--output", "json"]);
    }

    #[test]
    fn overwrite_flag_after_download_not_rewritten() {
        // --overwrite must pass through unchanged
        let got = rewritten(&[
            "sp",
            "files",
            "download",
            "REF",
            "--output",
            "/tmp/f",
            "--overwrite",
        ]);
        assert_eq!(
            got,
            [
                "sp",
                "files",
                "download",
                "REF",
                "--path",
                "/tmp/f",
                "--overwrite"
            ]
        );
    }
}
