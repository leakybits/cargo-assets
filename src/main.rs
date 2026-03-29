use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use clap::{Args, Parser};
use serde::Deserialize;
use sha2::Digest;
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;

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

        for asset in assets {
            let path = target_directory.join("assets").join(&asset.name);

            if path.exists() {
                continue;
            }

            let mut res = reqwest::get(&asset.url).await?.error_for_status()?;

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut dst = fs::File::create(&path).await?;
            let mut sha = sha2::Sha256::new();

            while let Some(chunk) = res.chunk().await? {
                dst.write_all(&chunk).await?;
                sha.update(&chunk);
            }

            if let Some(want) = asset.sha256
                && hex::encode(sha.finalize()) != want
            {
                fs::remove_file(&path).await?;
                bail!("SHA-256 mismatch for {}", asset.name);
            }
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
