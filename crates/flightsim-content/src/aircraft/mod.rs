//! Offline aircraft package v1. This is independent of terrain manifest v1.
//! The content layer verifies integrity and a closed static GLB subset. The app
//! must validate the original profile bytes with its existing physical-family
//! decoder before publication. Import never activates or changes a flight.
mod glb;
mod manifest;

use crate::archive::{ArchiveLimits, ArchiveRecord, stage_archive};
use crate::install::{
    check_directory_path, is_link_or_reparse, publish_directory, read_bounded_file,
};
use crate::{Error, ImportPhase, ImportProgress, Result, notify, sha256};
pub use glb::GeometrySummary;
pub use manifest::{FileKind, FileRecord, Manifest, ModelRecord, ProfileRecord, parse_manifest};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
pub const MAX_GLB_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_FILES: usize = 16;
pub const MAX_ENTRIES: usize = 64;

/// Exact manifest bytes bind all declared bytes. This is neither publisher
/// authentication, a physical-model identity, replay approval nor rights clearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    pub id: String,
    pub version: String,
    pub manifest_sha256: String,
}

/// A byte-validated stage has no publication API until the caller supplies its
/// authoritative semantic profile validator. Dropping either stage cancels it.
#[derive(Debug)]
pub struct StagedAircraftPackage {
    temp: tempfile::TempDir,
    store: PathBuf,
    manifest: Manifest,
    identity: PackageIdentity,
    profile_bytes: Vec<u8>,
    geometry: GeometrySummary,
}
#[derive(Debug)]
pub struct ValidatedAircraftPackage(StagedAircraftPackage);
#[derive(Debug, Clone)]
pub struct InstalledAircraftPackage {
    directory: PathBuf,
    manifest: Manifest,
    identity: PackageIdentity,
    profile_bytes: Vec<u8>,
    geometry: GeometrySummary,
}
impl StagedAircraftPackage {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn identity(&self) -> &PackageIdentity {
        &self.identity
    }
    pub fn geometry(&self) -> &GeometrySummary {
        &self.geometry
    }
    pub fn profile_bytes(&self) -> &[u8] {
        &self.profile_bytes
    }
    /// The callback must decode these exact bytes through the authoritative
    /// family loader and bind its version/model path/fit to the manifest/geometry.
    /// It must not interpret the package ID or rights metadata as physical identity.
    pub fn validate_profile(
        self,
        validate: impl FnOnce(&[u8], &Manifest, &GeometrySummary) -> Result<()>,
    ) -> Result<ValidatedAircraftPackage> {
        validate(&self.profile_bytes, &self.manifest, &self.geometry)?;
        Ok(ValidatedAircraftPackage(self))
    }
}
impl ValidatedAircraftPackage {
    pub fn identity(&self) -> &PackageIdentity {
        &self.0.identity
    }
    pub fn manifest(&self) -> &Manifest {
        &self.0.manifest
    }
    /// Publication is atomic and never overwrites. Caller checks cancellation or
    /// generation immediately before this call; a completed rename cannot be undone.
    pub fn commit(self) -> Result<InstalledAircraftPackage> {
        let staged = self.0;
        let directory = publish_directory(
            &staged.temp,
            &staged.store,
            &staged.manifest.id,
            &staged.manifest.version,
            || count_versions(&staged.store),
        )?;
        Ok(InstalledAircraftPackage {
            directory,
            manifest: staged.manifest,
            identity: staged.identity,
            profile_bytes: staged.profile_bytes,
            geometry: staged.geometry,
        })
    }
}
impl InstalledAircraftPackage {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn profile_path(&self) -> PathBuf {
        self.directory.join(&self.manifest.profile.path)
    }
    pub fn assets_directory(&self) -> PathBuf {
        self.directory.join("assets")
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn identity(&self) -> &PackageIdentity {
        &self.identity
    }
    pub fn profile_bytes(&self) -> &[u8] {
        &self.profile_bytes
    }
    pub fn geometry(&self) -> &GeometrySummary {
        &self.geometry
    }
}

pub fn stage_zip_with_progress(
    zip: &Path,
    store: &Path,
    mut callback: impl FnMut(ImportProgress) -> bool,
) -> Result<StagedAircraftPackage> {
    let staged = stage_archive(
        zip,
        store,
        ArchiveLimits {
            archive_bytes: MAX_TOTAL_BYTES,
            total_bytes: MAX_TOTAL_BYTES,
            files: MAX_FILES,
            entries: MAX_ENTRIES,
        },
        manifest::file_byte_limit,
        |bytes| {
            Ok(parse_manifest(bytes)?
                .files
                .into_iter()
                .map(|f| ArchiveRecord {
                    path: f.path,
                    size_bytes: f.size_bytes,
                    sha256: f.sha256,
                })
                .collect())
        },
        &mut callback,
    )?;
    let manifest = parse_manifest(&staged.manifest_bytes)?;
    let identity = identity(&manifest, &staged.manifest_bytes);
    let (profile_bytes, geometry) =
        validate_payloads(staged.temp.path(), &manifest, &mut callback)?;
    Ok(StagedAircraftPackage {
        temp: staged.temp,
        store: store.to_owned(),
        manifest,
        identity,
        profile_bytes,
        geometry,
    })
}

/// Fully recheck the immutable installed tree and exact bytes before using it.
/// Semantic profile validation is still the caller's responsibility.
pub fn inspect_installed(directory: &Path) -> Result<InstalledAircraftPackage> {
    inspect_installed_with_progress(directory, |_| true)
}
pub fn inspect_installed_with_progress(
    directory: &Path,
    mut callback: impl FnMut(ImportProgress) -> bool,
) -> Result<InstalledAircraftPackage> {
    check_directory_path(directory)?;
    let bytes = read_bounded_file(
        &directory.join(crate::MANIFEST_NAME),
        MAX_MANIFEST_BYTES as u64,
    )?;
    let manifest = parse_manifest(&bytes)?;
    if directory.file_name().and_then(|s| s.to_str()) != Some(manifest.version.as_str())
        || directory
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some(manifest.id.as_str())
    {
        return Err(Error::Invalid(
            "aircraft installed ID/version directory mismatch".into(),
        ));
    }
    let mut expected: BTreeSet<_> = manifest.files.iter().map(|f| f.path.clone()).collect();
    expected.insert(crate::MANIFEST_NAME.into());
    let allowed: BTreeSet<_> = expected
        .iter()
        .flat_map(|p| {
            let parts: Vec<_> = p.split('/').collect();
            (1..parts.len())
                .map(|i| parts[..i].join("/"))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut stack = vec![directory.to_owned()];
    let mut paths = crate::manifest::PortablePaths::with_limit(MAX_ENTRIES);
    let mut count = 0;
    while let Some(dir) = stack.pop() {
        for item in std::fs::read_dir(dir)? {
            notify(
                &mut callback,
                progress(ImportPhase::Inspecting, count, manifest.files.len()),
            )?;
            let item = item?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err(Error::Limit("aircraft tree entries"));
            }
            let path = item.path();
            let meta = std::fs::symlink_metadata(&path)?;
            if is_link_or_reparse(&meta) {
                return Err(Error::Invalid(
                    "aircraft tree links/reparse points forbidden".into(),
                ));
            }
            let relative = path
                .strip_prefix(directory)
                .map_err(|_| Error::Invalid("aircraft path escaped".into()))?
                .to_str()
                .ok_or_else(|| Error::Invalid("non-UTF8 path".into()))?
                .replace('\\', "/");
            paths.add(&relative, meta.is_dir())?;
            if meta.is_dir() {
                if !allowed.contains(&relative) {
                    return Err(Error::Invalid("undeclared aircraft directory".into()));
                }
                stack.push(path);
            } else if !meta.is_file() || !expected.remove(&relative) {
                return Err(Error::Invalid("undeclared/special aircraft file".into()));
            }
        }
    }
    if !expected.is_empty() {
        return Err(Error::Invalid("missing aircraft file".into()));
    }
    let (profile_bytes, geometry) = validate_payloads(directory, &manifest, &mut callback)?;
    Ok(InstalledAircraftPackage {
        directory: directory.to_owned(),
        identity: identity(&manifest, &bytes),
        manifest,
        profile_bytes,
        geometry,
    })
}
fn identity(manifest: &Manifest, bytes: &[u8]) -> PackageIdentity {
    PackageIdentity {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        manifest_sha256: sha256(bytes),
    }
}
fn progress(phase: ImportPhase, files_done: usize, files_total: usize) -> ImportProgress {
    ImportProgress {
        phase,
        files_done,
        files_total,
        bytes_done: 0,
        bytes_total: 0,
    }
}
fn validate_payloads(
    root: &Path,
    manifest: &Manifest,
    callback: &mut impl FnMut(ImportProgress) -> bool,
) -> Result<(Vec<u8>, GeometrySummary)> {
    let mut profile = None;
    let mut geometry = None;
    for (i, file) in manifest.files.iter().enumerate() {
        notify(
            callback,
            progress(ImportPhase::Validating, i, manifest.files.len()),
        )?;
        let path = root.join(&file.path);
        check_directory_path(
            path.parent()
                .ok_or_else(|| Error::Invalid("missing parent".into()))?,
        )?;
        let bytes = read_bounded_file(&path, manifest::file_byte_limit(&file.path)?)?;
        if bytes.len() as u64 != file.size_bytes || sha256(&bytes) != file.sha256 {
            return Err(Error::Invalid(format!(
                "aircraft size/SHA-256 mismatch: {}",
                file.path
            )));
        }
        match file.kind {
            FileKind::Profile => {
                profile = Some(bytes);
            }
            FileKind::Model => {
                geometry = Some(glb::validate(&bytes)?);
            }
            FileKind::Documentation => {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::Invalid("documentation must be UTF-8".into()))?;
                if text.trim().is_empty()
                    || text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                {
                    return Err(Error::Invalid(
                        "empty/control-containing documentation".into(),
                    ));
                }
            }
        }
    }
    notify(
        callback,
        progress(
            ImportPhase::Ready,
            manifest.files.len(),
            manifest.files.len(),
        ),
    )?;
    Ok((
        profile.ok_or_else(|| Error::Invalid("profile missing".into()))?,
        geometry.ok_or_else(|| Error::Invalid("model missing".into()))?,
    ))
}
fn count_versions(store: &Path) -> Result<usize> {
    let mut count = 0;
    let mut visited = 0;
    for id in std::fs::read_dir(store)? {
        visited += 1;
        if visited > 1024 {
            return Err(Error::Limit("aircraft store entries"));
        }
        let id = id?;
        let name = id.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| Error::Invalid("non-UTF8 store entry".into()))?;
        if name.starts_with('.') {
            continue;
        }
        crate::manifest::validate_id(name)?;
        check_directory_path(&id.path())?;
        for version in std::fs::read_dir(id.path())? {
            let version = version?;
            visited += 1;
            if visited > 1024 {
                return Err(Error::Limit("aircraft store entries"));
            }
            check_directory_path(&version.path())?;
            count += 1;
            if count >= 256 {
                return Ok(count);
            }
        }
    }
    Ok(count)
}

/// Validate an archive in private temporary storage without installing anything.
/// The semantic callback has the same authority contract as `validate_profile`.
pub fn validate_zip(
    zip: &Path,
    validate: impl FnOnce(&[u8], &Manifest, &GeometrySummary) -> Result<()>,
) -> Result<PackageIdentity> {
    let root = tempfile::tempdir()?;
    let staged = stage_zip_with_progress(zip, root.path(), |_| true)?.validate_profile(validate)?;
    Ok(staged.identity().clone())
}
