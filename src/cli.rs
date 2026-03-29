use crate::download::{DownloadAssetTask, Task};
use crate::metadata::CargoMetadata;
use crate::progress::ProgressMsg;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashMap;
use tokio::fs;
use tokio::sync::mpsc;

#[allow(async_fn_in_trait)]
pub trait AsyncRun {
    async fn run(self) -> Result<()>;
}

#[derive(Debug, Parser)]
#[command(name = "cargo-assets", bin_name = "cargo-assets")]
pub enum Cmd {
    /// Manage assets defined in Cargo.toml metadata
    #[command(subcommand)]
    Assets(AssetsSub),

    /// Download and sync assets (shortcut for `assets sync`)
    Sync(SyncCmd),

    /// Show the current status of the assets (shortcut for `assets status`)
    Status(StatusCmd),

    /// Remove all cached assets (shortcut for `assets clean`)
    Clean(CleanCmd),
}

#[derive(Debug, Subcommand)]
pub enum AssetsSub {
    /// Download and sync assets
    Sync(SyncCmd),

    /// Show the current status of the assets
    Status(StatusCmd),

    /// Remove all cached assets
    Clean(CleanCmd),
}

impl AsyncRun for Cmd {
    async fn run(self) -> Result<()> {
        match self {
            Self::Assets(sub) => match sub {
                AssetsSub::Sync(cmd) => cmd.run().await,
                AssetsSub::Status(cmd) => cmd.run().await,
                AssetsSub::Clean(cmd) => cmd.run().await,
            },
            Self::Sync(cmd) => cmd.run().await,
            Self::Status(cmd) => cmd.run().await,
            Self::Clean(cmd) => cmd.run().await,
        }
    }
}

#[derive(Debug, Args)]
pub struct SyncCmd;

impl AsyncRun for SyncCmd {
    async fn run(self) -> Result<()> {
        let metadata = CargoMetadata::load()?;
        let assets = metadata.all_assets();

        let assets_dir = metadata.target_directory.join("assets");
        let (tx, mut rx) = mpsc::channel(100);

        let progress_manager = async move {
            let mp = MultiProgress::new();
            let mut bars: HashMap<usize, ProgressBar> = HashMap::new();
            let style = ProgressStyle::with_template(
                "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
            )
            .unwrap()
            .progress_chars("#>-");

            while let Some(msg) = rx.recv().await {
                match msg {
                    ProgressMsg::Start {
                        id,
                        name,
                        total_size,
                    } => {
                        let pb = mp.add(ProgressBar::new(total_size));
                        pb.set_style(style.clone());
                        pb.set_message(format!("Downloading {}", name));
                        bars.insert(id, pb);
                    }
                    ProgressMsg::Inc { id, n } => {
                        if let Some(pb) = bars.get(&id) {
                            pb.inc(n);
                        }
                    }
                    ProgressMsg::Finish { id } => {
                        if let Some(pb) = bars.remove(&id) {
                            pb.finish_and_clear();
                        }
                    }
                    ProgressMsg::Error { id, msg } => {
                        if let Some(pb) = bars.remove(&id) {
                            pb.finish_with_message(msg);
                        }
                    }
                }
            }
        };

        let downloads = async {
            let tx_ref = &tx;
            let dir_ref = &assets_dir;
            let futures = assets
                .into_iter()
                .enumerate()
                .map(|(id, asset)| async move {
                    let task = DownloadAssetTask {
                        id,
                        asset,
                        assets_dir: dir_ref,
                    };
                    task.perform(tx_ref).await
                });
            let results = futures::future::join_all(futures).await;
            drop(tx);
            results
        };

        let ((), results) = tokio::join!(progress_manager, downloads);

        for result in results {
            result?;
        }

        Ok(())
    }
}

#[derive(Debug, Args)]
pub struct StatusCmd;

enum AssetStatus {
    Missing,
    Cached,
    Mismatch,
    Error(String),
}

impl AsyncRun for StatusCmd {
    async fn run(self) -> Result<()> {
        let metadata = CargoMetadata::load()?;
        let assets = metadata.all_assets();

        let assets_dir = metadata.target_directory.join("assets");
        let (tx, mut rx) = mpsc::channel(100);

        let printer = async move {
            while let Some((name, status)) = rx.recv().await {
                match status {
                    AssetStatus::Missing => println!("MISSING  {}", name),
                    AssetStatus::Cached => println!("CACHED   {}", name),
                    AssetStatus::Mismatch => println!("MISMATCH {}", name),
                    AssetStatus::Error(e) => println!("ERROR    {} ({})", name, e),
                }
            }
        };

        let verifier = async {
            let tx_ref = &tx;
            let dir_ref = &assets_dir;
            let futures = assets.into_iter().map(|asset| async move {
                let path = dir_ref.join(&asset.name);
                let status = if !path.exists() {
                    AssetStatus::Missing
                } else {
                    match asset.verify_checksum(&path).await {
                        Ok(true) => AssetStatus::Cached,
                        Ok(false) => AssetStatus::Mismatch,
                        Err(e) => AssetStatus::Error(e.to_string()),
                    }
                };
                let _ = tx_ref.send((asset.name.clone(), status)).await;
            });
            futures::future::join_all(futures).await;
            drop(tx);
        };

        tokio::join!(printer, verifier);

        Ok(())
    }
}

#[derive(Debug, Args)]
pub struct CleanCmd;

impl AsyncRun for CleanCmd {
    async fn run(self) -> Result<()> {
        let metadata = CargoMetadata::load()?;
        let assets_dir = metadata.target_directory.join("assets");

        if assets_dir.exists() {
            fs::remove_dir_all(&assets_dir).await?;
            println!("Cleaned {}", assets_dir);
        } else {
            println!("Nothing to clean");
        }

        Ok(())
    }
}
