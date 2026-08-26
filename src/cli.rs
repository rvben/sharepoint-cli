//! CLI entry point: clap derive structs and the `run` dispatcher.

use std::io;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::config::{self, ConfigFile, ENV_CLIENT_ID, ENV_PROFILE, ENV_TENANT, ResolvedConfig};
use crate::error::Result;
use crate::output::{OutputConfig, OutputFormat};

#[derive(Debug, Parser)]
#[command(
    name = "sharepoint",
    about = "Agent-friendly SharePoint Online CLI",
    after_help = "Get started:\n  sharepoint init                     Configure and sign in\n  sharepoint doctor                   Check configuration and Graph access\n  sharepoint sites list               Discover your sites\n  sharepoint schema --command 'files ls'\n                                      Inspect one command for automation",
    version,
    propagate_version = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Output format: auto (JSON when piped), text, or json.
    #[arg(
        long,
        short = 'o',
        global = true,
        default_value = "auto",
        value_name = "FORMAT"
    )]
    pub output: OutputFormat,

    /// Alias for --output json.
    #[arg(long, global = true, hide = true)]
    pub json: bool,

    /// Suppress informational messages on stderr.
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color even on a terminal.
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Active config profile (default: "default"). Env: SHAREPOINT_PROFILE.
    #[arg(long, global = true, env = ENV_PROFILE)]
    pub profile: Option<String>,

    /// Tenant override. Env: SHAREPOINT_TENANT_ID.
    #[arg(long, global = true, env = ENV_TENANT)]
    pub tenant: Option<String>,

    /// Client ID override. Env: SHAREPOINT_CLIENT_ID.
    #[arg(long, global = true, env = ENV_CLIENT_ID)]
    pub client_id: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Configure a profile and optionally start device-code login.
    Init(InitArgs),
    /// Sub-commands: login, logout, status.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Sub-commands: show, path.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Sub-commands: list, use.
    #[command(subcommand)]
    Sites(SitesCmd),
    /// Sub-commands: list.
    #[command(subcommand)]
    Drives(DrivesCmd),
    /// Sub-commands: ls, stat, download, find.
    #[command(subcommand)]
    Files(FilesCmd),
    /// Check configuration, credential cache, and Graph access.
    Doctor {
        /// Skip the Microsoft Graph connectivity check.
        #[arg(long)]
        offline: bool,
    },
    /// Generate shell completions.
    Completions { shell: Shell },
    /// Emit a machine-readable description of all commands and their output shapes.
    Schema {
        /// Return only one complete command path.
        #[arg(long)]
        command: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthCmd {
    /// Run the device-code flow and cache the resulting tokens.
    Login,
    /// Delete cached tokens for the active profile's tenant/client.
    Logout,
    /// Show cached account info, expiry, scopes.
    Status {
        /// Maximum number of accounts to show.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Opaque pagination cursor from a previous response's `next` field.
        #[arg(long)]
        page: Option<String>,
        /// Comma-separated fields to include (e.g. username,expires_at).
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Default site name or URL for commands that omit a site.
    #[arg(long, env = config::ENV_DEFAULT_SITE)]
    pub default_site: Option<String>,

    /// Save the profile without starting device-code login.
    #[arg(long)]
    pub no_login: bool,

    /// Block remote write operations for this profile.
    #[arg(long, env = config::ENV_READ_ONLY)]
    pub read_only: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print the resolved config (token & secrets masked).
    Show,
    /// Print the absolute path to the config file.
    Path,
}

#[derive(Debug, Subcommand)]
pub enum SitesCmd {
    /// List sites. Without --query: followed sites; with --query: search.
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        page: Option<String>,
        /// Comma-separated output fields to include (e.g. id,name,url).
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
    /// Set `default_site` in the active profile.
    Use {
        /// Site name or URL.
        site: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum DrivesCmd {
    /// List drives (libraries) for a site reference.
    List {
        site: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        all: bool,
        /// Comma-separated output fields to include (e.g. id,name,drive_type).
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum FilesCmd {
    /// List items at a reference (folder).
    Ls {
        #[arg(value_name = "REF")]
        reference: String,
        #[arg(short = 'r', long)]
        recursive: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        page: Option<String>,
        /// Comma-separated output fields to include (e.g. name,size,kind).
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
    /// Show metadata for a single item.
    Stat {
        #[arg(value_name = "REF")]
        reference: String,
    },
    /// Download a file. PATH or `-` for stdout.
    Download {
        #[arg(value_name = "REF")]
        reference: String,
        /// Destination path (or `-` for stdout). Use `--output`/`-o` as aliases (rewritten in argv before clap).
        #[arg(long, short = 'p')]
        path: Option<String>,
        #[arg(long)]
        overwrite: bool,
    },
    /// Search inside a drive (by query and/or shell glob).
    Find {
        #[arg(value_name = "REF")]
        reference: String,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        page: Option<String>,
        /// Comma-separated output fields to include (e.g. name,size,kind).
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
}

pub struct Runtime {
    pub out: OutputConfig,
    pub cfg: ResolvedConfig,
    pub config_file: ConfigFile,
    pub config_path: std::path::PathBuf,
    pub cache_path: std::path::PathBuf,
}

impl Runtime {
    pub fn build(cli: &Cli) -> Result<Self> {
        let config_path = config::config_path()?;
        let config_file = config::load_file(&config_path)?;
        let env_lookup =
            |k: &str| -> Option<String> { std::env::var(k).ok().filter(|s| !s.is_empty()) };
        let mut cfg = config::resolve(&config_file, cli.profile.as_deref(), &env_lookup)?;
        if let Some(t) = &cli.tenant {
            cfg.tenant_id = Some(t.clone());
        }
        if let Some(c) = &cli.client_id {
            cfg.client_id = Some(c.clone());
        }
        let cache_path = config::token_cache_path()?;
        Ok(Self {
            out: OutputConfig::new(
                if cli.json && cli.output == OutputFormat::Auto {
                    OutputFormat::Json
                } else {
                    cli.output
                },
                cli.quiet,
            ),
            cfg,
            config_file,
            config_path,
            cache_path,
        })
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    // Schema runs before any config/auth is needed.
    crate::output::set_no_color(cli.no_color);
    if let Command::Schema { command } = &cli.command {
        return crate::commands::schema::run(command.as_deref());
    }
    if let Command::Completions { shell } = &cli.command {
        clap_complete::generate(*shell, &mut Cli::command(), "sharepoint", &mut io::stdout());
        return Ok(());
    }
    let rt = Runtime::build(&cli)?;
    match cli.command {
        Command::Schema { .. } | Command::Completions { .. } => unreachable!(),
        Command::Init(args) => crate::commands::init::run(&rt, args).await,
        Command::Auth(sub) => crate::commands::auth::run(&rt, sub).await,
        Command::Config(sub) => crate::commands::config::run(&rt, sub).await,
        Command::Sites(sub) => crate::commands::sites::run(&rt, sub).await,
        Command::Drives(sub) => crate::commands::drives::run(&rt, sub).await,
        Command::Files(sub) => crate::commands::files::run(&rt, sub).await,
        Command::Doctor { offline } => crate::commands::doctor::run(&rt, offline).await,
    }
}
