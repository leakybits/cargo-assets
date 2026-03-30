use crate::error::{Error, Result};
use crate::metadata::Asset;
use crate::progress::Progress;
use camino::{Utf8Path, Utf8PathBuf};
use reqwest::Client;
use sha2::Digest;
use std::io::SeekFrom;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedSender as Sender;

const CHUNK_SIZE: u64 = 1 << 30; // 1 GiB

#[allow(async_fn_in_trait)]
pub trait Task {
    async fn run(&self) -> Result<()>;
}

#[derive(Debug)]
pub struct SyncAssetTask {
    pub id: usize,
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub assets_dir: Utf8PathBuf,
    pub tx: Sender<Progress>,
    pub rc: Client,
}

impl SyncAssetTask {
    pub fn new(
        id: usize,
        asset: Asset,
        assets_dir: Utf8PathBuf,
        tx: Sender<Progress>,
        rc: Client,
    ) -> Self {
        Self {
            id,
            name: asset.name,
            url: asset.url,
            sha256: asset.sha256,
            assets_dir,
            tx,
            rc,
        }
    }

    async fn download_parallel(&self, path: &Utf8Path, size: u64) -> Result<()> {
        self.tx.send(Progress::Start {
            id: self.id,
            name: self.name.to_owned(),
            size,
        })?;

        let file = fs::File::create(path).await?;
        file.set_len(size).await?;
        drop(file);

        let mut start = 0;
        let mut tasks = Vec::new();

        while start < size {
            let end = (start + CHUNK_SIZE - 1).min(size - 1);
            tasks.push(self.download_chunk(path, start, end));
            start += CHUNK_SIZE;
        }

        futures::future::try_join_all(tasks).await?;

        if let Some(want) = &self.sha256 {
            self.tx.send(Progress::Reset {
                id: self.id,
                name: format!("Verifying {}...", self.name),
                size,
            })?;

            let mut file = fs::File::open(path).await?;
            let mut sha = sha2::Sha256::new();
            let mut buf = vec![0u8; 1024 * 1024]; // 1MiB buffer

            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                sha.update(&buf[..n]);
                self.tx.send(Progress::Inc {
                    id: self.id,
                    n: n as u64,
                })?;
            }

            if hex::encode(sha.finalize()) != *want {
                fs::remove_file(path).await?;
                self.tx.send(Progress::Error {
                    id: self.id,
                    msg: format!("SHA-256 mismatch for {}", self.name),
                })?;
                return Err(Error::Checksum(self.name.clone()));
            }
        }

        self.tx.send(Progress::Finish { id: self.id })?;
        Ok(())
    }

    async fn download_chunk(&self, path: &Utf8Path, start: u64, end: u64) -> Result<()> {
        let mut res = self
            .rc
            .get(&self.url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .await?
            .error_for_status()?;

        let mut file = fs::OpenOptions::new().write(true).open(path).await?;
        file.seek(SeekFrom::Start(start)).await?;

        while let Some(chunk) = res.chunk().await? {
            file.write_all(&chunk).await?;
            self.tx.send(Progress::Inc {
                id: self.id,
                n: chunk.len() as u64,
            })?;
        }

        Ok(())
    }

    async fn download_sequential(&self, path: &Utf8Path) -> Result<()> {
        let mut res = self.rc.get(&self.url).send().await?.error_for_status()?;

        self.tx.send(Progress::Start {
            id: self.id,
            name: self.name.to_owned(),
            size: res.content_length().unwrap_or(0),
        })?;

        let mut dst = fs::File::create(path).await?;
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
            && hex::encode(sha.finalize()) != *want
        {
            fs::remove_file(path).await?;
            self.tx.send(Progress::Error {
                id: self.id,
                msg: format!("SHA-256 mismatch for {}", self.name),
            })?;
            return Err(Error::Checksum(self.name.clone()));
        } else {
            self.tx.send(Progress::Finish { id: self.id })?;
        }

        Ok(())
    }
}

impl Task for SyncAssetTask {
    async fn run(&self) -> Result<()> {
        let path = self.assets_dir.join(&self.name);

        if path.exists() {
            self.tx.send(Progress::Finish { id: self.id })?;
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let head = self.rc.head(&self.url).send().await;

        match head {
            Ok(res) if res.status().is_success() => {
                let size = res.content_length().unwrap_or(0);
                let accept_ranges = res
                    .headers()
                    .get(reqwest::header::ACCEPT_RANGES)
                    .map(|v| v == "bytes")
                    .unwrap_or(false);

                if accept_ranges && size > CHUNK_SIZE {
                    self.download_parallel(&path, size).await?;
                } else {
                    self.download_sequential(&path).await?;
                }
            }
            _ => {
                self.download_sequential(&path).await?;
            }
        }

        Ok(())
    }
}
