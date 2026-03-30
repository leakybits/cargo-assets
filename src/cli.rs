use crate::download::{SyncAssetTask, Task};
use crate::error::Result;
use crate::metadata::CargoMetadata;
use crate::progress::ProgressWatcher;
use clap::{Args, Parser, Subcommand};
use futures::prelude::*;
use indicatif::ProgressStyle;
use indoc::indoc;
use tokio::sync::mpsc;

#[allow(async_fn_in_trait)]
pub trait AsyncRun {
    async fn run(self) -> Result<()>;
}

#[derive(Debug, Parser)]
pub enum Cmd {
    #[command(subcommand)]
    Assets(AssetsCmd),
}

impl AsyncRun for Cmd {
    async fn run(self) -> Result<()> {
        match self {
            Self::Assets(cmd) => cmd.run().await,
        }
    }
}

/// Manage assets defined in Cargo.toml metadata
#[derive(Debug, Subcommand)]
pub enum AssetsCmd {
    Sync(SyncCmd),
}

impl AsyncRun for AssetsCmd {
    async fn run(self) -> Result<()> {
        match self {
            Self::Sync(cmd) => cmd.run().await,
        }
    }
}

/// Download and sync assets
#[derive(Debug, Args)]
pub struct SyncCmd;

impl AsyncRun for SyncCmd {
    async fn run(self) -> Result<()> {
        let metadata = CargoMetadata::load()?;
        let assets = metadata.assets();
        let assets_dir = metadata.target_directory.join("assets");
        let (tx, rx) = mpsc::unbounded_channel();

        let style = ProgressStyle::with_template(indoc! {r"
            {msg}
            [{wide_bar}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})
        "})?;

        let watcher = ProgressWatcher::new(style);

        let downloads = async move {
            let rc = reqwest::Client::new();
            let tasks = assets
                .into_iter()
                .enumerate()
                .map(|(id, asset)| {
                    SyncAssetTask::new(id, asset, assets_dir.clone(), tx.clone(), rc.clone())
                })
                .map(async |a| a.run().await);

            let res = future::join_all(tasks).await;

            drop(tx);

            res
        };

        let (progress_res, results) = tokio::join!(watcher.watch(rx), downloads);
        progress_res?;

        for result in results {
            result?;
        }

        Ok(())
    }
}
