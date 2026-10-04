use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, MAX_FILES, MAX_MANIFEST_BYTES, MAX_TOTAL_BYTES, Result};

/// The v1 package has one regional DEM content kind. Unknown kinds fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    TerrainDem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    TerrainDem,
    Documentation,
}

/// Geographic input boundary; degrees are serialized explicitly, not runtime units.
/// A west > east interval crosses the antimeridian.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundsDegrees {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainMetadata {
    pub bounds_degrees: BoundsDegrees,
    /// Source's nominal ground resolution, not a guarantee of runtime LOD detail.
    pub nominal_resolution_m: f64,
    /// v1 accepts exactly EPSG:4979 (WGS84 ellipsoidal metres).
    pub datum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseRecord {
    pub name: String,
    pub url: String,
    pub text_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub id: String,
    pub url: String,
    /// Immutable source revision, release or dated dataset identifier.
    pub revision: String,
    /// Input datum, normalization and processing history, including assumptions.
    pub provenance: String,
    pub credits: String,
    pub license: LicenseRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub kind: FileKind,
    pub size_bytes: u64,
    /// Lowercase 64-digit hexadecimal SHA-256.
    pub sha256: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    /// Canonical three-part numeric version. Installing the same version never overwrites it.
    pub version: String,
    pub title: String,
    pub content_kinds: Vec<ContentKind>,
    pub terrain: TerrainMetadata,
    pub sources: Vec<SourceRecord>,
    pub files: Vec<FileRecord>,
}

/// Parse only after bounding the input; strict fields prevent silently ignored future features.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit("manifest bytes"));
    }
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    manifest.validate()?;
    Ok(manifest)
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::Invalid("unsupported manifest schema".into()));
        }
        validate_id(&self.id)?;
        let parts: Vec<_> = self.version.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|s| {
                s.is_empty()
                    || s.len() > 9
                    || !s.bytes().all(|c| c.is_ascii_digit())
                    || s.len() > 1 && s.starts_with('0')
            })
        {
            return Err(Error::Invalid(
                "version must be canonical MAJOR.MINOR.PATCH".into(),
            ));
        }
        bounded_text(&self.title, 160)?;
        if self.content_kinds != [ContentKind::TerrainDem] {
            return Err(Error::Invalid(
                "v1 requires exactly terrain_dem content".into(),
            ));
        }
        let b = &self.terrain.bounds_degrees;
        if !(-180.0..=180.0).contains(&b.west)
            || !(-180.0..=180.0).contains(&b.east)
            || !(-90.0..=90.0).contains(&b.south)
            || !(-90.0..=90.0).contains(&b.north)
            || b.south >= b.north
            || b.west.to_bits() == b.east.to_bits()
            || (b.west - b.east).abs() < f64::EPSILON
            || (b.west >= 180.0 && b.east <= -180.0)
        {
            return Err(Error::Invalid("invalid geographic bounds".into()));
        }
        let resolution = self.terrain.nominal_resolution_m;
        if !resolution.is_finite()
            || !(0.01..=100_000.0).contains(&resolution)
            || self.terrain.datum != "EPSG:4979"
        {
            return Err(Error::Invalid(
                "unsupported datum or nominal resolution".into(),
            ));
        }
        if self.sources.is_empty() || self.sources.len() > 64 {
            return Err(Error::Limit("source count"));
        }
        let mut sources = BTreeSet::new();
        for source in &self.sources {
            validate_id(&source.id)?;
            if !sources.insert(&source.id) {
                return Err(Error::Invalid("duplicate source ID".into()));
            }
            source_url(&source.url)?;
            source_url(&source.license.url)?;
            bounded_text(&source.revision, 256)?;
            bounded_text(&source.provenance, 8192)?;
            bounded_text(&source.credits, 4096)?;
            bounded_text(&source.license.name, 256)?;
        }
        if self.files.is_empty() || self.files.len() > MAX_FILES {
            return Err(Error::Limit("file count"));
        }
        let mut paths = PortablePaths::default();
        let mut total = 0u64;
        let mut has_dem = false;
        for file in &self.files {
            paths.add(&file.path, false)?;
            if file.path == crate::MANIFEST_NAME
                || file.size_bytes == 0
                || file.size_bytes > file_byte_limit(&file.path)?
            {
                return Err(Error::Invalid(format!(
                    "unsupported size/path: {}",
                    file.path
                )));
            }
            match file.kind {
                FileKind::TerrainDem => {
                    has_dem = true;
                    if !file.path.starts_with("terrain/") || !file.path.ends_with(".fsdem") {
                        return Err(Error::Invalid(
                            "DEM must use terrain/<level>/<x>/<y>.fsdem".into(),
                        ));
                    }
                }
                FileKind::Documentation => {
                    if !file.path.starts_with("docs/")
                        || !(file.path.ends_with(".txt") || file.path.ends_with(".md"))
                    {
                        return Err(Error::Invalid(
                            "documentation must be docs/*.txt or docs/*.md".into(),
                        ));
                    }
                }
            }
            if !sources.contains(&file.source)
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(Error::Invalid("unknown source or invalid SHA-256".into()));
            }
            total = total
                .checked_add(file.size_bytes)
                .ok_or(Error::Limit("total bytes"))?;
        }
        if !has_dem || total > MAX_TOTAL_BYTES {
            return Err(Error::Limit("DEM required / total bytes"));
        }
        for source in &self.sources {
            if !self
                .files
                .iter()
                .any(|f| f.path == source.license.text_path && f.kind == FileKind::Documentation)
            {
                return Err(Error::Invalid(
                    "license text must name a declared documentation file".into(),
                ));
            }
        }
        Ok(())
    }
}

fn bounded_text(value: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > limit
        || value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Invalid(
            "missing, oversized or control-containing metadata".into(),
        ));
    }
    Ok(())
}

fn source_url(value: &str) -> Result<()> {
    bounded_text(value, 2048)?;
    if !(value.starts_with("https://") || value.starts_with("http://"))
        || value.chars().any(char::is_whitespace)
    {
        return Err(Error::Invalid("source/license URL must be HTTP(S)".into()));
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    validate_path(id)?;
    if id.len() > 80
        || id.contains('/')
        || !id.starts_with(|c: char| c.is_ascii_lowercase())
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
    {
        return Err(Error::Invalid(
            "ID must be lowercase ASCII letters/digits/dot/hyphen".into(),
        ));
    }
    Ok(())
}

/// Deliberately narrower than host Path semantics so Linux validation is safe on Windows.
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 180 || path.split('/').count() > 8 {
        return Err(Error::Invalid("empty or oversized package path".into()));
    }
    for component in path.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            || !component
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || (stem.len() == 4
                && (stem.starts_with("com") || stem.starts_with("lpt"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(Error::Invalid(format!(
                "nonportable/unsafe package path: {path}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn file_byte_limit(path: &str) -> Result<u64> {
    if path == crate::MANIFEST_NAME {
        Ok(MAX_MANIFEST_BYTES as u64)
    } else if path.starts_with("terrain/") && path.ends_with(".fsdem") {
        Ok(crate::MAX_DEM_BYTES)
    } else if path.starts_with("docs/") && (path.ends_with(".txt") || path.ends_with(".md")) {
        Ok(crate::MAX_DOCUMENT_BYTES)
    } else {
        Err(Error::Invalid(format!("unsupported package file: {path}")))
    }
}

#[derive(Debug, Default)]
pub(crate) struct PortablePaths {
    // lowercased prefix -> (original case, is_directory, explicitly present)
    seen: BTreeMap<String, (String, bool, bool)>,
}

impl PortablePaths {
    pub fn add(&mut self, path: &str, directory: bool) -> Result<()> {
        validate_path(path)?;
        let components: Vec<_> = path.split('/').collect();
        for end in 1..=components.len() {
            let prefix = components[..end].join("/");
            let key = prefix.to_ascii_lowercase();
            let leaf = end == components.len();
            let is_directory = !leaf || directory;
            if let Some((original, was_directory, explicit)) = self.seen.get_mut(&key) {
                if *original != prefix || !*was_directory || !is_directory || leaf && *explicit {
                    return Err(Error::Invalid(format!("duplicate/colliding path: {path}")));
                }
                if leaf {
                    *explicit = true;
                }
            } else {
                self.seen.insert(key, (prefix, is_directory, leaf));
                if self.seen.len() > crate::MAX_ARCHIVE_ENTRIES {
                    return Err(Error::Limit("materialized file/directory count"));
                }
            }
        }
        Ok(())
    }
}
