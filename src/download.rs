use crate::metadata::Asset;
use crate::progress::ProgressMsg;
use anyhow::{Result, bail};
use camino::Utf8Path;
use sha2::Digest;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[allow(async_fn_in_trait)]
pub trait Task {
    async fn perform(&self, tx: &mpsc::Sender<ProgressMsg>) -> Result<()>;
}

pub struct DownloadAssetTask<'a> {
    pub id: usize,
    pub asset: &'a Asset,
    pub assets_dir: &'a Utf8Path,
}

impl<'a> Task for DownloadAssetTask<'a> {
    async fn perform(&self, tx: &mpsc::Sender<ProgressMsg>) -> Result<()> {
        let path = self.assets_dir.join(&self.asset.name);

        if path.exists() {
            tx.send(ProgressMsg::Finish { id: self.id }).await?;
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

        tx.send(ProgressMsg::Finish { id: self.id }).await?;

        Ok(())
    }
}
