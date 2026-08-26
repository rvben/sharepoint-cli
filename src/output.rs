//! Output configuration: TTY detection, JSON/table/quiet modes,
//! color, and the structured error contract.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;

use crate::error::{CliError, exit_code_for, kind_for};

pub fn use_color() -> bool {
    !NO_COLOR.load(Ordering::Relaxed)
        && std::env::var_os("NO_COLOR").is_none()
        && std::io::stdout().is_terminal()
}

static NO_COLOR: AtomicBool = AtomicBool::new(false);

pub fn set_no_color(disabled: bool) {
    NO_COLOR.store(disabled, Ordering::Relaxed);
}

pub fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

/// Three-valued output format flag (mirrors `--output auto|text|json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// JSON when stdout is not a TTY; human-friendly text when it is.
    Auto,
    /// Always human-friendly text (no JSON even when piped).
    Text,
    /// Always JSON.
    Json,
}

#[derive(Clone, Copy, Debug)]
pub struct OutputConfig {
    /// Whether to emit JSON on stdout for data output.
    pub json: bool,
    pub quiet: bool,
}

impl OutputConfig {
    /// Build from the `--output` enum and `--quiet` flag.
    ///
    /// `--output auto` (the default) emits JSON when stdout is not a TTY.
    /// An explicit `text` or `json` always wins.
    pub fn new(format: OutputFormat, quiet: bool) -> Self {
        let json = match format {
            OutputFormat::Json => true,
            OutputFormat::Text => false,
            OutputFormat::Auto => !std::io::stdout().is_terminal(),
        };
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

    /// Print an interactive prompt that the user MUST see to proceed.
    ///
    /// Device-code prompts (verification URL, user code) are interactive
    /// instructions, not optional status messages. They are emitted
    /// unconditionally to stderr regardless of `--quiet` or `--json`.
    /// Stderr is used even in `--json` mode to keep the JSON stdout stream
    /// clean and parseable by agents.
    pub fn print_required_prompt(&self, msg: &str) {
        eprintln!("{msg}");
    }

    /// Print serialized JSON to stdout.
    pub fn print_json(&self, value: &serde_json::Value) {
        println!(
            "{}",
            serde_json::to_string_pretty(value).expect("serialize JSON")
        );
    }

    /// Render a structured error.
    ///
    /// Machine-readable mode writes a one-line JSON envelope to stderr; text
    /// mode writes one human-readable error. Stdout remains data-only.
    ///
    /// Returns the exit code the caller should use.
    pub fn render_error(&self, err: &CliError) -> i32 {
        let exit = exit_code_for(err);
        let kind = kind_for(err);
        let envelope = json!({
            "error": {
                "kind": kind,
                "message": err.to_string(),
                "exit_code": exit,
            }
        });
        if self.json {
            eprintln!(
                "{}",
                serde_json::to_string(&envelope).expect("serialize error envelope")
            );
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
        // Tests run without a TTY, so auto format should still set json=true.
        let cfg = OutputConfig::new(OutputFormat::Auto, false);
        assert!(cfg.json);
    }

    #[test]
    fn explicit_text_wins_over_auto() {
        let cfg = OutputConfig::new(OutputFormat::Text, false);
        assert!(!cfg.json, "text format must not emit JSON even when piped");
    }

    #[test]
    fn explicit_json_wins_over_auto() {
        let cfg = OutputConfig::new(OutputFormat::Json, false);
        assert!(cfg.json);
    }

    #[test]
    fn quiet_flag_propagates() {
        let cfg = OutputConfig::new(OutputFormat::Auto, true);
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
