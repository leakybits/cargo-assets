use crate::download::{CheckAssetTask, SyncAssetTask};
use crate::error::Result;
use crate::metadata::CargoMetadata;
use crate::progress::ProgressWatcher;
use clap::{Args, Parser, Subcommand};
use futures::prelude::*;
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
    Check(CheckCmd),
}

impl AsyncRun for AssetsCmd {
    async fn run(self) -> Result<()> {
        match self {
            Self::Sync(cmd) => cmd.run().await,
            Self::Check(cmd) => cmd.run().await,
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
        let assets_dir = metadata.target_directory().join("assets");
        let rc = reqwest::Client::new();

        let mut tasks = Vec::new();
        let mut streams = stream::SelectAll::new();

        for (id, asset) in assets.into_iter().enumerate() {
            let (tx, rx) = mpsc::unbounded_channel();
            tasks.push(SyncAssetTask::new(
                id,
                asset,
                assets_dir.clone(),
                tx,
                rc.clone(),
            ));
            streams.push(
                stream::unfold(rx, |mut rx| async { rx.recv().await.map(|m| (m, rx)) }).boxed(),
            );
        }

        let watcher = ProgressWatcher::new()?.watch(streams);

        let results = stream::iter(tasks)
            .map(|t| t.into_future())
            .buffer_unordered(4)
            .try_collect::<Vec<_>>();

        tokio::try_join!(watcher, results)?;

        Ok(())
    }
}

/// Deeply verify all local assets
#[derive(Debug, Args)]
pub struct CheckCmd;

impl AsyncRun for CheckCmd {
    async fn run(self) -> Result<()> {
        let metadata = CargoMetadata::load()?;
        let assets = metadata.assets();
        let assets_dir = metadata.target_directory().join("assets");

        let mut tasks = Vec::new();
        let mut streams = stream::SelectAll::new();

        for (id, asset) in assets.into_iter().enumerate() {
            let (tx, rx) = mpsc::unbounded_channel();
            tasks.push(CheckAssetTask::new(id, asset, assets_dir.clone(), tx));
            streams.push(
                stream::unfold(rx, |mut rx| async { rx.recv().await.map(|m| (m, rx)) }).boxed(),
            );
        }

        let watcher = ProgressWatcher::new()?.watch(streams);

        let results = stream::iter(tasks)
            .map(|t| t.into_future())
            .buffer_unordered(4)
            .try_collect::<Vec<_>>();

        tokio::try_join!(watcher, results)?;

        Ok(())
    }
}
