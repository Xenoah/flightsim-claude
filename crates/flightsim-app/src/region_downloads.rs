//! Opt-in application catalog. All acquisition stays in flightsim-content.
use super::*;
use flightsim_content::download::{CacheMode, DownloadProgress, DownloadSource};
use flightsim_content::{BoundsDegrees, Manifest};
use serde::Deserialize;
use std::{collections::BTreeSet, io::Read, path::Path};

const MAX_CATALOG_BYTES: u64 = 256 * 1024;
const MAX_CATALOG_ENTRIES: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: u32,
    regions: Vec<CatalogRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRecord {
    id: String,
    version: String,
    title: String,
    bounds_degrees: BoundsDegrees,
    url: String,
    archive_sha256: String,
    provenance: String,
}

/// Validated catalog claims, still untrusted until the downloaded manifest matches.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Entry {
    pub key: String,
    pub title: String,
    pub bounds: BoundsDegrees,
    pub source: DownloadSource,
    provenance: String,
}

impl Entry {
    pub fn preview(&self, offline: bool) -> String {
        format!(
            "CATALOG CLAIMS / PREVIEW ONLY\n{}\n{}\nSource ZIP (only fetched on explicit Download):\n{}\nArchive SHA256:\n{}\nDeclared bounds: {},{} to {},{}\nPublisher-declared provenance:\n{}\n\n{}\nHash integrity is not publisher authentication or rights clearance. Read installed source/license notices before use. Catalog bounds do not promise complete tile coverage. Installing never activates terrain.",
            self.title,
            self.key,
            self.source.url(),
            self.source.archive_sha256(),
            self.bounds.west,
            self.bounds.south,
            self.bounds.east,
            self.bounds.north,
            self.provenance,
            if offline {
                "Offline mode: every attempt uses verified cache only."
            } else {
                "Download rechecks cache first, then uses GitHub on a clean miss. Cached only never contacts the network."
            },
        )
    }

    pub fn center(&self) -> Geodetic {
        let east = if self.bounds.east < self.bounds.west {
            self.bounds.east + 360.0
        } else {
            self.bounds.east
        };
        Geodetic::new(
            Degrees((self.bounds.south + self.bounds.north) * 0.5).to_radians(),
            Degrees((self.bounds.west + east) * 0.5)
                .to_radians()
                .wrap_signed(),
            Meters::ZERO,
        )
    }

    fn check_manifest(&self, manifest: &Manifest) -> Result<(), String> {
        if self.key != format!("{}@{}", manifest.id, manifest.version)
            || self.title != manifest.title
            || self.bounds != manifest.terrain.bounds_degrees
        {
            return Err("Downloaded manifest disagrees with catalog identity/title/bounds; nothing installed. Correct the catalog and Refresh".into());
        }
        Ok(())
    }
}

pub(super) struct DownloadRuntime {
    pub path: PathBuf,
    pub cache: Result<PathBuf, String>,
    pub offline: bool,
    pub entries: Vec<Entry>,
    pub selected: Option<Entry>,
}

impl DownloadRuntime {
    pub fn new(options: &Options) -> Option<Self> {
        options.catalog.as_ref().map(|path| Self {
            path: path.clone(),
            cache: options.cache.clone().map_or_else(
                || default_store().map(|store| store.with_file_name("region-download-cache")),
                Ok,
            ),
            offline: options.offline,
            entries: Vec::new(),
            selected: None,
        })
    }

    pub fn select(&mut self, key: &str, map: &mut WorldMapState) -> Result<(), String> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.key == key)
            .ok_or("Catalog region is no longer available. Refresh and choose again")?;
        map.select(entry.center(), &entry.title);
        map.regions.download_selected = Some(entry.key.clone());
        map.regions.download_credits = entry.preview(self.offline);
        self.selected = Some(entry.clone());
        map.regions.status = "Catalog preview only. Download/Retry or Cached only installs; Installed then Start activates".into();
        Ok(())
    }

    pub fn set_entries(&mut self, entries: Vec<Entry>, map: &mut WorldMapState) {
        // A refreshed source/hash/claim must be explicitly selected again.
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| !entries.contains(selected))
        {
            self.selected = None;
            map.regions.download_selected = None;
            map.regions.download_credits.clear();
        }
        map.regions
            .set_downloads(entries.iter().map(|entry| RegionSummary {
                key: entry.key.clone(),
                name: entry.title.clone(),
            }));
        self.entries = entries;
    }

    pub fn job(&self, offline: bool) -> Result<Job, String> {
        Ok(Job::Download {
            entry: self
                .selected
                .clone()
                .ok_or("Choose a catalog region first")?,
            cache: self.cache.clone()?,
            mode: if offline || self.offline {
                CacheMode::Offline
            } else {
                CacheMode::PreferCache
            },
        })
    }
}

pub(super) fn read_catalog(path: &Path) -> Result<Vec<Entry>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("Catalog: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_CATALOG_BYTES {
        return Err("Catalog must be a regular, non-symlink file of at most 256 KiB".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_CATALOG_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("Catalog: {e}"))?;
    parse_catalog(&bytes)
}

fn parse_catalog(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err("Catalog exceeds 256 KiB".into());
    }
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid region catalog: {e}"))?;
    if catalog.schema_version != 1 || catalog.regions.len() > MAX_CATALOG_ENTRIES {
        return Err("Catalog requires schema_version 1 and at most 64 regions".into());
    }
    let mut keys = BTreeSet::new();
    let mut sources = BTreeSet::new();
    catalog
        .regions
        .into_iter()
        .map(|record| {
            let key = format!("{}@{}", record.id, record.version);
            let stem = record.id.split('.').next().unwrap_or("");
            let reserved = [
                "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
                "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
                "lpt9",
            ]
            .contains(&stem);
            if !valid_key(&key) || record.id.ends_with('.') || reserved || !keys.insert(key.clone())
            {
                return Err("Catalog contains invalid, nonportable or duplicate ID@VERSION".into());
            }
            bounded_text(&record.title, 160, false)?;
            bounded_text(&record.provenance, 8192, true)?;
            let b = &record.bounds_degrees;
            if ![b.west, b.south, b.east, b.north]
                .into_iter()
                .all(f64::is_finite)
                || !(-180.0..=180.0).contains(&b.west)
                || !(-180.0..=180.0).contains(&b.east)
                || !(-90.0..=90.0).contains(&b.south)
                || !(-90.0..=90.0).contains(&b.north)
                || b.south >= b.north
                || b.west.to_bits() == b.east.to_bits()
                || (b.west - b.east).abs() < f64::EPSILON
                || (b.west >= 180.0 && b.east <= -180.0)
            {
                return Err("Catalog has invalid geographic bounds".into());
            }
            let source = DownloadSource::github(&record.url, &record.archive_sha256)
                .map_err(|e| e.to_string())?;
            if !sources.insert(record.url) {
                return Err("Catalog contains a duplicate archive source".into());
            }
            Ok(Entry {
                key,
                title: record.title,
                bounds: record.bounds_degrees,
                source,
                provenance: record.provenance,
            })
        })
        .collect()
}

fn bounded_text(value: &str, max: usize, multiline: bool) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() && !(multiline && c == '\n'))
    {
        return Err("Catalog has empty, oversized or control-containing text".into());
    }
    Ok(())
}

pub(super) fn download(
    entry: &Entry,
    cache: &Path,
    store: &Path,
    mode: CacheMode,
    cancel: &AtomicBool,
    progress: &Mutex<Option<Progress>>,
) -> Result<Outcome, String> {
    let downloaded = flightsim_content::download::stage_github_with_progress(
        &entry.source,
        cache,
        store,
        mode,
        |next| {
            if let Ok(mut progress) = progress.lock() {
                *progress = Some(Progress::Download(next));
            }
            !cancel.load(Ordering::Relaxed)
        },
    )
    .map_err(|e| e.to_string())?;
    entry.check_manifest(downloaded.staged.manifest())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Region download cancelled".into());
    }
    let cache_hit = downloaded.cache_hit;
    downloaded.staged.commit().map_err(|e| e.to_string())?;
    Ok(Outcome::Downloaded {
        installed: flightsim_content::list_installed(store).map_err(|e| e.to_string())?,
        key: entry.key.clone(),
        cache_hit,
    })
}

pub(super) fn progress_text(progress: DownloadProgress) -> (Option<u8>, String) {
    match progress {
        DownloadProgress::CheckingCache => (None, "Checking verified download cache".into()),
        DownloadProgress::Connecting { redirect } => (
            None,
            format!("Connecting to GitHub (redirect {redirect}); X cancels"),
        ),
        DownloadProgress::Receiving {
            bytes_done,
            bytes_total,
        } => (
            bytes_total.and_then(|total| percentage(bytes_done, total)),
            format!(
                "Downloading archive: {bytes_done} / {} bytes",
                bytes_total.map_or_else(|| "unknown".into(), |n| n.to_string())
            ),
        ),
        DownloadProgress::VerifyingCache {
            bytes_done,
            bytes_total,
        } => (
            percentage(bytes_done, bytes_total),
            format!("Verifying cached archive: {bytes_done} / {bytes_total} bytes"),
        ),
        DownloadProgress::Importing(p) => import_progress_text(p),
        DownloadProgress::Ready { cache_hit } => (
            Some(100),
            format!(
                "Verified package ready; cache hit {cache_hit}; checking catalog before install"
            ),
        ),
    }
}

#[cfg(test)]
#[path = "region_downloads_tests.rs"]
mod tests;
