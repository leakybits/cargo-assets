use crate::download::{DownloadAssetTask, Task};
use crate::metadata::{CargoMetadata, Metadata};
use crate::progress::ProgressMsg;
use anyhow::{Result, bail};
use clap::{Args, Parser};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::process::Command;
use tokio::sync::mpsc;

#[allow(async_fn_in_trait)]
pub trait AsyncRun {
    async fn run(self) -> Result<()>;
}

#[derive(Debug, Parser)]
pub enum Cmd {
    Assets(AssetsCmd),
}

impl AsyncRun for Cmd {
    async fn run(self) -> Result<()> {
        match self {
            Self::Assets(cmd) => cmd.run().await,
        }
    }
}

#[derive(Debug, Args)]
pub struct AssetsCmd;

impl AsyncRun for AssetsCmd {
    async fn run(self) -> Result<()> {
        let output = Command::new("cargo")
            .args(["metadata"])
            .args(["--format-version", "1"])
            .args(["--no-deps"])
            .output()?;

        let CargoMetadata {
            target_directory,
            metadata: Metadata { assets },
        } = if output.status.success() {
            serde_json::from_slice(&output.stdout)?
        } else {
            bail!(String::from_utf8(output.stderr)?);
        };

        let assets_dir = target_directory.join("assets");
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
                    ProgressMsg::Finish { id, msg } => {
                        if let Some(pb) = bars.remove(&id) {
                            pb.finish_with_message(msg);
                        } else {
                            let _ = mp.println(msg);
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
            let futures = assets.iter().enumerate().map(|(id, asset)| async move {
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
