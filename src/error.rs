use crate::progress::Progress;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UTF-8: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error("template: {0}")]
    Template(#[from] indicatif::style::TemplateError),

    #[error("send: {0}")]
    Send(#[from] tokio::sync::mpsc::error::SendError<Progress>),

    #[error("cargo metadata: {0}")]
    CargoMetadata(String),

    #[error("checksum mismatch: {0}")]
    Checksum(String),

    #[error("download: {0}")]
    Download(#[from] DownloadError),

    #[error("verification: {0}")]
    Verification(#[from] VerificationError),
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("failed to download {name} chunk ({start}-{end}): {source}")]
    Chunk {
        name: String,
        start: u64,
        end: u64,
        source: reqwest::Error,
    },
    #[error("failed to initialize download for {name}: {source}")]
    Init {
        name: String,
        source: reqwest::Error,
    },
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("checksum mismatch for {name}: expected {expected}, got {actual}")]
    Mismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("failed to read {name} for verification: {source}")]
    Io {
        name: String,
        source: std::io::Error,
    },
}
