use clap::{Parser, Subcommand};
use anyhow::Result;

mod api;
mod commands;

#[derive(Parser, Debug)]
#[command(name = "multiclaw")]
#[command(about = "MultiClaw Administration CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show service health and container status
    Status,
    /// Show current version and update channel
    Version,
    /// Update MultiClaw to the latest version
    Update {
        /// Switch update channel before updating (stable, beta, dev)
        #[arg(long)]
        channel: Option<String>,
    },
    /// Stream container logs
    Logs {
        /// Service name (multiclawd, ui, postgres, ollama-proxy). Default: multiclawd
        service: Option<String>,
        /// Number of lines to show
        #[arg(long, default_value = "100")]
        tail: u32,
    },
    /// Show system configuration info
    Info,
    /// Initialize a new MultiClaw instance (MVP)
    Init,
    /// List companies (MVP)
    Companies,
    /// List agents (MVP)
    Agents,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => commands::status::run().await?,
        Commands::Version => commands::version::run().await?,
        Commands::Update { channel } => commands::update::run(channel).await?,
        Commands::Logs { service, tail } => commands::logs::run(service, tail).await?,
        Commands::Info => commands::info::run().await?,
        Commands::Init => commands::init::run().await?,
        Commands::Companies => commands::companies::run().await?,
        Commands::Agents => commands::agents::run().await?,
    }

    Ok(())
}
