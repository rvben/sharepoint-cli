//! CLI entry point: clap derive structs and the `run` dispatcher.

use clap::{Parser, Subcommand};

use crate::config::{self, ConfigFile, ENV_CLIENT_ID, ENV_PROFILE, ENV_TENANT, ResolvedConfig};
use crate::error::Result;
use crate::output::{OutputConfig, OutputFormat};

#[derive(Debug, Parser)]
#[command(
    name = "sharepoint",
    about = "Agent-friendly SharePoint Online CLI",
    after_help = "Run `sharepoint schema` for a machine-readable description of all commands.",
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

    /// Suppress informational messages on stderr.
    #[arg(long, global = true)]
    pub quiet: bool,

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
    /// Interactive setup + first device-code login.
    Init,
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
    /// Emit a machine-readable description of all commands and their output shapes.
    Schema,
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
            out: OutputConfig::new(cli.output, cli.quiet),
            cfg,
            config_file,
            config_path,
            cache_path,
        })
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    // Schema runs before any config/auth is needed.
    if matches!(cli.command, Command::Schema) {
        return crate::commands::schema::run();
    }
    let rt = Runtime::build(&cli)?;
    match cli.command {
        Command::Schema => unreachable!(),
        Command::Init => crate::commands::init::run(&rt).await,
        Command::Auth(sub) => crate::commands::auth::run(&rt, sub).await,
        Command::Config(sub) => crate::commands::config::run(&rt, sub).await,
        Command::Sites(sub) => crate::commands::sites::run(&rt, sub).await,
        Command::Drives(sub) => crate::commands::drives::run(&rt, sub).await,
        Command::Files(sub) => crate::commands::files::run(&rt, sub).await,
    }
}
