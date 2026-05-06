//! sharepoint-cli library.
//!
//! # Public API
//!
//! The genuinely public surface is small: `auth`, `config`, and `graph` are
//! exported `pub` because integration tests in `tests/` import internals from
//! them directly (e.g. `AuthContext`, `token_cache`, `ResolvedConfig`,
//! `GraphClient`).  Everything else is `pub(crate)` — it is implementation
//! detail that the binary (`main.rs`) and command modules need within the
//! crate but that external consumers should not depend on.

pub mod auth;
pub mod cli;
pub(crate) mod commands;
pub mod config;
pub mod error;
pub mod graph;
pub mod output;
pub(crate) mod reference;
pub(crate) mod util;
