use clap::{Parser, Subcommand};
use anyhow::Result;

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
    Init,
    Status,
    Logs,
    Companies,
    Agents,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init => commands::init::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Logs => commands::logs::run().await?,
        Commands::Companies => commands::companies::run().await?,
        Commands::Agents => commands::agents::run().await?,
    }

    Ok(())
}
