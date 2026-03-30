use crate::error::{DownloadError, Result};
use crate::metadata::Asset;
use crate::progress::Progress;
use camino::Utf8PathBuf;
use futures::future::BoxFuture;
use futures::prelude::*;
use reqwest::Client;
use std::future::IntoFuture;
use std::io::SeekFrom;
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedSender as Sender;

const CHUNK_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

#[derive(Debug)]
pub struct SyncAssetTask {
    id: usize,
    asset: Asset,
    assets_dir: Utf8PathBuf,
    tx: Sender<Progress>,
    rc: Client,
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
            asset,
            assets_dir,
            tx,
            rc,
        }
    }

    async fn download_chunk(&self, path: Utf8PathBuf, start: u64, end: u64) -> Result<()> {
        let res = self
            .rc
            .get(self.asset.url())
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| DownloadError::Chunk {
                name: self.asset.name().to_owned(),
                start,
                end,
                source: e,
            })?
            .error_for_status()
            .map_err(|e| DownloadError::Chunk {
                name: self.asset.name().to_owned(),
                start,
                end,
                source: e,
            })?;

        self.process_response(path, start, res).await
    }

    async fn process_response(
        &self,
        path: Utf8PathBuf,
        start: u64,
        mut res: reqwest::Response,
    ) -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).open(&path).await?;
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
}

impl IntoFuture for SyncAssetTask {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let path = self.assets_dir.join(self.asset.name());

            if path.exists() {
                self.tx.send(Progress::Finish { id: self.id })?;
                return Ok(());
            }

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            // Start by requesting the first chunk to determine size and range support
            let res = self
                .rc
                .get(self.asset.url())
                .header(
                    reqwest::header::RANGE,
                    format!("bytes=0-{}", CHUNK_SIZE - 1),
                )
                .send()
                .await
                .map_err(|e| DownloadError::Init {
                    name: self.asset.name().to_owned(),
                    source: e,
                })?
                .error_for_status()
                .map_err(|e| DownloadError::Init {
                    name: self.asset.name().to_owned(),
                    source: e,
                })?;

            let (total_size, is_partial) = if res.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                let total = res
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.split('/').next_back())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                (total, true)
            } else {
                (res.content_length().unwrap_or(0), false)
            };

            self.tx.send(Progress::Start {
                id: self.id,
                name: self.asset.name().to_owned(),
                size: total_size,
            })?;

            let mut chunk_futures: Vec<BoxFuture<'_, Result<()>>> = Vec::new();

            if is_partial && total_size > CHUNK_SIZE {
                let file = fs::File::create(&path).await?;
                file.set_len(total_size).await?;
                drop(file);

                chunk_futures.push(self.process_response(path.clone(), 0, res).boxed());

                let mut start = CHUNK_SIZE;
                while start < total_size {
                    let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
                    chunk_futures.push(self.download_chunk(path.clone(), start, end).boxed());
                    start += CHUNK_SIZE;
                }
            } else {
                fs::File::create(&path).await?;
                chunk_futures.push(self.process_response(path.clone(), 0, res).boxed());
            }

            stream::iter(chunk_futures)
                .buffer_unordered(4)
                .try_collect::<Vec<_>>()
                .await?;

            if let Err(e) = self
                .asset
                .verify_checksum(&path, Some((self.id, &self.tx)))
                .await
            {
                fs::remove_file(&path).await?;
                self.tx.send(Progress::Error {
                    id: self.id,
                    msg: e.to_string(),
                })?;
                return Err(e);
            }

            self.tx.send(Progress::Finish { id: self.id })?;
            Ok(())
        })
    }
}

pub struct CheckAssetTask {
    id: usize,
    asset: Asset,
    assets_dir: Utf8PathBuf,
    tx: Sender<Progress>,
}

impl CheckAssetTask {
    pub fn new(id: usize, asset: Asset, assets_dir: Utf8PathBuf, tx: Sender<Progress>) -> Self {
        Self {
            id,
            asset,
            assets_dir,
            tx,
        }
    }
}

impl IntoFuture for CheckAssetTask {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let path = self.assets_dir.join(self.asset.name());

            self.tx.send(Progress::Start {
                id: self.id,
                name: self.asset.name().to_owned(),
                size: 0,
            })?;

            if !path.exists() {
                self.tx.send(Progress::Error {
                    id: self.id,
                    msg: format!("File missing: {}", self.asset.name()),
                })?;
                return Ok(());
            }

            if let Err(e) = self
                .asset
                .verify_checksum(&path, Some((self.id, &self.tx)))
                .await
            {
                self.tx.send(Progress::Error {
                    id: self.id,
                    msg: e.to_string(),
                })?;
                return Err(e);
            }

            self.tx.send(Progress::Finish { id: self.id })?;
            Ok(())
        })
    }
}
