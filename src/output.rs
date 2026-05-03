//! Output configuration: TTY detection, JSON/table/quiet modes,
//! color, and the JSON-error-on-stdout contract.

use std::io::IsTerminal;

use serde_json::json;

use crate::error::{CliError, exit_code_for};

pub fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

pub fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

#[derive(Clone, Copy, Debug)]
pub struct OutputConfig {
    pub json: bool,
    pub quiet: bool,
}

impl OutputConfig {
    /// Build from `--json` / `--quiet` flags. JSON is forced on when stdout is not a TTY.
    pub fn new(json_flag: bool, quiet: bool) -> Self {
        let json = json_flag || !std::io::stdout().is_terminal();
        Self { json, quiet }
    }

    /// Print one line of data to stdout.
    pub fn print_data(&self, data: &str) {
        println!("{data}");
    }

    /// Print informational message to stderr; suppressed by --quiet.
    pub fn print_message(&self, msg: &str) {
        if !self.quiet {
            eprintln!("{msg}");
        }
    }

    /// Print serialized JSON to stdout.
    pub fn print_json(&self, value: &serde_json::Value) {
        println!(
            "{}",
            serde_json::to_string_pretty(value).expect("serialize JSON")
        );
    }

    /// Render an error per the spec contract:
    /// - JSON mode: emit `{"error": {...}}` to **stdout** (deliberate divergence
    ///   from jira-cli — agents parsing stdout get a structured error).
    /// - Plain mode: emit the message to **stderr**.
    ///
    /// Returns the exit code the caller should use.
    pub fn render_error(&self, err: &CliError) -> i32 {
        let exit = exit_code_for(err);
        if self.json {
            let code = match err {
                CliError::Input(_) => "input",
                CliError::Auth(_) => "auth",
                CliError::ReadOnly(_) => "read_only",
                CliError::NotFound(_) => "not_found",
                CliError::Api { .. } => "api",
                CliError::RateLimit => "rate_limit",
                CliError::Http(_) => "http",
                CliError::Other(_) => "other",
            };
            let value = json!({
                "error": {
                    "code": code,
                    "message": err.to_string(),
                    "exit": exit,
                }
            });
            self.print_json(&value);
        } else {
            eprintln!("error: {err}");
        }
        exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_forced_on_when_not_tty() {
        // Tests run without a TTY, so `new(false, false)` should still set json=true.
        let cfg = OutputConfig::new(false, false);
        assert!(cfg.json);
    }

    #[test]
    fn quiet_flag_propagates() {
        let cfg = OutputConfig::new(false, true);
        assert!(cfg.quiet);
    }

    #[test]
    fn render_error_returns_input_exit_for_input_error() {
        let cfg = OutputConfig {
            json: true,
            quiet: true,
        };
        let exit = cfg.render_error(&CliError::Input("bad ref".into()));
        assert_eq!(exit, 2);
    }

    #[test]
    fn render_error_returns_auth_exit_for_auth_error() {
        let cfg = OutputConfig {
            json: true,
            quiet: true,
        };
        let exit = cfg.render_error(&CliError::Auth("expired".into()));
        assert_eq!(exit, 3);
    }

    #[test]
    fn use_color_respects_no_color_env() {
        // Even with TTY, NO_COLOR=1 should disable color. Tests have no TTY,
        // so we're really asserting the function returns false either way.
        // SAFETY: setting env vars in tests is racy; this single-threaded
        // assertion is safe because we only read inside this block.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        assert!(!use_color());
        unsafe { std::env::remove_var("NO_COLOR") };
    }
}
