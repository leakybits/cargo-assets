use anyhow::Result;
use cargo_assets::cli::{AsyncRun, Cmd};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    Cmd::parse().run().await?;

    Ok(())
}
