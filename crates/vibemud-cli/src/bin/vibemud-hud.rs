use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let args = std::iter::once("vibemud".to_string())
        .chain(std::iter::once("hud".to_string()))
        .chain(std::env::args().skip(1));
    vibemud_cli::run_vibemud(vibemud_cli::VibeMudCli::parse_from(args))
}
