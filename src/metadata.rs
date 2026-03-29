use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use sha2::Digest;
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Deserialize)]
pub struct CargoMetadata {
    pub packages: Vec<Package>,
    pub target_directory: Utf8PathBuf,
    pub metadata: Option<PackageMetadata>,
}

impl CargoMetadata {
    pub fn load() -> Result<Self> {
        let output = Command::new("cargo")
            .args(["metadata"])
            .args(["--format-version", "1"])
            .args(["--no-deps"])
            .output()?;

        if output.status.success() {
            Ok(serde_json::from_slice(&output.stdout)?)
        } else {
            bail!(String::from_utf8(output.stderr)?);
        }
    }

    pub fn assets(&self) -> Vec<Asset> {
        let mut all = Vec::new();

        // Workspace assets
        if let Some(meta) = &self.metadata
            && let Some(assets) = &meta.assets
        {
            all.extend(assets.to_owned());
        }

        // Package assets
        for pkg in &self.packages {
            if let Some(meta) = &pkg.metadata
                && let Some(assets) = &meta.assets
            {
                all.extend(assets.to_owned());
            }
        }

        all
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    pub name: String,
    pub metadata: Option<PackageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageMetadata {
    pub assets: Option<Vec<Asset>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    /// The name with which to save the asset.
    pub name: String,

    /// The URL from which to download the asset.
    pub url: String,

    /// The SHA-256 hash of the asset.
    ///
    /// If provided, the downloaded asset will be verified against this hash.
    /// If the hash does not match, the asset will be deleted and an error will
    /// be returned.
    pub sha256: Option<String>,
}

impl Asset {
    pub async fn verify_checksum(&self, path: &Utf8Path) -> Result<bool> {
        let Some(want) = &self.sha256 else {
            return Ok(true);
        };

        let mut file = fs::File::open(path).await?;
        let mut sha = sha2::Sha256::new();
        let mut buf = [0u8; 8192];

        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            sha.update(&buf[..n]);
        }

        Ok(&hex::encode(sha.finalize()) == want)
    }
}
