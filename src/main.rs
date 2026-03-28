use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use serde::Deserialize;
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> Result<()> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
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

        let mut dst = fs::File::create(path).await?;

        while let Some(chunk) = res.chunk().await? {
            dst.write_all(&chunk).await?;
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct CargoMetadata {
    target_directory: Utf8PathBuf,
    metadata: Metadata,
}

#[derive(Deserialize)]
struct Metadata {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    url: String,
}
