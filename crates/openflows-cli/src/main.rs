use anyhow::Result;
use clap::Parser;
use openflows_cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    openflows_cli::run(cli).await
}
