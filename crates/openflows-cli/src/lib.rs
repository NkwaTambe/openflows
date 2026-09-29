//! OpenFlows remote CLI — a client for the deployed OpenFlows control surface.
//!
//! Operators run this from outside a host's network to manage tenants, assign
//! work directly, and halt/resume a fleet, without direct access to Redis or
//! Coder. It talks to the `openflows-manager` HTTP API over bearer-token auth.

pub mod client;
pub mod commands;
pub mod config;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

pub use commands::Command;

#[derive(Debug, Parser)]
#[command(
    name = "openflows-cli",
    about = "Remote control surface for a deployed OpenFlows instance",
    version
)]
pub struct Cli {
    /// Manager base URL (overrides the value saved by `login`).
    #[arg(long, global = true, env = "OPENFLOWS_MANAGER_URL")]
    pub url: Option<String>,

    /// Bearer token (overrides the value saved by `login`).
    #[arg(long, global = true, env = "OPENFLOWS_MANAGER_TOKEN")]
    pub token: Option<String>,

    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Path to the config file (defaults to ~/.config/openflows/config.toml).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// Resolve the config file path, falling back to the user default.
fn config_path(cli: &Cli) -> PathBuf {
    cli.config
        .clone()
        .unwrap_or_else(config::default_config_path)
}

/// Run the CLI to completion, returning an error for any failed operation.
pub async fn run(cli: Cli) -> Result<()> {
    let cfg_path = config_path(&cli);

    // `login` writes credentials and needs no client, so handle it first.
    if let Command::Login { url, token } = &cli.command {
        let token = token
            .clone()
            .or_else(|| cli.token.clone())
            .or_else(|| std::env::var("OPENFLOWS_MANAGER_TOKEN").ok());
        config::save_login(&cfg_path, url, token.as_deref())?;
        println!(
            "✓ Saved OpenFlows manager credentials to {}",
            cfg_path.display()
        );
        return Ok(());
    }

    let saved = config::load(&cfg_path)?;
    let url = cli.url.clone().or(saved.url).context(
        "no manager URL configured; run `openflows-cli login --url <url>` or pass --url",
    )?;
    let token = cli.token.clone().or(saved.token).context(
        "no bearer token configured; run `openflows-cli login` or set OPENFLOWS_MANAGER_TOKEN",
    )?;

    let client = client::Client::new(url, token);
    commands::dispatch(cli.command, &client, cli.json).await
}
