use anyhow::Result;

fn main() -> Result<()> {
    vibemud_cli::run_mudctl_from(std::env::args())
}
