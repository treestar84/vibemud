use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    ticks: Option<u32>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    vibemud_runtime::start_runtime(args.ticks)
}
