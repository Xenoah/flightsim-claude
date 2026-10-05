use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use flightsim_world::{
    dem::{
        DemTile,
        io::{HEADER_LEN, MAX_GRID_DIMENSION, read_tile, tile_relative_path},
    },
    terrain::{TerrainError, TileSource},
    tile::TileId,
};
use fs2::FileExt;

use crate::{
    Error, FileKind, FileRecord, MANIFEST_NAME, MAX_ARCHIVE_ENTRIES, MAX_MANIFEST_BYTES, Manifest,
    RegionalIdentity, Result,
    manifest::{PortablePaths, file_byte_limit, validate_id},
    parse_manifest, sha256,
};

#[derive(Debug)]
pub struct StagedPackage {
    temp: tempfile::TempDir,
    store: PathBuf,
    manifest: Manifest,
    identity: RegionalIdentity,
}
impl StagedPackage {
    pub(crate) fn new(
        temp: tempfile::TempDir,
        store: PathBuf,
        manifest: Manifest,
        bytes: &[u8],
    ) -> Self {
        let identity = identity(&manifest, bytes);
        Self {
            temp,
            store,
            manifest,
            identity,
        }
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn identity(&self) -> &RegionalIdentity {
        &self.identity
    }

    /// Publish once, without replacing any existing version. Dropping instead cancels.
    /// A process crash before the same-filesystem rename leaves an ignored .import-*
    /// directory, never a version reservation. Locks are released by the OS on exit.
    pub fn commit(self) -> Result<InstalledPackage> {
        let target = publish_directory(
            &self.temp,
            &self.store,
            &self.manifest.id,
            &self.manifest.version,
            || Ok(list_installed(&self.store)?.len()),
        )?;
        Ok(InstalledPackage {
            directory: target,
            manifest: self.manifest,
            identity: self.identity,
        })
    }
}

/// Atomic, cooperating-importer no-overwrite publication in a trusted local store.
pub(crate) fn publish_directory(
    temp: &tempfile::TempDir,
    store: &Path,
    id: &str,
    version: &str,
    count_versions: impl FnOnce() -> Result<usize>,
) -> Result<PathBuf> {
    check_directory_path(store)?;
    let lock_path = store.join(".install.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&lock_path) {
        if is_link_or_reparse(&meta) || !meta.is_file() {
            return Err(Error::Invalid("store lock is not a regular file".into()));
        }
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock_exclusive().map_err(|e| {
        // Windows reports ERROR_LOCK_VIOLATION, whose Rust ErrorKind is
        // not WouldBlock. Use fs2's platform contract, not a guessed code.
        if e.kind() == std::io::ErrorKind::WouldBlock
            || e.raw_os_error()
                .is_some_and(|code| Some(code) == fs2::lock_contended_error().raw_os_error())
        {
            Error::StoreBusy
        } else {
            Error::Io(e)
        }
    })?;
    let parent = store.join(id);
    check_directory_path(&parent)?;
    let target = parent.join(version);
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Err(Error::AlreadyInstalled(target)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if count_versions()? >= 256 {
        return Err(Error::Limit("installed version count"));
    }
    std::fs::create_dir_all(&parent)?;
    // This store is trusted local application state. The OS lock serializes all
    // cooperating importers; archive contents cannot choose the destination.
    std::fs::rename(temp.path(), &target)?;
    Ok(target)
}

/// Validated metadata only. Call inspect_installed before activation.
#[derive(Debug, Clone)]
pub struct InstalledSummary {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub identity: RegionalIdentity,
}

/// Immutable through this API. The store owner must not modify installed data.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    directory: PathBuf,
    manifest: Manifest,
    identity: RegionalIdentity,
}
impl InstalledPackage {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn terrain_directory(&self) -> PathBuf {
        self.directory.join("terrain")
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn identity(&self) -> &RegionalIdentity {
        &self.identity
    }
    /// Always fails until a future replay schema actually stores and checks regional identity.
    /// Do not write legacy v1/v2 replays or play them against package-backed terrain.
    pub fn require_replay_support(&self) -> Result<()> {
        Err(Error::ReplayUnsupported)
    }
    /// Only declared, hash-checked DEMs can be loaded. Wrap this in the existing world
    /// global fallback source; no fallback or physical sampling policy changes here.
    pub fn tile_source(&self) -> PackageTileSource {
        let files = self
            .manifest
            .files
            .iter()
            .filter(|f| f.kind == FileKind::TerrainDem)
            .map(|f| (f.path.clone(), f.clone()))
            .collect();
        PackageTileSource {
            directory: self.directory.clone(),
            files: Arc::new(files),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageTileSource {
    directory: PathBuf,
    files: Arc<BTreeMap<String, FileRecord>>,
}
impl TileSource for PackageTileSource {
    fn load(&self, id: TileId) -> std::result::Result<Option<DemTile>, TerrainError> {
        let path = format!(
            "terrain/{}",
            tile_relative_path(id).to_string_lossy().replace('\\', "/")
        );
        let Some(file) = self.files.get(&path) else {
            return Ok(None);
        };
        read_dem(&self.directory, file)
            .map(|tile| Some(tile.tile))
            .map_err(|error| TerrainError::Io {
                path: self.directory.join(path),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()),
            })
    }
}

/// List at most 256 versions. Staging and lock files are intentionally invisible.
/// Payloads are not loaded here; the returned summaries are not activation tokens.
pub fn list_installed(store: &Path) -> Result<Vec<InstalledSummary>> {
    check_directory_path(store)?;
    if !store.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut visited = 0usize;
    for package in std::fs::read_dir(store)? {
        let package = package?;
        visited += 1;
        if visited > 1024 {
            return Err(Error::Limit("store entry count"));
        }
        let name = package.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        validate_id(&name)?;
        if !package.file_type()?.is_dir() {
            return Err(Error::Invalid("non-directory package ID".into()));
        }
        for version in std::fs::read_dir(package.path())? {
            let version = version?;
            if !version.file_type()?.is_dir() {
                return Err(Error::Invalid("non-directory package version".into()));
            }
            if result.len() >= 256 {
                return Err(Error::Limit("installed version count"));
            }
            result.push(read_summary(&version.path())?);
        }
    }
    result.sort_by(|a, b| {
        (&a.identity.id, &a.identity.version).cmp(&(&b.identity.id, &b.identity.version))
    });
    Ok(result)
}

/// Fully validate an installed version before a new-flight transaction. Any failure
/// leaves the caller's previous active flight and the global fallback untouched.
pub fn inspect_installed(directory: &Path) -> Result<InstalledPackage> {
    inspect_installed_with_progress(directory, |_| true)
}

/// Cancellable full inspection for application workers. Cancellation is checked
/// before metadata, for every tree entry and between bounded payload validations
/// (at most one 2 MiB DEM and its 1,048,576 samples between checks).
pub fn inspect_installed_with_progress(
    directory: &Path,
    mut callback: impl FnMut(crate::ImportProgress) -> bool,
) -> Result<InstalledPackage> {
    let mut progress = crate::ImportProgress {
        phase: crate::ImportPhase::Inspecting,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
    };
    crate::notify(&mut callback, progress)?;
    let summary = read_summary(directory)?;
    progress.files_total = summary.manifest.files.len();
    progress.bytes_total = summary
        .manifest
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum();
    verify_tree(directory, &summary.manifest, || {
        crate::notify(&mut callback, progress)
    })?;
    progress.phase = crate::ImportPhase::Validating;
    for file in &summary.manifest.files {
        crate::notify(&mut callback, progress)?;
        validate_payload(directory, file, &summary.manifest)?;
        progress.files_done += 1;
        progress.bytes_done += file.size_bytes;
    }
    progress.phase = crate::ImportPhase::Ready;
    crate::notify(&mut callback, progress)?;
    Ok(InstalledPackage {
        directory: directory.to_owned(),
        manifest: summary.manifest,
        identity: summary.identity,
    })
}

fn read_summary(directory: &Path) -> Result<InstalledSummary> {
    check_directory_path(directory)?;
    let bytes = read_bounded_file(&directory.join(MANIFEST_NAME), MAX_MANIFEST_BYTES as u64)?;
    let manifest = parse_manifest(&bytes)?;
    if directory.file_name().and_then(|s| s.to_str()) != Some(manifest.version.as_str())
        || directory
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some(manifest.id.as_str())
    {
        return Err(Error::Invalid(
            "installed ID/version does not match its directory".into(),
        ));
    }
    let identity = identity(&manifest, &bytes);
    Ok(InstalledSummary {
        directory: directory.to_owned(),
        manifest,
        identity,
    })
}

fn identity(manifest: &Manifest, bytes: &[u8]) -> RegionalIdentity {
    RegionalIdentity {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        manifest_sha256: sha256(bytes),
    }
}

fn verify_tree(
    root: &Path,
    manifest: &Manifest,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut expected: BTreeSet<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
    expected.insert(MANIFEST_NAME.into());
    let allowed_dirs: BTreeSet<String> = expected
        .iter()
        .flat_map(|p| {
            let parts: Vec<_> = p.split('/').collect();
            (1..parts.len())
                .map(|i| parts[..i].join("/"))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut stack = vec![root.to_owned()];
    let mut paths = PortablePaths::default();
    let mut count = 0;
    while let Some(directory) = stack.pop() {
        for item in std::fs::read_dir(directory)? {
            checkpoint()?;
            let item = item?;
            count += 1;
            if count > MAX_ARCHIVE_ENTRIES {
                return Err(Error::Limit("installed entry count"));
            }
            let path = item.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::Invalid("path escaped install".into()))?;
            let relative = relative
                .to_str()
                .ok_or_else(|| Error::Invalid("non-UTF8 installed path".into()))?
                .replace('\\', "/");
            let file_type = item.file_type()?;
            paths.add(&relative, file_type.is_dir())?;
            if file_type.is_dir() {
                if !allowed_dirs.contains(&relative) {
                    return Err(Error::Invalid("undeclared installed directory".into()));
                }
                stack.push(path);
            } else if !file_type.is_file() || !expected.remove(&relative) {
                return Err(Error::Invalid(
                    "undeclared, linked or special installed file".into(),
                ));
            }
        }
    }
    if !expected.is_empty() {
        return Err(Error::Invalid("installed package file missing".into()));
    }
    Ok(())
}

pub(crate) fn validate_payload(root: &Path, file: &FileRecord, manifest: &Manifest) -> Result<()> {
    match file.kind {
        FileKind::Documentation => {
            let bytes = verified_bytes(root, file)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::Invalid("documentation must be UTF-8 text".into()))?;
            if text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(Error::Invalid(
                    "documentation contains control bytes".into(),
                ));
            }
        }
        FileKind::TerrainDem => {
            let stored = read_dem(root, file)?;
            let tile = stored.id.bounds();
            let bounds = &manifest.terrain.bounds_degrees;
            let (west, east, south, north) = (
                tile.west.to_degrees().get(),
                tile.east.to_degrees().get(),
                tile.south.to_degrees().get(),
                tile.north.to_degrees().get(),
            );
            let inside_lon = if bounds.west < bounds.east {
                west + 1e-9 >= bounds.west && east - 1e-9 <= bounds.east
            } else {
                west + 1e-9 >= bounds.west || east - 1e-9 <= bounds.east
            };
            if !inside_lon || south + 1e-9 < bounds.south || north - 1e-9 > bounds.north {
                return Err(Error::Invalid(format!(
                    "tile lies outside declared coverage: {}",
                    file.path
                )));
            }
        }
    }
    Ok(())
}

fn verified_bytes(root: &Path, file: &FileRecord) -> Result<Vec<u8>> {
    let path = root.join(&file.path);
    if let Some(parent) = path.parent() {
        check_directory_path(parent)?;
    }
    let bytes = read_bounded_file(&path, file_byte_limit(&file.path)?)?;
    if bytes.len() as u64 != file.size_bytes || sha256(&bytes) != file.sha256 {
        return Err(Error::Invalid(format!(
            "size/SHA-256 mismatch: {}",
            file.path
        )));
    }
    Ok(bytes)
}

fn read_dem(root: &Path, file: &FileRecord) -> Result<flightsim_world::dem::io::StoredTile> {
    let bytes = verified_bytes(root, file)?;
    if bytes.len() < HEADER_LEN {
        return Err(Error::Invalid("truncated DEM header".into()));
    }
    let width = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let samples = u64::from(width) * u64::from(height);
    if !(2..=MAX_GRID_DIMENSION).contains(&width)
        || !(2..=MAX_GRID_DIMENSION).contains(&height)
        || samples > 1024 * 1024
        || HEADER_LEN as u64 + samples * 2 != bytes.len() as u64
    {
        return Err(Error::Invalid(
            "DEM dimensions/size exceed package budget or do not match".into(),
        ));
    }
    // All allocation-sensitive header fields are bounded before calling the shared runtime reader.
    let stored = read_tile(&mut Cursor::new(&bytes))
        .map_err(|e| Error::Invalid(format!("DEM runtime reader: {e}")))?;
    let canonical = format!(
        "terrain/{}",
        tile_relative_path(stored.id)
            .to_string_lossy()
            .replace('\\', "/")
    );
    if canonical != file.path {
        return Err(Error::Invalid("DEM path/header tile ID mismatch".into()));
    }
    for row in 0..height {
        for col in 0..width {
            let height = stored.tile.grid().sample_at(col, row).get();
            if !height.is_finite() || !(-12_000.0..=100_000.0).contains(&height) {
                return Err(Error::Invalid(
                    "DEM elevation outside supported physical domain".into(),
                ));
            }
        }
    }
    // The writer derives decimation error from this decoded grid. Bilinear
    // interpolation is bounded by its extrema, so a legitimate error cannot
    // exceed the validated elevation range (apart from floating-point roundoff).
    // Bounding heights alone is insufficient: render skirts multiply this header
    // value and eventually cast positions to f32.
    let (minimum, maximum) = stored.tile.grid().elevation_range();
    let maximum_error = maximum.get() - minimum.get() + 1e-6;
    if stored.tile.geometric_error().get() > maximum_error {
        return Err(Error::Invalid(
            "DEM geometric error exceeds decoded elevation range".into(),
        ));
    }
    Ok(stored)
}

pub(crate) fn read_bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() > limit {
        return Err(Error::Limit("regular file bytes"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::Limit("actual file bytes"));
    }
    Ok(bytes)
}

pub(crate) fn check_directory_path(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if is_link_or_reparse(&meta) || !meta.is_dir() => {
                return Err(Error::Invalid(
                    "package store/directory contains symlink or non-directory".into(),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Symlinks, multiply linked regular Unix files and all Windows reparse points.
pub(crate) fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.is_file() && metadata.nlink() > 1
    }
    #[cfg(not(any(windows, unix)))]
    {
        metadata.file_type().is_symlink()
    }
}
