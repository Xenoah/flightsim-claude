//! Bounded, read-free hints for discovering the coarsest regional DEM coverage.
//!
//! A hint changes only the render request tree. It is not terrain data or a
//! promise that a later read succeeds. Payload validation and streaming budgets
//! remain authoritative; missing/corrupt tiles still use the ordinary fallback.

use crate::TileId;
use std::collections::BTreeSet;
use std::path::Path;

/// Maximum input records, including duplicates, accepted by a coverage index.
pub const MAX_COVERAGE_TILES: usize = 16_384;
/// Includes unrelated directory entries; a failed snapshot is never partial.
pub const MAX_COVERAGE_DIRECTORY_ENTRIES: usize = 32_768;

/// Strict ancestor paths to the coarsest declared primary tiles. Once a
/// primary ancestor is declared, finer descendants do not impose a LOD floor.
#[derive(Debug, Clone, Default)]
pub struct PrimaryCoverage {
    refinements: BTreeSet<TileId>,
}

impl PrimaryCoverage {
    /// Build once from a bounded snapshot, never by loading DEMs during selection.
    ///
    /// # Errors
    /// Too many records or an invalid public-field tile ID.
    pub fn from_tiles(tiles: impl IntoIterator<Item = TileId>) -> Result<Self, &'static str> {
        let mut primary = BTreeSet::new();
        for (count, id) in tiles.into_iter().enumerate() {
            if count >= MAX_COVERAGE_TILES {
                return Err("primary coverage tile limit");
            }
            if id.level > crate::tile::MAX_LEVEL
                || id.x >= TileId::columns(id.level)
                || id.y >= TileId::rows(id.level)
            {
                return Err("invalid primary coverage tile ID");
            }
            primary.insert(id);
        }
        let mut refinements = BTreeSet::new();
        for &id in &primary {
            let mut ancestor = id.parent();
            let mut path = Vec::new();
            let mut covered = false;
            while let Some(parent) = ancestor {
                if primary.contains(&parent) {
                    covered = true;
                    break;
                }
                path.push(parent);
                ancestor = parent.parent();
            }
            if !covered {
                refinements.extend(path);
            }
        }
        Ok(Self { refinements })
    }

    /// Bounded index lookup, with no file operations or payload reads.
    #[must_use]
    pub fn requires_refinement(&self, id: TileId) -> bool {
        self.refinements.contains(&id)
    }

    /// Snapshot canonical `level/x/y.fsdem` filenames, visiting at most three
    /// directory levels. Symlinks and unrelated names are not coverage hints.
    /// The file contents are deliberately not decoded or attested here.
    ///
    /// # Errors
    /// Directory I/O failure or either explicit snapshot limit is exceeded.
    pub fn from_directory(root: &Path) -> std::io::Result<Self> {
        Self::from_directory_bounded(root, MAX_COVERAGE_DIRECTORY_ENTRIES)
    }

    fn from_directory_bounded(root: &Path, entry_limit: usize) -> std::io::Result<Self> {
        let mut entries = 0;
        let mut tiles = Vec::new();
        let mut pending = vec![(root.to_owned(), String::new(), 0)];
        while let Some((directory, prefix, depth)) = pending.pop() {
            for entry in std::fs::read_dir(&directory)? {
                let entry = entry?;
                entries += 1;
                if entries > entry_limit {
                    return Err(std::io::Error::other(
                        "primary coverage directory entry limit",
                    ));
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                let kind = entry.file_type()?;
                let relative = format!("{prefix}{name}");
                if depth < 2 && kind.is_dir() && canonical_integer(name).is_some() {
                    pending.push((entry.path(), format!("{relative}/"), depth + 1));
                } else if depth == 2
                    && kind.is_file()
                    && let Some(id) = tile_id_from_relative_path(&relative)
                {
                    if tiles.len() >= MAX_COVERAGE_TILES {
                        return Err(std::io::Error::other("primary coverage tile limit"));
                    }
                    tiles.push(id);
                }
            }
        }
        Self::from_tiles(tiles).map_err(std::io::Error::other)
    }
}

fn canonical_integer(value: &str) -> Option<u32> {
    let number: u32 = value.parse().ok()?;
    (number.to_string() == value).then_some(number)
}

/// Parse only a canonical geographic tile path, without touching the filesystem.
#[must_use]
pub fn tile_id_from_relative_path(path: &str) -> Option<TileId> {
    let mut parts = path.split('/');
    let level = u8::try_from(canonical_integer(parts.next()?)?).ok()?;
    let x = canonical_integer(parts.next()?)?;
    let y = canonical_integer(parts.next()?.strip_suffix(".fsdem")?)?;
    if parts.next().is_some()
        || level > crate::tile::MAX_LEVEL
        || x >= TileId::columns(level)
        || y >= TileId::rows(level)
    {
        return None;
    }
    Some(TileId::new(level, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiskTileSource, TileSource};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn only_coarsest_available_paths_impose_a_floor() {
        let root = TileId::new(10, 1077, 244);
        let coverage =
            PrimaryCoverage::from_tiles(root.children().unwrap().into_iter().chain([root]))
                .unwrap();
        assert!(!coverage.requires_refinement(root));
        assert!(coverage.requires_refinement(root.parent().unwrap()));
        assert!(!coverage.requires_refinement(TileId::new(9, 0, 0)));
        let sparse = PrimaryCoverage::from_tiles([root.children().unwrap()[0]]).unwrap();
        assert!(sparse.requires_refinement(root));
    }

    #[test]
    fn input_limits_count_duplicates_and_invalid_ids_are_errors() {
        assert!(
            PrimaryCoverage::from_tiles(std::iter::repeat_n(
                TileId::roots()[0],
                MAX_COVERAGE_TILES + 1,
            ))
            .is_err()
        );
        assert!(
            PrimaryCoverage::from_tiles([TileId {
                level: 255,
                x: 0,
                y: 0
            }])
            .is_err()
        );
        assert!(
            PrimaryCoverage::from_tiles([TileId {
                level: 1,
                x: 4,
                y: 0
            }])
            .is_err()
        );
    }

    #[test]
    fn paths_are_canonical_and_cannot_escape_or_overflow() {
        assert_eq!(
            tile_id_from_relative_path("10/1077/244.fsdem"),
            Some(TileId::new(10, 1077, 244))
        );
        for bad in [
            "../1077/244.fsdem",
            "10/1077/244.fsdem/extra",
            "10/1077/0244.fsdem",
            "010/1077/244.fsdem",
            "25/0/0.fsdem",
            "0/2/0.fsdem",
            "0/0/1.fsdem",
            "10\\1077\\244.fsdem",
            "/10/1077/244.fsdem",
            "4294967296/0/0.fsdem",
        ] {
            assert_eq!(tile_id_from_relative_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn disk_snapshot_is_metadata_only_and_forwards_through_wrappers() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "flightsim-coverage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("10/1077")).unwrap();
        let id = TileId::new(10, 1077, 244);
        std::fs::write(root.join("10/1077/244.fsdem"), b"invalid payload").unwrap();
        std::fs::write(root.join("10/1077/0244.fsdem"), b"ignored alias").unwrap();
        assert!(PrimaryCoverage::from_directory_bounded(&root, 3).is_err());
        assert!(PrimaryCoverage::from_directory_bounded(&root, 4).is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("10"), root.join("12")).unwrap();
            std::os::unix::fs::symlink(
                root.join("10/1077/244.fsdem"),
                root.join("10/1077/800.fsdem"),
            )
            .unwrap();
        }
        let disk = DiskTileSource::new(&root).with_coverage_index().unwrap();
        assert!(
            !disk
                .primary_coverage()
                .unwrap()
                .requires_refinement(TileId::new(12, 1077, 244).parent().unwrap())
        );
        assert!(
            !disk
                .primary_coverage()
                .unwrap()
                .requires_refinement(TileId::new(10, 1077, 800).parent().unwrap())
        );
        assert!(disk.primary_reads_possible());
        let boxed: Box<dyn TileSource> = Box::new(crate::global::GlobalTileSource::new(
            disk,
            crate::global::GlobalTerrain::bundled().unwrap(),
        ));
        let reference = &boxed;
        assert!(
            reference
                .primary_coverage()
                .unwrap()
                .requires_refinement(id.parent().unwrap())
        );
        assert!(
            reference.load(id).is_err(),
            "a hint must not attest the payload"
        );
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            reference.load(id).unwrap().is_none(),
            "reads remain dynamic after snapshot"
        );
    }
}
