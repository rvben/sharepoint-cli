//! Error types and exit code mapping.

use std::fmt;

#[derive(Debug)]
pub enum CliError {
    /// Bad user input or config (exit 2).
    Input(String),
    /// Authentication or token failure (exit 3).
    Auth(String),
    /// Read-only mode blocked a write (exit 2).
    ReadOnly(String),
    /// Site, library, or item not found (exit 4).
    NotFound(String),
    /// Graph API returned a non-2xx error (exit 5).
    Api { status: u16, message: String },
    /// Rate limited by Graph (exit 6).
    RateLimit,
    /// Underlying HTTP transport error (exit 1).
    Http(String),
    /// Anything else (exit 1).
    Other(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Input(m) => write!(f, "{m}"),
            CliError::Auth(m) => write!(f, "{m}"),
            CliError::ReadOnly(m) => write!(f, "{m}"),
            CliError::NotFound(m) => write!(f, "{m}"),
            CliError::Api { status, message } => write!(f, "Graph API {status}: {message}"),
            CliError::RateLimit => write!(f, "rate limited by Microsoft Graph"),
            CliError::Http(m) => write!(f, "{m}"),
            CliError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        CliError::Http(err.to_string())
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Other(err.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        CliError::Other(format!("JSON error: {err}"))
    }
}

pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const INPUT: i32 = 2;
    pub const AUTH: i32 = 3;
    pub const NOT_FOUND: i32 = 4;
    pub const API: i32 = 5;
    pub const RATE_LIMIT: i32 = 6;
}

pub fn exit_code_for(err: &CliError) -> i32 {
    match err {
        CliError::Input(_) | CliError::ReadOnly(_) => exit_codes::INPUT,
        CliError::Auth(_) => exit_codes::AUTH,
        CliError::NotFound(_) => exit_codes::NOT_FOUND,
        CliError::Api { .. } => exit_codes::API,
        CliError::RateLimit => exit_codes::RATE_LIMIT,
        CliError::Http(_) | CliError::Other(_) => exit_codes::GENERAL,
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_error_maps_to_exit_2() {
        assert_eq!(exit_code_for(&CliError::Input("x".into())), 2);
    }

    #[test]
    fn read_only_error_maps_to_exit_2() {
        assert_eq!(exit_code_for(&CliError::ReadOnly("x".into())), 2);
    }

    #[test]
    fn auth_error_maps_to_exit_3() {
        assert_eq!(exit_code_for(&CliError::Auth("x".into())), 3);
    }

    #[test]
    fn not_found_maps_to_exit_4() {
        assert_eq!(exit_code_for(&CliError::NotFound("x".into())), 4);
    }

    #[test]
    fn api_error_maps_to_exit_5() {
        assert_eq!(
            exit_code_for(&CliError::Api {
                status: 500,
                message: "x".into()
            }),
            5
        );
    }

    #[test]
    fn rate_limit_maps_to_exit_6() {
        assert_eq!(exit_code_for(&CliError::RateLimit), 6);
    }

    #[test]
    fn http_error_maps_to_general() {
        assert_eq!(exit_code_for(&CliError::Http("x".into())), 1);
    }

    #[test]
    fn other_error_maps_to_general() {
        assert_eq!(exit_code_for(&CliError::Other("x".into())), 1);
    }

    #[test]
    fn display_includes_status_for_api() {
        let e = CliError::Api {
            status: 404,
            message: "not found".into(),
        };
        assert_eq!(format!("{e}"), "Graph API 404: not found");
    }
}
