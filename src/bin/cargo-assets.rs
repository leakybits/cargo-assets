use anyhow::Result;
use cargo_assets::cli::{AsyncRun, Cmd};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    Cmd::parse().run().await
}
