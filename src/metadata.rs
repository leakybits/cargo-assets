use camino::Utf8PathBuf;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    pub target_directory: Utf8PathBuf,
    pub metadata: Metadata,
}

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub assets: Vec<Asset>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
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
