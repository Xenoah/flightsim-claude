use flightsim_content::{Error, ImportPhase, aircraft};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[path = "support/aircraft_zip.rs"]
mod zip;
use zip::Entry;
const MANIFEST: &[u8] =
    include_bytes!("../../../docs/examples/aircraft-packages/swift/manifest.json");
const PROFILE: &[u8] = include_bytes!("../../../assets/aircraft/swift_sport.json");
fn fixture() -> Vec<Entry> {
    vec![
        Entry::new("manifest.json", MANIFEST),
        Entry::new("profile.json", PROFILE),
        Entry::new(
            "assets/aircraft/swift_sport.glb",
            include_bytes!("../../../assets/aircraft/swift_sport.glb"),
        ),
        Entry::new(
            "docs/LICENSE-MIT.txt",
            include_bytes!("../../../docs/examples/aircraft-packages/swift/LICENSE-MIT.txt"),
        ),
        Entry::new(
            "docs/LICENSE-APACHE.txt",
            include_bytes!("../../../docs/examples/aircraft-packages/swift/LICENSE-APACHE.txt"),
        ),
        Entry::new(
            "docs/PROVENANCE.md",
            include_bytes!("../../../docs/examples/aircraft-packages/swift/PROVENANCE.md"),
        ),
    ]
}
fn stage(
    entries: &[Entry],
    root: &Path,
) -> flightsim_content::Result<aircraft::StagedAircraftPackage> {
    let input = root.join("input.zip");
    fs::write(&input, zip::zip(entries)).unwrap();
    aircraft::stage_zip_with_progress(&input, &root.join("store"), |_| true)
}
// Pure content tests do not replace app semantics. This callback only verifies
// the known original fixture, whose actual app validation is in app tests.
fn known_original(
    bytes: &[u8],
    manifest: &aircraft::Manifest,
    geometry: &aircraft::GeometrySummary,
) -> flightsim_content::Result<()> {
    assert_eq!(bytes, PROFILE);
    assert_eq!(manifest.profile.version, 1);
    assert_eq!(geometry.nodes, 31);
    Ok(())
}
fn rejected(entries: &[Entry]) {
    let root = tempfile::tempdir().unwrap();
    assert!(stage(entries, root.path()).is_err());
    assert_eq!(fs::read_dir(root.path().join("store")).unwrap().count(), 0);
}
fn edit_manifest(entries: &mut [Entry], edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&entries[0].data).unwrap();
    edit(&mut value);
    entries[0].data = serde_json::to_vec(&value).unwrap();
}
#[test]
fn original_stored_deflated_and_published_fixture_preserve_exact_bytes() {
    for compressed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut entries = fixture();
        if compressed {
            for e in &mut entries {
                e.method = 8;
            }
        }
        let staged = stage(&entries, root.path()).unwrap();
        assert_eq!(staged.profile_bytes(), PROFILE);
        assert!((staged.geometry().extents[0].get() - 7.12).abs() < 1e-5);
        let identity = staged.identity().clone();
        let installed = staged
            .validate_profile(known_original)
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(fs::read(installed.profile_path()).unwrap(), PROFILE);
        let checked = aircraft::inspect_installed(installed.directory()).unwrap();
        assert_eq!(checked.identity(), &identity);
        assert!(matches!(
            stage(&entries, root.path())
                .unwrap()
                .validate_profile(known_original)
                .unwrap()
                .commit(),
            Err(Error::AlreadyInstalled(_))
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("published.zip");
    fs::write(
        &path,
        include_bytes!("../../../docs/examples/aircraft-packages/swift-sport-1.0.0.zip"),
    )
    .unwrap();
    aircraft::validate_zip(&path, known_original).unwrap();
}
#[test]
fn semantic_rejection_and_dropped_stage_never_publish() {
    let root = tempfile::tempdir().unwrap();
    let staged = stage(&fixture(), root.path()).unwrap();
    assert!(
        staged
            .validate_profile(|_, _, _| Err(Error::Invalid(
                "authoritative profile rejected".into()
            )))
            .is_err()
    );
    assert_eq!(fs::read_dir(root.path().join("store")).unwrap().count(), 0);
    drop(
        stage(&fixture(), root.path())
            .unwrap()
            .validate_profile(known_original)
            .unwrap(),
    );
    assert_eq!(fs::read_dir(root.path().join("store")).unwrap().count(), 0);
}
#[test]
fn cancellation_at_each_phase_and_inspection_leaves_installed_bytes() {
    let root = tempfile::tempdir().unwrap();
    let installed = stage(&fixture(), root.path())
        .unwrap()
        .validate_profile(known_original)
        .unwrap()
        .commit()
        .unwrap();
    for phase in [
        ImportPhase::Inspecting,
        ImportPhase::Extracting,
        ImportPhase::Validating,
        ImportPhase::Ready,
    ] {
        let result = aircraft::stage_zip_with_progress(
            &root.path().join("input.zip"),
            &root.path().join("store"),
            |p| p.phase != phase,
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(fs::read(installed.profile_path()).unwrap(), PROFILE);
    }
    for phase in [
        ImportPhase::Inspecting,
        ImportPhase::Validating,
        ImportPhase::Ready,
    ] {
        assert!(matches!(
            aircraft::inspect_installed_with_progress(installed.directory(), |p| p.phase != phase),
            Err(Error::Cancelled)
        ));
    }
}
#[test]
fn manifests_paths_file_sets_and_terrain_are_separate() {
    for edit in [
        ("schema_version", Value::from(2)),
        ("kind", Value::from("terrain_dem")),
        ("unknown", Value::from(1)),
        ("version", Value::from("01.0.0")),
    ] {
        let mut entries = fixture();
        edit_manifest(&mut entries, |m| m[edit.0] = edit.1);
        rejected(&entries);
    }
    for name in [
        "../escape",
        "assets/../evil.glb",
        "assets/C:evil.glb",
        "assets/CON.glb",
        "assets/a\\b.glb",
        "assets/.hidden.glb",
    ] {
        let mut entries = fixture();
        entries.push(Entry::new(name, b"bad"));
        rejected(&entries);
    }
    for name in ["profile.json", "PROFILE.json", "docs/extra.txt", "docs/"] {
        let mut entries = fixture();
        entries.push(Entry::new(name, b"extra"));
        rejected(&entries);
    }
    let mut entries = fixture();
    entries[2].mode = 0o120777;
    rejected(&entries);
    let mut entries = fixture();
    entries[0].data = String::from_utf8(entries[0].data.clone())
        .unwrap()
        .replacen(
            "\"schema_version\": 1",
            "\"schema_version\": 1, \"schema_version\": 1",
            1,
        )
        .into_bytes();
    rejected(&entries);
    assert!(flightsim_content::parse_manifest(MANIFEST).is_err());
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.zip");
    fs::write(&input, zip::zip(&fixture())).unwrap();
    assert!(
        flightsim_content::stage_zip_with_progress(&input, &root.path().join("terrain"), |_| true)
            .is_err()
    );
    assert!(
        aircraft::parse_manifest(br#"{"schema_version":1,"content_kinds":["terrain_dem"]}"#)
            .is_err()
    );
}
#[test]
fn original_profile_whitespace_is_content_identity_not_physics_identity() {
    let root = tempfile::tempdir().unwrap();
    let a = stage(&fixture(), root.path()).unwrap();
    let identity = a.identity().clone();
    drop(a);
    let mut entries = fixture();
    entries[1].data.push(b' ');
    let size = entries[1].data.len();
    let hash = format!("{:x}", Sha256::digest(&entries[1].data));
    edit_manifest(&mut entries, |m| {
        m["files"][0]["size_bytes"] = size.into();
        m["files"][0]["sha256"] = hash.into();
    });
    let b = stage(&entries, root.path()).unwrap();
    assert_ne!(&identity, b.identity());
    let left: Value = serde_json::from_slice(PROFILE).unwrap();
    let right: Value = serde_json::from_slice(b.profile_bytes()).unwrap();
    assert_eq!(left, right);
}
#[test]
fn sizes_crc_hashes_and_installed_tampering_fail_closed() {
    let mut entries = fixture();
    entries[2].data[40] ^= 1;
    rejected(&entries);
    let mut entries = fixture();
    entries[2].size = Some(20 * 1024 * 1024);
    rejected(&entries);
    let root = tempfile::tempdir().unwrap();
    let installed = stage(&fixture(), root.path())
        .unwrap()
        .validate_profile(known_original)
        .unwrap()
        .commit()
        .unwrap();
    fs::write(installed.profile_path(), b"{}").unwrap();
    assert!(aircraft::inspect_installed(installed.directory()).is_err());
}
#[cfg(unix)]
#[test]
fn installed_links_and_linked_store_roots_fail() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let installed = stage(&fixture(), root.path())
        .unwrap()
        .validate_profile(known_original)
        .unwrap()
        .commit()
        .unwrap();
    let link = installed.directory().join("docs/extra");
    symlink(root.path(), &link).unwrap();
    assert!(aircraft::inspect_installed(installed.directory()).is_err());
    let store = root.path().join("linked");
    symlink(root.path().join("store"), &store).unwrap();
    assert!(
        aircraft::stage_zip_with_progress(&root.path().join("input.zip"), &store, |_| true)
            .is_err()
    );
}
#[test]
fn nonblocking_lock_retains_the_ready_stage_boundary() {
    use fs2::FileExt;
    let root = tempfile::tempdir().unwrap();
    let stage = stage(&fixture(), root.path())
        .unwrap()
        .validate_profile(known_original)
        .unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.path().join("store/.install.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    assert!(matches!(stage.commit(), Err(Error::StoreBusy)));
    assert!(!root.path().join("store/swift-sport-original").exists());
}

#[test]
fn implicit_directory_entries_are_bounded_before_extracting() {
    let mut entries = fixture();
    for i in 0..10 {
        let path = format!("docs/n{i}/a/b/c/d/e/test.txt");
        let data = b"original test documentation\n";
        let hash = format!("{:x}", Sha256::digest(data));
        edit_manifest(&mut entries, |m| {
            m["files"].as_array_mut().unwrap().push(serde_json::json!({"path":path,"kind":"documentation","size_bytes":data.len(),"sha256":hash,"source":"original-swift"}))
        });
        entries.push(Entry::new(&path, data));
    }
    rejected(&entries);
}

#[cfg(unix)]
#[test]
fn installed_hardlinks_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let installed = stage(&fixture(), root.path())
        .unwrap()
        .validate_profile(known_original)
        .unwrap()
        .commit()
        .unwrap();
    fs::hard_link(
        installed.profile_path(),
        root.path().join("second-profile-link"),
    )
    .unwrap();
    assert!(aircraft::inspect_installed(installed.directory()).is_err());
}

fn mutate_glb(entries: &mut [Entry], mutate: impl FnOnce(&mut Value, &mut Vec<u8>)) {
    let old = &entries[2].data;
    let length = usize::try_from(u32::from_le_bytes(old[12..16].try_into().unwrap())).unwrap();
    let mut document: Value = serde_json::from_slice(&old[20..20 + length]).unwrap();
    let mut binary = old[28 + length..].to_vec();
    mutate(&mut document, &mut binary);
    let mut json = serde_json::to_vec(&document).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    glb.extend(
        u32::try_from(28 + json.len() + binary.len())
            .unwrap()
            .to_le_bytes(),
    );
    glb.extend(u32::try_from(json.len()).unwrap().to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json);
    glb.extend(u32::try_from(binary.len()).unwrap().to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(binary);
    let size = glb.len();
    let hash = format!("{:x}", Sha256::digest(&glb));
    entries[2].data = glb;
    edit_manifest(entries, |m| {
        m["files"][1]["size_bytes"] = size.into();
        m["files"][1]["sha256"] = hash.into();
    });
}
#[test]
fn hash_valid_glb_external_features_and_resource_topology_are_rejected() {
    for field in [
        "images",
        "textures",
        "animations",
        "skins",
        "cameras",
        "extensionsUsed",
    ] {
        let mut entries = fixture();
        mutate_glb(&mut entries, |d, _| d[field] = serde_json::json!([]));
        rejected(&entries);
    }
    for uri in [
        "https://example.test/model.bin",
        "../escape.bin",
        "data:application/octet-stream;base64,AA==",
    ] {
        let mut entries = fixture();
        mutate_glb(&mut entries, |d, _| d["buffers"][0]["uri"] = uri.into());
        rejected(&entries);
    }
    for mode in 0..5 {
        let mut entries = fixture();
        mutate_glb(&mut entries, |d, _| match mode {
            0 => d["nodes"][0]["children"] = serde_json::json!([0]),
            1 => d["scenes"][0]["nodes"] = serde_json::json!(vec![0; 1000]),
            2 => {
                d["nodes"][0]["matrix"] =
                    serde_json::json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])
            }
            3 => d["nodes"][0]["scale"] = serde_json::json!([0, 1, 1]),
            _ => d["accessors"][0]["count"] = serde_json::json!(u32::MAX),
        });
        rejected(&entries);
    }
}
#[test]
fn hash_valid_glb_binary_nan_lied_bounds_and_degenerate_triangles_reject() {
    for mode in 0..3 {
        let mut entries = fixture();
        mutate_glb(&mut entries, |d, b| match mode {
            0 => {
                b[..4].copy_from_slice(&f32::NAN.to_le_bytes());
            }
            1 => d["accessors"][0]["max"][0] = serde_json::json!(10000),
            _ => {
                let accessor =
                    usize::try_from(d["meshes"][0]["primitives"][0]["indices"].as_u64().unwrap())
                        .unwrap();
                let view =
                    usize::try_from(d["accessors"][accessor]["bufferView"].as_u64().unwrap())
                        .unwrap();
                let offset =
                    usize::try_from(d["bufferViews"][view]["byteOffset"].as_u64().unwrap())
                        .unwrap();
                let length =
                    usize::try_from(d["bufferViews"][view]["byteLength"].as_u64().unwrap())
                        .unwrap();
                b[offset..offset + length].fill(0);
            }
        });
        rejected(&entries);
    }
}

#[test]
fn zip_windows_reparse_metadata_is_rejected_on_every_host() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = zip::zip(&fixture());
    let at = bytes
        .windows(4)
        .position(|b| b == [0x50, 0x4b, 0x01, 0x02])
        .unwrap()
        + 38;
    let attrs = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) | 0x400;
    bytes[at..at + 4].copy_from_slice(&attrs.to_le_bytes());
    let path = root.path().join("reparse.zip");
    fs::write(&path, bytes).unwrap();
    assert!(
        aircraft::stage_zip_with_progress(&path, &root.path().join("store"), |_| true).is_err()
    );
}
