//! Command implementations dispatched from `cli.rs`.
//!
//! Each subcommand lives in its own file so we can grow the surface without
//! `cli.rs` ballooning.

pub(crate) mod auth;
pub(crate) mod config;
pub(crate) mod drives;
pub(crate) mod files;
pub(crate) mod init;
pub(crate) mod sites;
