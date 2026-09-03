//! Command implementations dispatched from `cli.rs`.
//!
//! Each subcommand lives in its own file so we can grow the surface without
//! `cli.rs` ballooning.

pub(crate) mod auth;
pub(crate) mod config;
pub(crate) mod doctor;
pub(crate) mod drives;
pub(crate) mod files;
pub(crate) mod init;
pub(crate) mod profile;
pub(crate) mod schema;
pub(crate) mod sites;
