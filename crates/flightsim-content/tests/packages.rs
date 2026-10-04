use std::{fs, io::Write, path::Path};

use flightsim_content::*;
use flightsim_core::Meters;
use flightsim_world::{
    dem::{HeightGrid, io::write_tile},
    terrain::TileSource,
    tile::TileId,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn tile_id() -> TileId {
    TileId::new(9, 900, 180)
}
fn tile_path() -> &'static str {
    "terrain/9/900/180.fsdem"
}
fn dem() -> Vec<u8> {
    let mut bytes = Vec::new();
    write_tile(
        &mut bytes,
        tile_id(),
        &HeightGrid::flat(3, 3, Meters(350.0)),
    )
    .unwrap();
    bytes
}
fn manifest(data: &[u8]) -> Value {
    json!({
        "schema_version":1,"id":"org.example.fixture","version":"1.0.0","title":"Synthetic terrain fixture",
        "content_kinds":["terrain_dem"],
        "terrain":{"bounds_degrees":{"west":-180.0,"south":-90.0,"east":180.0,"north":90.0},"nominal_resolution_m":90.0,"datum":"EPSG:4979"},
        "sources":[{"id":"synthetic","url":"https://example.test/terrain","revision":"fixture-v1","provenance":"Synthetic WGS84 ellipsoidal metres; no external inputs","credits":"Synthetic test data","license":{"name":"CC0-1.0","url":"https://creativecommons.org/publicdomain/zero/1.0/","text_path":"docs/LICENSE.txt"}}],
        "files":[
            {"path":tile_path(),"kind":"terrain_dem","size_bytes":data.len(),"sha256":digest(data),"source":"synthetic"},
            {"path":"docs/LICENSE.txt","kind":"documentation","size_bytes":12,"sha256":digest(b"CC0 fixture\n"),"source":"synthetic"}
        ]
    })
}
#[derive(Clone)]
struct Entry {
    name: String,
    data: Vec<u8>,
    method: u16,
    mode: u32,
    size: Option<u32>,
    flags: u16,
    compressed_payload: Option<Vec<u8>>,
}
impl Entry {
    fn new(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            data: data.to_vec(),
            method: 0,
            mode: 0o100644,
            size: None,
            flags: 0,
            compressed_payload: None,
        }
    }
}
fn entries_with(manifest: &Value, data: &[u8]) -> Vec<Entry> {
    vec![
        Entry::new(MANIFEST_NAME, &serde_json::to_vec(manifest).unwrap()),
        Entry::new(tile_path(), data),
        Entry::new("docs/LICENSE.txt", b"CC0 fixture\n"),
    ]
}
fn fixture() -> Vec<Entry> {
    let data = dem();
    entries_with(&manifest(&data), &data)
}
fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
fn zip(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32::try_from(out.len()).unwrap();
        let payload = if let Some(bytes) = &entry.compressed_payload {
            bytes.clone()
        } else if entry.method == 8 {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(&entry.data).unwrap();
            encoder.finish().unwrap()
        } else {
            entry.data.clone()
        };
        let crc = rawzip::crc32(&entry.data);
        let size = entry
            .size
            .unwrap_or(u32::try_from(entry.data.len()).unwrap());
        let compressed = u32::try_from(payload.len()).unwrap();
        u32le(&mut out, 0x04034b50);
        u16le(&mut out, 20);
        u16le(&mut out, entry.flags);
        u16le(&mut out, entry.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u32le(&mut out, crc);
        u32le(&mut out, compressed);
        u32le(&mut out, size);
        u16le(&mut out, u16::try_from(entry.name.len()).unwrap());
        u16le(&mut out, 0);
        out.extend(entry.name.as_bytes());
        out.extend(payload);
        u32le(&mut central, 0x02014b50);
        u16le(&mut central, 0x0314);
        u16le(&mut central, 20);
        u16le(&mut central, entry.flags);
        u16le(&mut central, entry.method);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, crc);
        u32le(&mut central, compressed);
        u32le(&mut central, size);
        u16le(&mut central, u16::try_from(entry.name.len()).unwrap());
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, entry.mode << 16);
        u32le(&mut central, offset);
        central.extend(entry.name.as_bytes());
    }
    let offset = u32::try_from(out.len()).unwrap();
    let len = u32::try_from(central.len()).unwrap();
    out.extend(central);
    u32le(&mut out, 0x06054b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, u16::try_from(entries.len()).unwrap());
    u16le(&mut out, u16::try_from(entries.len()).unwrap());
    u32le(&mut out, len);
    u32le(&mut out, offset);
    u16le(&mut out, 0);
    out
}
fn write_zip(root: &Path, entries: &[Entry]) -> std::path::PathBuf {
    let path = root.join("input.zip");
    fs::write(&path, zip(entries)).unwrap();
    path
}
fn rejected(entries: &[Entry]) {
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), entries);
    let store = tmp.path().join("packages");
    assert!(install_zip(&input, &store).is_err());
    assert!(list_installed(&store).unwrap().is_empty());
    if store.exists() {
        assert!(!fs::read_dir(store).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".import-")
        }));
    }
}

#[test]
fn stored_and_deflated_imports_preserve_bytes_identity_and_world_reader() {
    for compressed in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut entries = fixture();
        if compressed {
            for entry in &mut entries {
                entry.method = 8;
            }
        }
        let input = write_zip(tmp.path(), &entries);
        let store = tmp.path().join("packages");
        let installed = install_zip(&input, &store).unwrap();
        assert_eq!(
            installed.identity().manifest_sha256,
            digest(&entries[0].data)
        );
        assert_eq!(
            fs::read(installed.directory().join(tile_path())).unwrap(),
            dem()
        );
        assert_eq!(
            installed
                .tile_source()
                .load(tile_id())
                .unwrap()
                .unwrap()
                .elevation_at(tile_id().center()),
            Meters(350.0)
        );
        assert!(
            installed
                .tile_source()
                .load(TileId::new(9, 901, 180))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            installed.require_replay_support(),
            Err(Error::ReplayUnsupported)
        ));
        assert_eq!(list_installed(&store).unwrap().len(), 1);
        assert_eq!(
            inspect_installed(installed.directory()).unwrap().identity(),
            installed.identity()
        );
        assert!(matches!(
            install_zip(&input, &store),
            Err(Error::AlreadyInstalled(_))
        ));
        assert_eq!(
            fs::read(installed.directory().join(tile_path())).unwrap(),
            dem()
        );
    }
}

#[test]
fn cancelled_import_cleans_staging_and_never_changes_an_existing_version() {
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let existing = install_zip(&input, &store).unwrap();
    for phase in [
        ImportPhase::Inspecting,
        ImportPhase::Extracting,
        ImportPhase::Validating,
        ImportPhase::Ready,
    ] {
        let result = stage_zip_with_progress(&input, &store, |p| p.phase != phase);
        assert!(matches!(result, Err(Error::Cancelled)), "{phase:?}");
        assert_eq!(list_installed(&store).unwrap().len(), 1);
        assert_eq!(
            inspect_installed(existing.directory()).unwrap().identity(),
            existing.identity()
        );
        assert!(!fs::read_dir(&store).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".import-")
        }));
    }
}

#[test]
fn dropped_ready_stage_and_crash_leftover_never_reserve_the_version() {
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let stage = stage_zip_with_progress(&input, &store, |_| true).unwrap();
    drop(stage);
    fs::create_dir(store.join(".import-crashed-operation")).unwrap();
    fs::write(
        store.join(".import-crashed-operation/partial"),
        b"not installed",
    )
    .unwrap();
    assert!(list_installed(&store).unwrap().is_empty());
    install_zip(&input, &store).unwrap();
    assert!(store.join(".import-crashed-operation/partial").exists());
}

#[test]
fn commit_lock_is_nonblocking_and_concurrent_publication_never_replaces() {
    use fs2::FileExt;
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let stage = stage_zip_with_progress(&input, &store, |_| true).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(store.join(".install.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    assert!(matches!(stage.commit(), Err(Error::StoreBusy)));
    drop(lock);
    let a = stage_zip_with_progress(&input, &store, |_| true).unwrap();
    let b = stage_zip_with_progress(&input, &store, |_| true).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let other = barrier.clone();
    let thread = std::thread::spawn(move || {
        other.wait();
        a.commit()
    });
    barrier.wait();
    let b = b.commit();
    let a = thread.join().unwrap();
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(list_installed(&store).unwrap().len(), 1);
}

#[test]
fn hostile_windows_and_posix_paths_are_rejected_without_extraction() {
    for path in [
        "../outside",
        "/absolute",
        "C:/escape",
        "C:relative",
        "\\\\server\\share",
        "terrain\\evil.fsdem",
        "docs/file.txt:stream",
        "docs/../file.txt",
        "docs//file.txt",
        "docs/./file.txt",
        "docs/CON.txt",
        "docs/lpt9.txt",
        "docs/aux",
        "docs/trailing.",
        "docs/trailing ",
        "docs/é.txt",
        "docs/\0.txt",
    ] {
        let mut entries = fixture();
        entries.push(Entry::new(path, b"attack"));
        rejected(&entries);
    }
}

#[test]
fn duplicates_case_collisions_file_directory_collisions_and_links_fail() {
    let valid = fixture();
    for extra in [
        valid[2].clone(),
        Entry::new("docs/license.txt", b"case"),
        Entry::new("DOCS/other.txt", b"directory case"),
        Entry::new("docs", b"file directory collision"),
    ] {
        let mut entries = valid.clone();
        entries.push(extra);
        rejected(&entries);
    }
    for mode in [0o120777, 0o010644, 0o020644, 0o060644, 0o140644] {
        let mut entries = fixture();
        entries[2].mode = mode;
        rejected(&entries);
    }
}

#[test]
fn unsupported_content_encryption_and_raw_repository_archives_fail_closed() {
    for name in [
        "run.exe",
        "script.js",
        "nested.zip",
        "terrain/input.tif",
        "region.fsairports",
        "region.fsscenery",
        "repo-main/manifest.json",
    ] {
        let mut entries = fixture();
        entries.push(Entry::new(name, b"anything"));
        rejected(&entries);
    }
    for flags in [1, 0x40, 0x2000] {
        let mut entries = fixture();
        entries[1].flags = flags;
        rejected(&entries);
    }
    let mut entries = fixture();
    entries[1].method = 93;
    rejected(&entries);
    rejected(&[Entry::new("region.tif", b"raw")]);
}

#[test]
fn archive_counts_declared_and_actual_sizes_ratios_and_checksums_are_bounded() {
    let mut entries = fixture();
    entries[1].size = Some(u32::try_from(MAX_DEM_BYTES + 1).unwrap());
    rejected(&entries);
    let mut entries = fixture();
    entries[1].size = Some(1);
    rejected(&entries);
    let mut entries = fixture();
    entries[1].data[60] ^= 1;
    rejected(&entries);
    let mut entries = fixture();
    entries[0].data = b" ".repeat(MAX_MANIFEST_BYTES + 1);
    rejected(&entries);
    let mut entries = fixture();
    entries[1].method = 8;
    entries[1].data = vec![0; 2 * 1024 * 1024];
    rejected(&entries);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("input.zip");
    let mut bytes = zip(&fixture());
    let end = bytes.len() - 22;
    bytes[end + 8..end + 12].copy_from_slice(&[255, 255, 255, 255]);
    fs::write(&path, bytes).unwrap();
    assert!(install_zip(&path, &tmp.path().join("packages")).is_err());
    let oversized = fs::File::create(&path).unwrap();
    oversized.set_len(MAX_ARCHIVE_BYTES + 1).unwrap();
    drop(oversized);
    assert!(matches!(
        install_zip(&path, &tmp.path().join("packages")),
        Err(Error::Limit(_))
    ));
}

#[test]
fn zip_crc_local_header_mismatch_and_truncation_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("input.zip");
    let store = tmp.path().join("packages");
    let original = zip(&fixture());
    for mutation in [0, 1, 2] {
        let mut bytes = original.clone();
        match mutation {
            0 => {
                bytes[30 + MANIFEST_NAME.len() + 5] ^= 1;
            }
            1 => {
                bytes[30] = b'x';
            }
            _ => {
                bytes.truncate(bytes.len() - 12);
            }
        }
        fs::write(&path, bytes).unwrap();
        assert!(install_zip(&path, &store).is_err());
    }
}

#[test]
fn manifest_unknown_fields_kinds_schema_and_ambiguous_identity_are_rejected() {
    let data = dem();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("content_kinds", json!(["terrain_dem", "airports"])),
        ("id", json!("CON")),
        ("id", json!("../escape")),
        ("version", json!("01.0.0")),
        ("version", json!("1.0.0/other")),
        ("title", json!("")),
        ("execute", json!("script")),
    ] {
        let mut m = manifest(&data);
        m[field] = value;
        rejected(&entries_with(&m, &data));
    }
    let mut m = manifest(&data);
    m["terrain"]["datum"] = json!("EGM2008");
    rejected(&entries_with(&m, &data));
    let mut m = manifest(&data);
    m["sources"][0]["license"]["text_path"] = json!("missing.txt");
    rejected(&entries_with(&m, &data));
    let mut m = manifest(&data);
    m["files"][0]["sha256"] = json!("0".repeat(64));
    rejected(&entries_with(&m, &data));
    let mut m = manifest(&data);
    m["files"][0]["size_bytes"] = json!(data.len() + 1);
    rejected(&entries_with(&m, &data));
}

#[test]
fn runtime_dem_dimensions_identity_trailing_bytes_and_coverage_are_verified() {
    for case in 0..5 {
        let mut data = dem();
        match case {
            0 => data[16..20].copy_from_slice(&4096u32.to_le_bytes()),
            1 => data[8..12].copy_from_slice(&901u32.to_le_bytes()),
            2 => data.push(0),
            3 => data[24..32].copy_from_slice(&f64::MAX.to_le_bytes()),
            _ => {}
        }
        let mut m = manifest(&data);
        if case == 4 {
            m["terrain"]["bounds_degrees"] =
                json!({"west":0.0,"south":-1.0,"east":1.0,"north":1.0});
        }
        rejected(&entries_with(&m, &data));
    }
}

#[test]
fn installed_tampering_is_detected_on_inspection_and_runtime_load() {
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let installed = install_zip(&input, &store).unwrap();
    fs::write(installed.directory().join(tile_path()), b"changed").unwrap();
    assert!(inspect_installed(installed.directory()).is_err());
    assert!(installed.tile_source().load(tile_id()).is_err());
    fs::write(installed.directory().join(tile_path()), dem()).unwrap();
    fs::write(installed.directory().join("surprise.dll"), b"never execute").unwrap();
    assert!(inspect_installed(installed.directory()).is_err());
}

#[cfg(unix)]
#[test]
fn installed_and_store_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let installed = install_zip(&input, &store).unwrap();
    let original = installed.directory().join(tile_path());
    fs::remove_file(&original).unwrap();
    let external = tmp.path().join("other.fsdem");
    fs::write(&external, dem()).unwrap();
    symlink(&external, &original).unwrap();
    assert!(inspect_installed(installed.directory()).is_err());
    assert!(installed.tile_source().load(tile_id()).is_err());
    let alias = tmp.path().join("alias");
    symlink(&store, &alias).unwrap();
    assert!(install_zip(&input, &alias).is_err());
}

#[test]
fn implicit_directory_count_and_cumulative_declared_bytes_are_bounded() {
    let data = dem();
    let mut m = manifest(&data);
    for index in 0..1200 {
        m["files"].as_array_mut().unwrap().push(json!({"path":format!("docs/d{index}/a/b/c/d/e/file.txt"),"kind":"documentation","size_bytes":1,"sha256":digest(b"x"),"source":"synthetic"}));
    }
    assert!(matches!(
        parse_manifest(&serde_json::to_vec(&m).unwrap()),
        Err(Error::Limit("materialized file/directory count"))
    ));
    let mut m = manifest(&data);
    for index in 0..300 {
        m["files"].as_array_mut().unwrap().push(json!({"path":format!("terrain/9/{index}/180.fsdem"),"kind":"terrain_dem","size_bytes":MAX_DEM_BYTES,"sha256":digest(&data),"source":"synthetic"}));
    }
    assert!(matches!(
        parse_manifest(&serde_json::to_vec(&m).unwrap()),
        Err(Error::Limit("DEM required / total bytes"))
    ));
}

#[test]
fn strict_zip32_guard_rejects_prefixed_zip64_offset_without_parser_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hostile.zip");
    let mut bytes = zip(&fixture());
    let end = bytes.len() - 22;
    let central = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    bytes[central + 42..central + 46].copy_from_slice(&u32::MAX.to_le_bytes());
    let name_len =
        u16::from_le_bytes(bytes[central + 28..central + 30].try_into().unwrap()) as usize;
    let extra_at = central + 46 + name_len;
    bytes[central + 30..central + 32].copy_from_slice(&12u16.to_le_bytes());
    let mut extra = vec![1, 0, 8, 0];
    extra.extend(u64::MAX.to_le_bytes());
    bytes.splice(extra_at..extra_at, extra);
    let new_end = bytes.len() - 22;
    let old_size = u32::from_le_bytes(bytes[new_end + 12..new_end + 16].try_into().unwrap());
    bytes[new_end + 12..new_end + 16].copy_from_slice(&(old_size + 12).to_le_bytes());
    bytes.insert(0, 0);
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        install_zip(&path, &tmp.path().join("packages")),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn store_capacity_is_enforced_before_commit_without_hiding_existing_versions() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("packages");
    for index in 0..256 {
        let version = format!("0.0.{index}");
        let dir = store.join("org.example.fixture").join(&version);
        fs::create_dir_all(&dir).unwrap();
        let mut m = manifest(&dem());
        m["version"] = json!(version);
        fs::write(dir.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
    }
    let data = dem();
    let mut candidate = manifest(&data);
    candidate["id"] = json!("org.example.another");
    let input = write_zip(tmp.path(), &entries_with(&candidate, &data));
    assert!(matches!(
        install_zip(&input, &store),
        Err(Error::Limit("installed version count"))
    ));
    assert_eq!(list_installed(&store).unwrap().len(), 256);
    assert!(!store.join("org.example.another").exists());
}

#[test]
fn antimeridian_bounds_and_global_fallback_compose_without_changing_primary() {
    use flightsim_world::global::{GlobalTerrain, GlobalTileSource};
    let tmp = tempfile::tempdir().unwrap();
    let data = dem();
    let mut m = manifest(&data);
    m["terrain"]["bounds_degrees"] = json!({"west":130.0,"south":-90.0,"east":-130.0,"north":90.0});
    let input = write_zip(tmp.path(), &entries_with(&m, &data));
    let installed = install_zip(&input, &tmp.path().join("packages")).unwrap();
    let source = GlobalTileSource::new(installed.tile_source(), GlobalTerrain::bundled().unwrap());
    assert_eq!(
        source
            .load(tile_id())
            .unwrap()
            .unwrap()
            .elevation_at(tile_id().center()),
        Meters(350.0)
    );
    let outside = TileId::new(9, 100, 180);
    assert!(source.load(outside).unwrap().is_none());
    assert!(source.load_fallback(outside).unwrap().is_some());
    assert!(
        source
            .fallback_elevation_at(outside.center())
            .unwrap()
            .get()
            .is_finite()
    );
}

#[test]
fn compression_ratio_is_rejected_before_decoding_a_forged_central_size() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ratio.zip");
    let mut bytes = zip(&fixture());
    let end = bytes.len() - 22;
    let central = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    bytes[central + 20..central + 24].copy_from_slice(&1u32.to_le_bytes());
    bytes[central + 24..central + 28].copy_from_slice(&2048u32.to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        install_zip(&path, &tmp.path().join("packages")),
        Err(Error::Limit("compression ratio"))
    ));
}

#[test]
fn original_zip_changes_after_snapshot_do_not_change_installed_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = fixture();
    let input = write_zip(tmp.path(), &entries);
    let store = tmp.path().join("packages");
    let mut replaced = false;
    let stage = stage_zip_with_progress(&input, &store, |progress| {
        if progress.phase == ImportPhase::Extracting && !replaced {
            fs::write(&input, b"replaced after validation").unwrap();
            replaced = true;
        }
        true
    })
    .unwrap();
    assert!(replaced);
    let installed = stage.commit().unwrap();
    assert_eq!(
        installed.identity().manifest_sha256,
        digest(&entries[0].data)
    );
    assert!(inspect_installed(installed.directory()).is_ok());
}

#[test]
fn valid_hash_and_zip_crc_do_not_authorize_unsafe_dem_geometric_error() {
    for error in [f64::MAX, f64::from(f32::MAX) * 2.0, 1.0] {
        let mut data = dem();
        data[40..48].copy_from_slice(&error.to_le_bytes());
        // Hash and ZIP CRC are regenerated from the altered bytes. The original
        // FSDM payload checksum remains valid because only its header changed.
        let tmp = tempfile::tempdir().unwrap();
        let input = write_zip(tmp.path(), &entries_with(&manifest(&data), &data));
        let result = install_zip(&input, &tmp.path().join("packages"));
        assert!(matches!(result, Err(Error::Invalid(ref message))
            if message == "DEM geometric error exceeds decoded elevation range"));
    }
}

#[test]
fn writer_generated_rough_dem_keeps_finite_default_skirt_geometry() {
    use flightsim_world::mesh::{MeshOptions, build_mesh};
    for dimension in [2u32, 3, 4, 17, 65] {
        let samples = (0..dimension * dimension)
            .map(|i| if i % 3 == 0 { -12_000.0 } else { 100_000.0 })
            .collect();
        let mut data = Vec::new();
        write_tile(
            &mut data,
            tile_id(),
            &HeightGrid::new(dimension, dimension, samples),
        )
        .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let input = write_zip(tmp.path(), &entries_with(&manifest(&data), &data));
        let installed = install_zip(&input, &tmp.path().join("packages")).unwrap();
        let tile = installed.tile_source().load(tile_id()).unwrap().unwrap();
        let mesh = build_mesh(tile_id(), &tile, &MeshOptions::default());
        assert!(mesh.positions.len() > mesh.surface_vertex_count);
        assert!(
            mesh.positions
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert!(mesh.normals.iter().flatten().all(|value| value.is_finite()));
        assert!(mesh.slopes.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn cancellation_interrupts_deflate_empty_block_reads_before_any_file_output() {
    let tmp = tempfile::tempdir().unwrap();
    let mut entries = fixture();
    // Valid raw Deflate: many empty, nonfinal stored blocks, then the license
    // text in one final block. CRC, sizes and manifest hash all remain valid.
    let mut compressed = [0u8, 0, 0, 255, 255].repeat(32_000);
    let length = u16::try_from(entries[2].data.len()).unwrap();
    compressed.push(1);
    compressed.extend(length.to_le_bytes());
    compressed.extend((!length).to_le_bytes());
    compressed.extend(&entries[2].data);
    entries[2].method = 8;
    entries[2].compressed_payload = Some(compressed);
    let input = write_zip(tmp.path(), &entries);
    // First prove this is a valid input rather than an early-format-error test.
    install_zip(&input, &tmp.path().join("control-store")).unwrap();
    let store = tmp.path().join("cancelled-store");
    let before_document = (entries[0].data.len() + entries[1].data.len()) as u64;
    let mut zero_output_checks = 0;
    let result = stage_zip_with_progress(&input, &store, |progress| {
        if progress.phase == ImportPhase::Extracting
            && progress.files_done == 2
            && progress.bytes_done == before_document
        {
            zero_output_checks += 1;
            return zero_output_checks < 4;
        }
        true
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(zero_output_checks, 4);
    assert!(list_installed(&store).unwrap().is_empty());
    assert!(fs::read_dir(store).unwrap().next().is_none());
}

#[test]
fn installed_inspection_cancels_at_tree_payload_and_ready_without_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let input = write_zip(tmp.path(), &fixture());
    let store = tmp.path().join("packages");
    let installed = install_zip(&input, &store).unwrap();
    let before = installed.identity().clone();
    for phase in [
        ImportPhase::Inspecting,
        ImportPhase::Validating,
        ImportPhase::Ready,
    ] {
        let result = inspect_installed_with_progress(installed.directory(), |progress| {
            progress.phase != phase
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(
            inspect_installed(installed.directory()).unwrap().identity(),
            &before
        );
    }
    let mut checkpoints = 0;
    let result = inspect_installed_with_progress(installed.directory(), |_| {
        checkpoints += 1;
        checkpoints < 4
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(checkpoints, 4);
    assert_eq!(list_installed(&store).unwrap().len(), 1);
}
