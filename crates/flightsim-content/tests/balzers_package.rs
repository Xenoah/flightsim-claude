//! Exercise the shipped real-terrain sample through the production Rust reader.
//! These are content-integrity/lifecycle checks, not surveyed terrain acceptance.
use std::{collections::BTreeSet, fs, path::PathBuf};

use flightsim_content::{Error, FileKind, ImportPhase, install_zip, stage_zip_with_progress};
use flightsim_world::{terrain::TileSource, tile::TileId};
use sha2::{Digest, Sha256};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/examples/terrain-packages/balzers/Balzers_Terrain_Package_v1.zip")
}

fn expected_tiles() -> BTreeSet<TileId> {
    let mut all = BTreeSet::new();
    let mut level = vec![TileId::new(10, 1077, 244), TileId::new(10, 1078, 244)];
    loop {
        all.extend(level.iter().copied());
        if level[0].level == 13 {
            return all;
        }
        level = level
            .into_iter()
            .flat_map(|id| id.children().unwrap())
            .collect();
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn real_sample_preserves_all_170_tiles_and_original_source_notices() {
    let archive = fs::read(sample()).unwrap();
    assert_eq!(archive.len(), 1_610_134);
    assert_eq!(
        digest(&archive),
        "d7e1265ee8015ad0fb1df23f7110b23124b90d889440f1b14a9cd85119f28528"
    );
    let temp = tempfile::tempdir().unwrap();
    let installed = install_zip(&sample(), temp.path()).unwrap();
    assert_eq!(installed.identity().id, "balzers-glo90-rebuilt");
    assert_eq!(installed.identity().version, "1.0.0");
    assert_eq!(
        installed.identity().manifest_sha256,
        "683db832fe789e72aa76e0afd7869112252aedbcd7d2c98c5937d785cb66cc74"
    );
    let reinspected = flightsim_content::inspect_installed(installed.directory()).unwrap();
    assert_eq!(installed.identity(), reinspected.identity());
    let manifest = installed.manifest();
    assert_eq!(manifest.terrain.datum, "EPSG:4979");
    assert_eq!(
        manifest.terrain.nominal_resolution_m.to_bits(),
        90.0_f64.to_bits()
    );
    let bounds = &manifest.terrain.bounds_degrees;
    for (actual, expected) in [
        (bounds.west, 9.31640625_f64),
        (bounds.south, 46.93359375_f64),
        (bounds.east, 9.66796875_f64),
        (bounds.north, 47.109375_f64),
    ] {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
    let tiles: BTreeSet<_> = manifest
        .files
        .iter()
        .filter(|f| f.kind == FileKind::TerrainDem)
        .map(|file| file.path.clone())
        .collect();
    let expected = expected_tiles();
    assert_eq!(expected.len(), 170);
    assert_eq!(
        tiles,
        expected
            .iter()
            .map(|id| format!("terrain/{}/{}/{}.fsdem", id.level, id.x, id.y))
            .collect()
    );
    for file in &manifest.files {
        let bytes = fs::read(installed.directory().join(&file.path)).unwrap();
        assert_eq!(bytes.len() as u64, file.size_bytes);
        assert_eq!(digest(&bytes), file.sha256);
    }
    for (path, hash) in [
        (
            "docs/notice.txt",
            "3c6ea356f260ba6243f275a2064d1f12132951eb64e1b0a8f080b412cfeb82a1",
        ),
        (
            "docs/copernicus-license-bundle.txt",
            "2fb23c33ab080faa0dc21c575c327005626b6a07af1d597994fb566129d6ab26",
        ),
        (
            "docs/source-provenance.md",
            "67ee3f27c2c5caa18a3b09f8f5d41c20089a60344104addba95ec7bd739cd19c",
        ),
    ] {
        assert_eq!(
            digest(&fs::read(installed.directory().join(path)).unwrap()),
            hash
        );
    }
    let source = installed.tile_source();
    for id in expected {
        let tile = source.load(id).unwrap().unwrap();
        assert!(tile.elevation_at(id.center()).0.is_finite());
    }
    assert!(source.load(TileId::new(10, 1079, 244)).unwrap().is_none());
    assert!(matches!(
        installed.require_replay_support(),
        Err(Error::ReplayUnsupported)
    ));
}

#[test]
fn duplicate_install_and_later_payload_corruption_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let installed = install_zip(&sample(), temp.path()).unwrap();
    assert!(matches!(
        install_zip(&sample(), temp.path()),
        Err(Error::AlreadyInstalled(_))
    ));
    let tile = TileId::new(10, 1077, 244);
    let path = installed.directory().join("terrain/10/1077/244.fsdem");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(installed.tile_source().load(tile).is_err());
    assert!(flightsim_content::inspect_installed(installed.directory()).is_err());
}

#[test]
fn dropped_and_cancelled_real_packages_never_install() {
    for cancel_at in [
        ImportPhase::Inspecting,
        ImportPhase::Extracting,
        ImportPhase::Validating,
        ImportPhase::Ready,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let result = stage_zip_with_progress(&sample(), temp.path(), |p| p.phase != cancel_at);
        assert!(matches!(result, Err(Error::Cancelled)), "{cancel_at:?}");
        assert!(
            flightsim_content::list_installed(temp.path())
                .unwrap()
                .is_empty()
        );
        assert!(fs::read_dir(temp.path()).unwrap().next().is_none());
    }
    let temp = tempfile::tempdir().unwrap();
    let staged = stage_zip_with_progress(&sample(), temp.path(), |_| true).unwrap();
    drop(staged);
    assert!(
        flightsim_content::list_installed(temp.path())
            .unwrap()
            .is_empty()
    );
    assert!(fs::read_dir(temp.path()).unwrap().next().is_none());
}
