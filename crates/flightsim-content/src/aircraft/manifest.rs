use crate::manifest::{PortablePaths, bounded_text, source_url, validate_id};
use crate::{Error, Result, SourceRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Profile,
    Model,
    Documentation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub kind: FileKind,
    pub size_bytes: u64,
    pub sha256: String,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRecord {
    pub path: String,
    pub version: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRecord {
    pub path: String,
    pub format: String,
    pub scene: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u16,
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    pub profile: ProfileRecord,
    pub model: ModelRecord,
    pub sources: Vec<SourceRecord>,
    pub files: Vec<FileRecord>,
}
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > super::MAX_MANIFEST_BYTES {
        return Err(Error::Limit("aircraft manifest bytes"));
    }
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    manifest.validate()?;
    Ok(manifest)
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.kind != "aircraft" {
            return Err(Error::Invalid(
                "unsupported aircraft package kind/schema".into(),
            ));
        }
        validate_id(&self.id)?;
        bounded_text(&self.title, 160)?;
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
                "aircraft version must be canonical MAJOR.MINOR.PATCH".into(),
            ));
        }
        if self.profile.path != "profile.json"
            || !(1..=4).contains(&self.profile.version)
            || self.model.format != "static-untextured-glb-v1"
            || self.model.scene != 0
        {
            return Err(Error::Invalid("unsupported profile/model contract".into()));
        }
        file_byte_limit(&self.model.path)?;
        if !self.model.path.starts_with("assets/") || !self.model.path.ends_with(".glb") {
            return Err(Error::Invalid("model must be assets/<path>.glb".into()));
        }
        if self.sources.is_empty()
            || self.sources.len() > 8
            || self.files.len() < 3
            || self.files.len() > super::MAX_FILES
        {
            return Err(Error::Limit("aircraft sources/files"));
        }
        let mut sources = BTreeSet::new();
        for s in &self.sources {
            validate_id(&s.id)?;
            if !sources.insert(&s.id) {
                return Err(Error::Invalid("duplicate aircraft source".into()));
            }
            source_url(&s.url)?;
            source_url(&s.license.url)?;
            bounded_text(&s.revision, 256)?;
            bounded_text(&s.provenance, 8192)?;
            bounded_text(&s.credits, 4096)?;
            bounded_text(&s.license.name, 256)?;
        }
        let mut paths = PortablePaths::with_limit(super::MAX_ENTRIES);
        paths.add(crate::MANIFEST_NAME, false)?;
        let mut total = 0u64;
        let mut profiles = 0;
        let mut models = 0;
        for f in &self.files {
            paths.add(&f.path, false)?;
            let limit = if f.kind == FileKind::Profile && self.profile.version == 1 {
                128 * 1024
            } else {
                file_byte_limit(&f.path)?
            };
            if f.size_bytes == 0 || f.size_bytes > limit || f.path == crate::MANIFEST_NAME {
                return Err(Error::Limit("aircraft member bytes"));
            }
            match f.kind {
                FileKind::Profile => {
                    profiles += 1;
                    if f.path != self.profile.path {
                        return Err(Error::Invalid("profile binding mismatch".into()));
                    }
                }
                FileKind::Model => {
                    models += 1;
                    if f.path != self.model.path {
                        return Err(Error::Invalid("model binding mismatch".into()));
                    }
                }
                FileKind::Documentation => {
                    if !f.path.starts_with("docs/")
                        || !(f.path.ends_with(".txt") || f.path.ends_with(".md"))
                    {
                        return Err(Error::Invalid("unsupported documentation path".into()));
                    }
                }
            }
            if !sources.contains(&f.source)
                || f.sha256.len() != 64
                || !f
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(Error::Invalid("invalid aircraft source/hash".into()));
            }
            total = total
                .checked_add(f.size_bytes)
                .ok_or(Error::Limit("aircraft total bytes"))?;
        }
        if profiles != 1 || models != 1 || total > super::MAX_TOTAL_BYTES {
            return Err(Error::Invalid(
                "one profile and one model required within total budget".into(),
            ));
        }
        for s in &self.sources {
            if !self
                .files
                .iter()
                .any(|f| f.kind == FileKind::Documentation && f.path == s.license.text_path)
            {
                return Err(Error::Invalid(
                    "license text must be declared documentation".into(),
                ));
            }
        }
        Ok(())
    }
}
pub(super) fn file_byte_limit(path: &str) -> Result<u64> {
    if path == crate::MANIFEST_NAME {
        Ok(super::MAX_MANIFEST_BYTES as u64)
    } else if path == "profile.json" {
        Ok(super::MAX_PROFILE_BYTES)
    } else if path.starts_with("assets/") && path.ends_with(".glb") {
        Ok(super::MAX_GLB_BYTES)
    } else if path.starts_with("docs/") && (path.ends_with(".txt") || path.ends_with(".md")) {
        Ok(super::MAX_DOCUMENT_BYTES)
    } else {
        Err(Error::Invalid(format!(
            "unsupported aircraft member: {path}"
        )))
    }
}
