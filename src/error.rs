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
}
