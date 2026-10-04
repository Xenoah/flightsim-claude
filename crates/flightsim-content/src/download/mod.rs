//! Explicit, hash-pinned public GitHub acquisition for prepared schema-v1 ZIPs.
//!
//! Enable the `downloads` Cargo feature. This synchronous API belongs on a worker,
//! never a render thread. It stages through the unchanged strict local importer;
//! the caller still decides whether to commit, inspect and activate a new flight.
//! HTTPS and checksums establish transport/integrity, not licensing or authorship.
mod http;
mod source;
#[cfg(test)]
mod tests;

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Instant,
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ImportProgress, MAX_ARCHIVE_BYTES, StagedPackage, install::check_directory_path};
use http::{GitHubHttp, Http, NETWORK_TIMEOUT};
pub use source::DownloadSource;

const CHUNK: usize = 32 * 1024;
const MAX_RECEIPT_BYTES: u64 = 8192;
const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_ROOT_ENTRIES: usize = 128;
/// Completed archive bytes only. Active download/snapshot plus importer staging
/// can need an additional three archive budgets. No automatic eviction occurs.
pub const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PACKAGE: &str = "package.zip";
const RECEIPT: &str = "source.json";

type Result<T> = std::result::Result<T, DownloadError>;

#[derive(Debug)]
pub enum DownloadError {
    Package(crate::Error),
    InvalidSource(&'static str),
    Network(&'static str),
    HttpStatus(u16),
    Limit(&'static str),
    HashMismatch,
    CacheMiss,
    CacheBusy,
    CacheCorrupt(&'static str),
    Cancelled,
}
impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Package(e) => write!(f, "{e}"),
            Self::InvalidSource(s) => write!(f, "invalid download source: {s}"),
            Self::Network(s) => write!(f, "package download: {s}"),
            Self::HttpStatus(s) => write!(f, "package download HTTP status {s}"),
            Self::Limit(s) => write!(f, "package download exceeds {s} limit"),
            Self::HashMismatch => {
                f.write_str("archive SHA-256 does not match the requested source")
            }
            Self::CacheMiss => f.write_str("verified package is not available offline"),
            Self::CacheBusy => {
                f.write_str("download cache is busy; retry after the current job finishes")
            }
            Self::CacheCorrupt(s) => write!(f, "download cache is invalid: {s}"),
            Self::Cancelled => f.write_str("package download cancelled"),
        }
    }
}
impl std::error::Error for DownloadError {}
impl From<crate::Error> for DownloadError {
    fn from(error: crate::Error) -> Self {
        if matches!(error, crate::Error::Cancelled) {
            Self::Cancelled
        } else {
            Self::Package(error)
        }
    }
}
impl From<std::io::Error> for DownloadError {
    fn from(error: std::io::Error) -> Self {
        Self::Package(error.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    /// Reverify a matching cache entry; contact GitHub only on a clean miss.
    PreferCache,
    /// Never construct an HTTP client, resolve DNS or contact GitHub.
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadProgress {
    CheckingCache,
    Connecting {
        redirect: usize,
    },
    /// Bytes refer to archive bytes, not extracted bytes. Total may be unknown.
    Receiving {
        bytes_done: u64,
        bytes_total: Option<u64>,
    },
    VerifyingCache {
        bytes_done: u64,
        bytes_total: u64,
    },
    Importing(ImportProgress),
    /// Cancellation here drops staging and prevents any new cache publication.
    Ready {
        cache_hit: bool,
    },
}

/// Download provenance stays separate from the package's manifest identity.
/// `staged.commit()` explicitly installs; it never selects or activates terrain.
#[derive(Debug)]
pub struct StagedDownload {
    pub staged: StagedPackage,
    pub source: DownloadSource,
    pub cache_hit: bool,
}

/// Fetch or reverify a prepared package, then stage it using every local ZIP rule.
///
/// The caller supplies trusted, user-owned local cache and package-store roots.
/// A separate nonblocking cache lock serializes cooperating download jobs. Local
/// ZIP imports/commits are not blocked by network waits. Failures/cancellation
/// remove only this job's temporary directories; valid completed cache survives.
/// Retry by calling again; there is no implicit retry, resume, update or eviction.
/// A corrupt cache fails closed even online; repair/remove it explicitly.
pub fn stage_github_with_progress(
    source: &DownloadSource,
    cache: &Path,
    store: &Path,
    mode: CacheMode,
    callback: impl FnMut(DownloadProgress) -> bool,
) -> Result<StagedDownload> {
    stage_with_http(source, cache, store, mode, callback, &mut GitHubHttp)
}

fn report(
    callback: &mut impl FnMut(DownloadProgress) -> bool,
    progress: DownloadProgress,
) -> Result<()> {
    if callback(progress) {
        Ok(())
    } else {
        Err(DownloadError::Cancelled)
    }
}

fn stage_with_http(
    source: &DownloadSource,
    cache: &Path,
    store: &Path,
    mode: CacheMode,
    mut callback: impl FnMut(DownloadProgress) -> bool,
    http: &mut impl Http,
) -> Result<StagedDownload> {
    report(&mut callback, DownloadProgress::CheckingCache)?;
    check_directory_path(cache)?;
    check_directory_path(store)?;
    if !cache.exists() && mode == CacheMode::Offline {
        return Err(DownloadError::CacheMiss);
    }
    std::fs::create_dir_all(cache)?;
    std::fs::create_dir_all(store)?;
    let cache_path = std::fs::canonicalize(cache)?;
    let store_path = std::fs::canonicalize(store)?;
    if cache_path.starts_with(&store_path) || store_path.starts_with(&cache_path) {
        return Err(DownloadError::InvalidSource(
            "cache and store roots must not overlap",
        ));
    }
    let _lock = cache_lock(cache)?;
    let entry = cache.join(source.key());
    let cache_hit = match std::fs::symlink_metadata(&entry) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => return Err(DownloadError::CacheCorrupt("entry is not a directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if !cache_hit && mode == CacheMode::Offline {
        return Err(DownloadError::CacheMiss);
    }
    let remaining = cache_budget(cache, cache_hit)?;
    let temp = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(cache)?;
    let archive = temp.path().join(PACKAGE);
    let expected_receipt = if cache_hit {
        let receipt = read_receipt(&entry, source)?;
        let file = regular_file(&entry.join(PACKAGE), MAX_ARCHIVE_BYTES)?;
        let total = file.metadata()?.len();
        copy_verified(file, &archive, source, total, &mut callback)?;
        Some(receipt)
    } else {
        receive(
            source,
            &archive,
            MAX_ARCHIVE_BYTES.min(remaining),
            &mut callback,
            http,
        )?;
        None
    };
    let staged = crate::stage_zip_with_progress(&archive, store, |p| {
        callback(DownloadProgress::Importing(p))
    })?;
    let receipt = Receipt::new(source, staged.identity());
    if expected_receipt
        .as_ref()
        .is_some_and(|expected| expected != &receipt)
    {
        return Err(DownloadError::CacheCorrupt("manifest identity changed"));
    }
    report(&mut callback, DownloadProgress::Ready { cache_hit })?;
    if !cache_hit {
        let bytes = serde_json::to_vec(&receipt).map_err(crate::Error::from)?;
        let mut file = File::create(temp.path().join(RECEIPT))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        // The cache lock covers existence check through same-filesystem rename.
        // Complete cache entries are immutable and are never silently replaced.
        std::fs::rename(temp.path(), entry)?;
    }
    Ok(StagedDownload {
        staged,
        source: source.clone(),
        cache_hit,
    })
}

fn cache_lock(cache: &Path) -> Result<File> {
    let path = cache.join(".download.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.is_file() {
            return Err(DownloadError::CacheCorrupt("lock is not a regular file"));
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock_exclusive().map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock
            || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        {
            DownloadError::CacheBusy
        } else {
            e.into()
        }
    })?;
    Ok(file)
}

fn regular_file(path: &Path, max: u64) -> Result<File> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(DownloadError::CacheCorrupt("nonregular file"));
    }
    if meta.len() > max {
        return Err(DownloadError::Limit("cached file bytes"));
    }
    Ok(File::open(path)?)
}

// Scan a bounded number of entries; incomplete job dirs are counted but never
// recursively visited or cleaned. The cache is not a sandbox against its owner.
fn cache_budget(cache: &Path, hit: bool) -> Result<u64> {
    let mut count = 0;
    let mut bytes = 0u64;
    for (n, entry) in std::fs::read_dir(cache)?.enumerate() {
        if n >= MAX_CACHE_ROOT_ENTRIES {
            return Err(DownloadError::Limit("cache root entries"));
        }
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".download.lock" || name.starts_with(".download-") {
            continue;
        }
        if !source::is_sha256(&name) || !entry.file_type()?.is_dir() {
            return Err(DownloadError::CacheCorrupt("unexpected cache entry"));
        }
        count += 1;
        bytes = bytes
            .checked_add(
                regular_file(&entry.path().join(PACKAGE), MAX_ARCHIVE_BYTES)?
                    .metadata()?
                    .len(),
            )
            .ok_or(DownloadError::Limit("cache bytes"))?;
        if bytes > MAX_CACHE_BYTES {
            return Err(DownloadError::Limit("cache bytes"));
        }
    }
    if count > MAX_CACHE_ENTRIES || (!hit && count == MAX_CACHE_ENTRIES) {
        return Err(DownloadError::Limit("cache entries"));
    }
    Ok(MAX_CACHE_BYTES - bytes)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    source_url: String,
    archive_sha256: String,
    package_id: String,
    package_version: String,
    manifest_sha256: String,
}
impl Receipt {
    fn new(source: &DownloadSource, identity: &crate::RegionalIdentity) -> Self {
        Self {
            schema_version: 1,
            source_url: source.url().into(),
            archive_sha256: source.archive_sha256().into(),
            package_id: identity.id.clone(),
            package_version: identity.version.clone(),
            manifest_sha256: identity.manifest_sha256.clone(),
        }
    }
}
fn read_receipt(entry: &Path, source: &DownloadSource) -> Result<Receipt> {
    let mut count = 0;
    for child in std::fs::read_dir(entry)? {
        count += 1;
        if count > 2 {
            return Err(DownloadError::CacheCorrupt("extra entry files"));
        }
        let child = child?;
        if child.file_name() != PACKAGE && child.file_name() != RECEIPT {
            return Err(DownloadError::CacheCorrupt("unexpected entry file"));
        }
    }
    let mut data = Vec::new();
    regular_file(&entry.join(RECEIPT), MAX_RECEIPT_BYTES)?
        .take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(DownloadError::Limit("cache receipt bytes"));
    }
    let receipt: Receipt = serde_json::from_slice(&data)
        .map_err(|_| DownloadError::CacheCorrupt("invalid source receipt"))?;
    if receipt.schema_version != 1
        || receipt.source_url != source.url()
        || receipt.archive_sha256 != source.archive_sha256()
    {
        return Err(DownloadError::CacheCorrupt("source identity mismatch"));
    }
    Ok(receipt)
}

fn copy_verified(
    mut input: File,
    output: &Path,
    source: &DownloadSource,
    total: u64,
    callback: &mut impl FnMut(DownloadProgress) -> bool,
) -> Result<()> {
    let mut output = File::create(output)?;
    let mut hash = Sha256::new();
    let mut done = 0;
    let mut chunk = [0; CHUNK];
    loop {
        report(
            callback,
            DownloadProgress::VerifyingCache {
                bytes_done: done,
                bytes_total: total,
            },
        )?;
        let max = usize::try_from((MAX_ARCHIVE_BYTES - done + 1).min(CHUNK as u64))
            .expect("bounded chunk");
        let n = input.read(&mut chunk[..max])?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > MAX_ARCHIVE_BYTES || done > total {
            return Err(DownloadError::Limit("archive bytes"));
        }
        output.write_all(&chunk[..n])?;
        hash.update(&chunk[..n]);
    }
    if done != total || format!("{:x}", hash.finalize()) != source.archive_sha256() {
        return Err(DownloadError::HashMismatch);
    }
    output.sync_all()?;
    Ok(())
}

fn receive(
    source: &DownloadSource,
    output: &Path,
    limit: u64,
    callback: &mut impl FnMut(DownloadProgress) -> bool,
    http: &mut impl Http,
) -> Result<()> {
    if limit == 0 {
        return Err(DownloadError::Limit("cache bytes"));
    }
    let start = Instant::now();
    let mut url = url::Url::parse(source.url()).expect("validated source");
    for redirect in 0..=3 {
        report(callback, DownloadProgress::Connecting { redirect })?;
        let remaining =
            NETWORK_TIMEOUT
                .checked_sub(start.elapsed())
                .ok_or(DownloadError::Network(
                    "request timed out; retry explicitly",
                ))?;
        let mut response = http.get(&url, remaining)?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if redirect == 3 {
                return Err(DownloadError::Limit("redirect count"));
            }
            let mut locations = response.headers.get_all("location").iter();
            let location = locations
                .next()
                .ok_or(DownloadError::Network("missing redirect location"))?;
            if locations.next().is_some() {
                return Err(DownloadError::Network("multiple redirect locations"));
            }
            let next = source.redirect(
                location
                    .to_str()
                    .map_err(|_| DownloadError::InvalidSource("invalid redirect location"))?,
            )?;
            if next == url {
                return Err(DownloadError::Network("redirect loop"));
            }
            url = next;
            continue;
        }
        if response.status != 200 {
            return Err(DownloadError::HttpStatus(response.status));
        }
        http::validate_encoding(&response.headers)?;
        let total = http::content_length(&response.headers)?;
        if total.is_some_and(|n| n > limit) {
            return Err(DownloadError::Limit("archive/cache bytes"));
        }
        let mut output = File::create(output)?;
        let mut hash = Sha256::new();
        let mut done = 0;
        let mut chunk = [0; CHUNK];
        loop {
            report(
                callback,
                DownloadProgress::Receiving {
                    bytes_done: done,
                    bytes_total: total,
                },
            )?;
            if start.elapsed() >= NETWORK_TIMEOUT {
                return Err(DownloadError::Network(
                    "request timed out; retry explicitly",
                ));
            }
            let max = usize::try_from((limit - done + 1).min(CHUNK as u64)).expect("bounded chunk");
            let n = response.body.read(&mut chunk[..max]).map_err(|_| {
                DownloadError::Network("body read failed or timed out; retry explicitly")
            })?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > limit || total.is_some_and(|total| done > total) {
                return Err(DownloadError::Limit("archive/cache bytes"));
            }
            output.write_all(&chunk[..n])?;
            hash.update(&chunk[..n]);
        }
        if total.is_some_and(|total| total != done) {
            return Err(DownloadError::Network("truncated HTTP body"));
        }
        if format!("{:x}", hash.finalize()) != source.archive_sha256() {
            return Err(DownloadError::HashMismatch);
        }
        output.sync_all()?;
        return Ok(());
    }
    unreachable!("redirect loop returns at cap")
}
