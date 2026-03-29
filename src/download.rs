use crate::metadata::Asset;
use crate::progress::Progress;
use anyhow::Result;
use camino::Utf8Path;
use sha2::Digest;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedSender as Sender;

#[allow(async_fn_in_trait)]
pub trait Task {
    async fn run(&self) -> Result<()>;
}

#[derive(Debug)]
pub struct SyncAssetTask<'a> {
    pub id: usize,
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub assets_dir: &'a Utf8Path,
    pub tx: Sender<Progress>,
}

impl<'a> SyncAssetTask<'a> {
    pub fn new(id: usize, asset: Asset, assets_dir: &'a Utf8Path, tx: Sender<Progress>) -> Self {
        Self {
            id,
            name: asset.name,
            url: asset.url,
            sha256: asset.sha256,
            assets_dir,
            tx,
        }
    }
}

impl Task for SyncAssetTask<'_> {
    async fn run(&self) -> Result<()> {
        let path = self.assets_dir.join(&self.name);

        if path.exists() {
            self.tx.send(Progress::Finish { id: self.id })?;
            return Ok(());
        }

        let mut res = reqwest::get(&self.url).await?.error_for_status()?;

        self.tx.send(Progress::Start {
            id: self.id,
            name: self.name.to_owned(),
            size: res.content_length().unwrap_or(0),
        })?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut dst = fs::File::create(&path).await?;
        let mut sha = sha2::Sha256::new();

        while let Some(chunk) = res.chunk().await? {
            dst.write_all(&chunk).await?;
            sha.update(&chunk);
            self.tx.send(Progress::Inc {
                id: self.id,
                n: chunk.len() as u64,
            })?;
        }

        if let Some(want) = &self.sha256
            && &hex::encode(sha.finalize()) != want
        {
            fs::remove_file(&path).await?;

            self.tx.send(Progress::Error {
                id: self.id,
                msg: format!("SHA-256 mismatch for {}", self.name),
            })?;
        } else {
            self.tx.send(Progress::Finish { id: self.id })?;
        }

        Ok(())
    }
}
