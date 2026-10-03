use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    vibemud_cli::run_vibemud(vibemud_cli::VibeMudCli::parse())
}
