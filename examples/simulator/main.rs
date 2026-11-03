mod server;
mod smoke;

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use server::{COOKIE, Fixtures, Result, Scenario, Simulator};

#[derive(Parser)]
#[command(about = "Local SharePoint REST simulator: dummy data, no tenant or browser needed")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve read-only fixtures on 127.0.0.1 until Ctrl-C.
    Serve {
        /// Listening port; 0 chooses an available port.
        #[arg(long, default_value_t = 8787)]
        port: u16,
        #[arg(long, value_enum, default_value_t = Scenario::Healthy)]
        scenario: Scenario,
        /// Maximum search or collection rows per response (forces pagination).
        #[arg(long, default_value_t = 1)]
        page_size: usize,
        /// Load a JSON dataset or sanitized response recording.
        #[arg(long)]
        fixtures: Option<PathBuf>,
    },
    /// Test a CLI binary against disposable servers for every scenario.
    Smoke {
        /// Path to the sharepoint binary to test (debug, release, or downloaded).
        #[arg(long)]
        cli: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            port,
            scenario,
            page_size,
            fixtures,
        } => {
            let fixtures = match fixtures {
                Some(path) => Fixtures::parse(&std::fs::read_to_string(path)?)?,
                None => Fixtures::bundled(),
            };
            let host = fixtures.host().to_string();
            let simulator = Simulator::start(port, scenario, page_size, fixtures).await?;
            println!(
                "SharePoint simulator listening at {} ({scenario:?})",
                simulator.uri()
            );
            println!("\nIn another POSIX shell (PowerShell instructions: docs/testing.md):");
            println!("export SHAREPOINT_BACKEND=rest");
            println!("export SHAREPOINT_HOST={host}");
            println!("export SHAREPOINT_REST_ENDPOINT={}", simulator.uri());
            println!("export SHAREPOINT_COOKIE='{COOKIE}'");
            println!("\nsharepoint sites list --all");
            println!(
                "\nUses dummy cookies; browser login and Microsoft SSO are not simulated.\nPress Ctrl-C to stop."
            );
            std::io::stdout().flush()?;
            tokio::signal::ctrl_c().await?;
        }
        Command::Smoke { cli } => {
            let checks = smoke::run(&cli).await?;
            for check in &checks {
                println!("PASS {check}");
            }
            println!("{} smoke checks passed", checks.len());
        }
    }
    Ok(())
}
