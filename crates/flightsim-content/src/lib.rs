//! Bounded, data-only offline terrain and aircraft packages.
//!
//! Importing installs immutable versioned data; it never changes a live flight.
//! For terrain, the app activates at most one validated package in its new-flight transaction
//! and retain the existing global fallback. No package code runs. Network acquisition
//! is an explicit opt-in through the `downloads` feature and never activates terrain.
//! The separate [`aircraft`] contract provides static resource validation/basic
//! import; the app retains authoritative physical-profile validation.
pub mod aircraft;
mod archive;
#[cfg(feature = "downloads")]
pub mod download;
mod install;
mod manifest;

pub use archive::{install_zip, stage_zip_with_progress};
pub use install::{
    InstalledPackage, InstalledSummary, PackageTileSource, StagedPackage, inspect_installed,
    inspect_installed_with_progress, list_installed,
};
pub use manifest::{
    BoundsDegrees, ContentKind, FileKind, FileRecord, LicenseRecord, Manifest, SourceRecord,
    TerrainMetadata, parse_manifest,
};

pub const MANIFEST_NAME: &str = "manifest.json";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_FILES: usize = 4096;
pub const MAX_ARCHIVE_ENTRIES: usize = 8192;
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 256 * 1024;
/// Header plus at most 1024 * 1024 u16 height samples per tile.
pub const MAX_DEM_BYTES: u64 = 56 + 2 * 1024 * 1024;
pub const MAX_COMPRESSION_RATIO: u64 = 1024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Zip(rawzip::Error),
    Invalid(String),
    Limit(&'static str),
    Cancelled,
    AlreadyInstalled(std::path::PathBuf),
    ReplayUnsupported,
    StoreBusy,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "package I/O: {e}"),
            Self::Json(e) => write!(f, "package manifest: {e}"),
            Self::Zip(e) => write!(f, "package ZIP: {e}"),
            Self::Invalid(e) => write!(f, "invalid package: {e}"),
            Self::Limit(e) => write!(f, "package exceeds {e} limit"),
            Self::Cancelled => f.write_str("package import cancelled"),
            Self::AlreadyInstalled(p) => write!(f, "package version already exists: {}", p.display()),
            Self::StoreBusy => f.write_str("package store is busy; retry after the current import finishes"),
            Self::ReplayUnsupported => f.write_str("regional package replay requires a future schema with regional identity; replay v1/v2 cannot be used"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<rawzip::Error> for Error {
    fn from(e: rawzip::Error) -> Self {
        Self::Zip(e)
    }
}

/// Exact manifest bytes anchor every declared file hash. This is integrity, not publisher authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionalIdentity {
    pub id: String,
    pub version: String,
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPhase {
    Inspecting,
    Extracting,
    Validating,
    Ready,
}

/// Callback returns false to cancel. Byte progress counts uncompressed extraction only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportProgress {
    pub phase: ImportPhase,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub(crate) fn notify(
    callback: &mut impl FnMut(ImportProgress) -> bool,
    progress: ImportProgress,
) -> Result<()> {
    if callback(progress) {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
