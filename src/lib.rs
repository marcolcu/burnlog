//! Library entry point for the burnlog CLI.

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

pub mod analyze;
pub mod model;
pub mod render;
pub mod source;

#[derive(Parser)]
#[command(name = "burnlog", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze session logs and report token usage.
    Report,
}

pub fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Report => bail!("not implemented"),
    }
}
