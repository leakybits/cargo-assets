use crate::IO_BUFFER_SIZE;
use crate::error::{Error, Result, VerificationError};
use crate::progress::Progress;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use sha2::Digest;
use std::process::Command;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Deserialize)]
pub struct CargoMetadata {
    packages: Vec<Package>,
    target_directory: Utf8PathBuf,
    metadata: Option<PackageMetadata>,
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
            Err(Error::CargoMetadata(String::from_utf8(output.stderr)?))
        }
    }

    pub fn target_directory(&self) -> &Utf8Path {
        &self.target_directory
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
struct Package {
    metadata: Option<PackageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct PackageMetadata {
    assets: Option<Vec<Asset>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    name: String,
    url: String,
    sha256: Option<String>,
}

impl Asset {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub async fn verify_checksum(
        &self,
        path: &Utf8Path,
        progress: Option<(usize, &UnboundedSender<Progress>)>,
    ) -> Result<()> {
        let Some(want) = &self.sha256 else {
            return Ok(());
        };

        let mut file = fs::File::open(path)
            .await
            .map_err(|e| VerificationError::Io {
                name: self.name.clone(),
                source: e,
            })?;

        if let Some((id, tx)) = progress {
            tx.send(Progress::Reset {
                id,
                name: format!("Verifying {}...", self.name),
                size: file.metadata().await?.len(),
            })?;
        }

        let mut sha = sha2::Sha256::new();
        let mut buf = vec![0u8; IO_BUFFER_SIZE];

        loop {
            let n = file
                .read(&mut buf)
                .await
                .map_err(|e| VerificationError::Io {
                    name: self.name.clone(),
                    source: e,
                })?;

            if n == 0 {
                break;
            }
            sha.update(&buf[..n]);

            if let Some((id, tx)) = progress {
                tx.send(Progress::Inc { id, n: n as u64 })?;
            }
        }

        let actual = hex::encode(sha.finalize());
        if actual != *want {
            return Err(VerificationError::Mismatch {
                name: self.name.clone(),
                expected: want.clone(),
                actual,
            }
            .into());
        }

        Ok(())
    }
}
