use crate::download::{SyncAssetTask, Task};
use crate::metadata::CargoMetadata;
use crate::progress::Progress;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use futures::prelude::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indoc::indoc;
use std::collections::HashMap;
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
        let (tx, mut rx) = mpsc::unbounded_channel();

        let style = ProgressStyle::with_template(indoc! {r"
            {msg}
            [{wide_bar}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})
        "})?;

        let progress = async move {
            let mp = MultiProgress::new();
            let mut bars = HashMap::new();

            while let Some(msg) = rx.recv().await {
                match msg {
                    Progress::Start { id, name, size } => {
                        let pb = mp.add(ProgressBar::new(size as _));
                        pb.set_style(style.clone());
                        pb.set_message(name);
                        bars.insert(id, pb);
                    }

                    Progress::Reset { id, name, size } => {
                        if let Some(pb) = bars.get(&id) {
                            pb.set_length(size as _);
                            pb.reset();
                            pb.set_message(name);
                        }
                    }

                    Progress::Inc { id, n } => {
                        if let Some(pb) = bars.get(&id) {
                            pb.inc(n as _);
                        }
                    }

                    Progress::Finish { id } => {
                        if let Some(pb) = bars.remove(&id) {
                            pb.finish_and_clear();
                        }
                    }

                    Progress::Error { id, msg } => {
                        if let Some(pb) = bars.remove(&id) {
                            pb.finish_with_message(msg);
                        }
                    }
                }
            }
        };

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

        let ((), results) = tokio::join!(progress, downloads);

        for result in results {
            result?;
        }

        Ok(())
    }
}
