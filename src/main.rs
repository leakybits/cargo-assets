use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Args, Parser};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Deserialize;
use sha2::Digest;
use std::collections::HashMap;
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    Cmd::parse().run().await
}

trait AsyncRun {
    async fn run(self) -> Result<()>;
}

#[derive(Debug, Parser)]
enum Cmd {
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
struct AssetsCmd;

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

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    target_directory: Utf8PathBuf,
    metadata: Metadata,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    assets: Vec<Asset>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Asset {
    /// The name with which to save the asset.
    name: String,

    /// The URL from which to download the asset.
    url: String,

    /// The SHA-256 hash of the asset.
    ///
    /// If provided, the downloaded asset will be verified against this hash.
    /// If the hash does not match, the asset will be deleted and an error will
    /// be returned.
    sha256: Option<String>,
}

#[derive(Debug)]
enum ProgressMsg {
    Start {
        id: usize,
        name: String,
        total_size: u64,
    },
    Inc {
        id: usize,
        n: u64,
    },
    Finish {
        id: usize,
        msg: String,
    },
    Error {
        id: usize,
        msg: String,
    },
}

trait Task {
    async fn perform(&self, tx: &mpsc::Sender<ProgressMsg>) -> Result<()>;
}

struct DownloadAssetTask<'a> {
    id: usize,
    asset: &'a Asset,
    assets_dir: &'a Utf8Path,
}

impl<'a> Task for DownloadAssetTask<'a> {
    async fn perform(&self, tx: &mpsc::Sender<ProgressMsg>) -> Result<()> {
        let path = self.assets_dir.join(&self.asset.name);

        if path.exists() {
            tx.send(ProgressMsg::Finish {
                id: self.id,
                msg: format!("✅ {} (cached)", self.asset.name),
            })
            .await?;
            return Ok(());
        }

        let mut res = reqwest::get(&self.asset.url).await?.error_for_status()?;
        let total_size = res.content_length().unwrap_or(0);

        tx.send(ProgressMsg::Start {
            id: self.id,
            name: self.asset.name.clone(),
            total_size,
        })
        .await?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut dst = fs::File::create(&path).await?;
        let mut sha = sha2::Sha256::new();

        while let Some(chunk) = res.chunk().await? {
            dst.write_all(&chunk).await?;
            sha.update(&chunk);
            tx.send(ProgressMsg::Inc {
                id: self.id,
                n: chunk.len() as u64,
            })
            .await?;
        }

        if let Some(want) = &self.asset.sha256
            && &hex::encode(sha.finalize()) != want
        {
            fs::remove_file(&path).await?;
            let msg = format!("❌ SHA-256 mismatch for {}", self.asset.name);
            tx.send(ProgressMsg::Error {
                id: self.id,
                msg: msg.clone(),
            })
            .await?;
            bail!(msg);
        }

        tx.send(ProgressMsg::Finish {
            id: self.id,
            msg: format!("✅ Downloaded {}", self.asset.name),
        })
        .await?;

        Ok(())
    }
}
