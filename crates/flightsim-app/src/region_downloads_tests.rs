use super::super::tests::{TestDirectory, await_worker, world};
use super::*;
use bevy::ecs::system::RunSystemOnce;
use serde_json::{Value, json};

// Independently generated Python ZIP/FSDM fixture from flightsim-content. No test
// contacts this synthetic URL. Fixed hashes also detect unintended fixture drift.
const URL: &str = "https://github.com/example/terrain/releases/download/v1/prepared.zip";
const HASH: &str = "ef3d47de1b40f09c5f4e377551a1f6fc99d8b599641db384301be2ec2e24c995";
const CACHE_KEY: &str = "6c07577563fdf3aa6e3ac951a4c1695e8fe267331efba72e5ac708e7475735ce";
const MANIFEST_HASH: &str = "2965ac386f46cc3bdca4539135305938292df0299db70de38a581d0d1b67f7bf";
const KEY: &str = "org.example.download@1.0.0";
const ZIP: &[u8] = include_bytes!("../../flightsim-content/tests/data/download-fixture.zip");

fn catalog() -> Value {
    json!({"schema_version": 1, "regions": [{
        "id": "org.example.download", "version": "1.0.0",
        "title": "Synthetic download fixture",
        "bounds_degrees": {"west": -180.0, "south": -90.0, "east": 180.0, "north": 90.0},
        "url": URL, "archive_sha256": HASH,
        "provenance": "Synthetic test fixture only. Not real geographic data or rights clearance."
    }]})
}
fn parse(value: &Value) -> Result<Vec<Entry>, String> {
    parse_catalog(&serde_json::to_vec(value).unwrap())
}
fn entry() -> Entry {
    parse(&catalog()).unwrap().remove(0)
}
fn seed_cache(temp: &TestDirectory) -> PathBuf {
    let cache = temp.0.join("cache");
    let directory = cache.join(CACHE_KEY);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("package.zip"), ZIP).unwrap();
    let receipt = json!({"schema_version":1, "source_url":URL, "archive_sha256":HASH,
        "package_id":"org.example.download", "package_version":"1.0.0", "manifest_sha256":MANIFEST_HASH});
    std::fs::write(
        directory.join("source.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    cache
}
fn configured(temp: &TestDirectory) -> World {
    let mut world = world(temp);
    let path = temp.0.join("catalog.json");
    std::fs::write(&path, serde_json::to_vec(&catalog()).unwrap()).unwrap();
    let options = Options {
        catalog: Some(path),
        cache: Some(temp.0.join("cache")),
        ..Default::default()
    };
    world.resource_mut::<RegionRuntime>().downloads = DownloadRuntime::new(&options);
    let job = world.resource::<RegionRuntime>().refresh_job();
    world.resource_mut::<RegionRuntime>().start(job).unwrap();
    await_worker(&mut world);
    world
}
fn action(world: &mut World, action: RegionAction) {
    world.resource_mut::<WorldMapActions>().regions.pending = Some(action);
    world.run_system_once(update).unwrap();
}

#[test]
fn strict_catalog_rejects_missing_unknown_duplicate_fields_and_oversized_inputs() {
    assert_eq!(parse(&catalog()).unwrap().len(), 1);
    let mut boundary = catalog();
    boundary["regions"] = json!(
        (0..MAX_CATALOG_ENTRIES)
            .map(|index| {
                let mut record = catalog()["regions"][0].clone();
                record["id"] = json!(format!("region-{index}"));
                record["url"] =
                    json!(URL.replace("prepared.zip", &format!("prepared-{index}.zip")));
                record
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(parse(&boundary).unwrap().len(), MAX_CATALOG_ENTRIES);
    for field in [
        "id",
        "version",
        "title",
        "bounds_degrees",
        "url",
        "archive_sha256",
        "provenance",
    ] {
        let mut value = catalog();
        value["regions"][0].as_object_mut().unwrap().remove(field);
        assert!(parse(&value).is_err(), "missing {field}");
    }
    for pointer in ["", "/regions/0", "/regions/0/bounds_degrees"] {
        let mut value = catalog();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(1));
        assert!(parse(&value).is_err(), "unknown at {pointer}");
    }
    let text = serde_json::to_string(&catalog()).unwrap();
    for (from, to) in [
        (
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        (
            "\"version\":\"1.0.0\"",
            "\"version\":\"1.0.0\",\"version\":\"1.0.0\"",
        ),
        ("\"west\":-180.0", "\"west\":-180.0,\"west\":-180.0"),
    ] {
        assert!(text.contains(from));
        assert!(parse_catalog(text.replace(from, to).as_bytes()).is_err());
    }
    let mut value = catalog();
    value["schema_version"] = json!(2);
    assert!(parse(&value).is_err());
    let mut value = catalog();
    value["regions"] = json!(vec![catalog()["regions"][0].clone(); 65]);
    assert!(parse(&value).is_err());
    assert!(parse_catalog(&vec![b' '; usize::try_from(MAX_CATALOG_BYTES).unwrap() + 1]).is_err());
}

#[test]
fn catalog_rejects_path_version_text_and_network_aliases() {
    for (field, values) in [
        (
            "id",
            vec![
                "../outside",
                "a/b",
                "a\\b",
                "con",
                "con.terrain",
                "lpt1",
                "nul",
                "bad.",
                "UPPER",
                "a@b",
                "a:stream",
            ],
        ),
        (
            "version",
            vec![
                "1.0",
                "01.0.0",
                "1.0.0-alpha",
                "1.0.0/../x",
                "1.0.0.0",
                "1000000000.0.0",
            ],
        ),
        (
            "title",
            vec!["", "  ", "bad\nline", "bad\tlabel", "bad\u{1b}"],
        ),
        ("provenance", vec!["", "  ", "bad\rline", "bad\0text"]),
        (
            "url",
            vec![
                "http://github.com/example/terrain/releases/download/v1/prepared.zip",
                "https://example.org/terrain.zip",
                "https://raw.githubusercontent.com/example/terrain/main/terrain.zip",
                "https://github.com/example/terrain/archive/main.zip",
                "https://github.com/example/terrain/releases/download/latest/prepared.zip",
            ],
        ),
        (
            "archive_sha256",
            vec![
                "",
                "1234",
                "EF3D47DE1B40F09C5F4E377551A1F6FC99D8B599641DB384301BE2EC2E24C995",
            ],
        ),
    ] {
        for bad in values {
            let mut value = catalog();
            value["regions"][0][field] = json!(bad);
            assert!(parse(&value).is_err(), "{field}: {bad:?}");
        }
    }
    for (field, limit) in [("title", 160), ("provenance", 8192)] {
        let mut value = catalog();
        value["regions"][0][field] = json!("a".repeat(limit + 1));
        assert!(parse(&value).is_err());
    }
    let mut duplicate = catalog();
    duplicate["regions"]
        .as_array_mut()
        .unwrap()
        .push(catalog()["regions"][0].clone());
    assert!(parse(&duplicate).unwrap_err().contains("duplicate ID"));
    duplicate["regions"][1]["id"] = json!("other");
    duplicate["regions"][1]["archive_sha256"] = json!("0".repeat(64));
    assert!(
        parse(&duplicate)
            .unwrap_err()
            .contains("duplicate archive source")
    );
}

#[test]
fn bounds_are_finite_nonempty_and_antimeridian_center_is_wrapped() {
    for bounds in [
        [-181.0, -90.0, 180.0, 90.0],
        [-180.0, -91.0, 180.0, 90.0],
        [-180.0, -90.0, 181.0, 90.0],
        [-180.0, -90.0, 180.0, 91.0],
        [0.0, -10.0, 0.0, 10.0],
        [-10.0, 0.0, 10.0, 0.0],
        [-10.0, 10.0, 10.0, -10.0],
        [180.0, -10.0, -180.0, 10.0],
    ] {
        let mut value = catalog();
        value["regions"][0]["bounds_degrees"] =
            json!({"west":bounds[0],"south":bounds[1],"east":bounds[2],"north":bounds[3]});
        assert!(parse(&value).is_err());
    }
    for nonfinite in ["1e400", "NaN", "Infinity", "null"] {
        let text = serde_json::to_string(&catalog())
            .unwrap()
            .replace("-180.0", nonfinite);
        assert!(parse_catalog(text.as_bytes()).is_err());
    }
    let mut value = catalog();
    value["regions"][0]["bounds_degrees"] =
        json!({"west":170.0,"south":10.0,"east":-170.0,"north":20.0});
    let center = parse(&value).unwrap()[0].center();
    assert!((center.latitude.to_degrees().get() - 15.0).abs() < 1e-9);
    assert!((center.longitude.to_degrees().get().abs() - 180.0).abs() < 1e-9);
}

#[test]
fn catalog_file_reader_rejects_directories_oversize_and_links() {
    let temp = TestDirectory::new();
    assert!(read_catalog(&temp.0).is_err());
    let path = temp.0.join("catalog.json");
    std::fs::write(
        &path,
        vec![b' '; usize::try_from(MAX_CATALOG_BYTES).unwrap() + 1],
    )
    .unwrap();
    assert!(read_catalog(&path).is_err());
    std::fs::write(&path, serde_json::to_vec(&catalog()).unwrap()).unwrap();
    assert_eq!(read_catalog(&path).unwrap().len(), 1);
    #[cfg(unix)]
    {
        let link = temp.0.join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_catalog(&link).is_err());
    }
}

#[test]
fn preview_changes_only_map_and_refresh_requires_reselection_for_changed_claims() {
    let temp = TestDirectory::new();
    let mut world = configured(&temp);
    let before = *world.resource::<FlightSimulation>().0.state();
    let start_before = world.resource::<WorldMapState>().selected;
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    let map = world.resource::<WorldMapState>();
    assert_ne!(map.selected, start_before);
    assert!(map.regions.download_credits.contains(URL));
    assert!(map.regions.download_credits.contains(HASH));
    assert!(
        map.regions
            .download_credits
            .contains("not publisher authentication")
    );
    assert!(map.regions.selected.is_none());
    assert!(map.regions.active.is_none());
    assert!(world.resource::<RegionRuntime>().pending.is_none());
    assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
    action(&mut world, RegionAction::Refresh);
    await_worker(&mut world);
    assert_eq!(
        world
            .resource::<WorldMapState>()
            .regions
            .download_selected
            .as_deref(),
        Some(KEY)
    );
    let mut changed = catalog();
    changed["regions"][0]["provenance"] = json!("Changed publisher claim");
    std::fs::write(
        temp.0.join("catalog.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    action(&mut world, RegionAction::Refresh);
    await_worker(&mut world);
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .download_selected
            .is_none()
    );
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    std::fs::write(temp.0.join("catalog.json"), b"not json").unwrap();
    action(&mut world, RegionAction::Refresh);
    await_worker(&mut world);
    let map = world.resource::<WorldMapState>();
    assert!(map.regions.downloads().is_empty());
    assert!(map.regions.download_selected.is_none());
    assert!(!map.regions.error.is_empty());
    action(&mut world, RegionAction::Download { offline: true });
    assert!(world.resource::<RegionRuntime>().pending.is_none());
}

#[test]
fn offline_mode_is_enforced_and_cache_miss_retries_are_explicit() {
    let temp = TestDirectory::new();
    let mut world = configured(&temp);
    world
        .resource_mut::<RegionRuntime>()
        .downloads
        .as_mut()
        .unwrap()
        .offline = true;
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    let job = world
        .resource::<RegionRuntime>()
        .downloads
        .as_ref()
        .unwrap()
        .job(false)
        .unwrap();
    assert!(matches!(
        job,
        Job::Download {
            mode: CacheMode::Offline,
            ..
        }
    ));
    action(&mut world, RegionAction::Download { offline: false });
    await_worker(&mut world);
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .error
            .contains("not available offline")
    );
    assert!(
        flightsim_content::list_installed(&temp.store())
            .unwrap()
            .is_empty()
    );
    seed_cache(&temp);
    // Just receiving cache bytes does not cause an automatic retry or activation.
    world.run_system_once(update).unwrap();
    assert!(world.resource::<RegionRuntime>().pending.is_none());
    assert!(
        flightsim_content::list_installed(&temp.store())
            .unwrap()
            .is_empty()
    );
    action(&mut world, RegionAction::Download { offline: false });
    await_worker(&mut world);
    let map = world.resource::<WorldMapState>();
    assert!(map.regions.error.is_empty());
    assert!(map.regions.status.contains("verified cache"));
    assert_eq!(map.regions.installed().len(), 1);
    assert!(map.regions.selected.is_none());
    assert!(map.regions.active.is_none());
    assert!(world.resource::<Startup>().active_region.is_none());
    assert!(world.resource::<RegionRuntime>().ready.is_none());
    action(&mut world, RegionAction::Select(Some(KEY.into())));
    assert!(world.resource::<Startup>().active_region.is_none());
    let request = WorldMapStart {
        aircraft_choice: 0,
        generation: 0,
        position: world.resource::<WorldMapState>().selected,
        month: world.resource::<WorldMapState>().preview_month(),
    };
    world.resource_mut::<WorldMapActions>().start_at = Some(request);
    world_runtime::apply_world_map_start(&mut world);
    await_worker(&mut world);
    world_runtime::apply_world_map_start(&mut world);
    assert_eq!(
        world
            .resource::<Startup>()
            .active_region
            .as_ref()
            .unwrap()
            .identity()
            .id,
        "org.example.download"
    );
}

#[test]
fn staged_manifest_claim_mismatch_never_commits_and_preserves_valid_cache() {
    for field in ["id", "version", "title", "bounds"] {
        let temp = TestDirectory::new();
        let cache = seed_cache(&temp);
        let mut candidate = entry();
        match field {
            "id" => candidate.key = "other@1.0.0".into(),
            "version" => candidate.key = "org.example.download@2.0.0".into(),
            "title" => candidate.title = "Different title".into(),
            "bounds" => candidate.bounds.north = 80.0,
            _ => unreachable!(),
        }
        let error = match download(
            &candidate,
            &cache,
            &temp.store(),
            CacheMode::Offline,
            &AtomicBool::new(false),
            &Mutex::new(None),
        ) {
            Ok(_) => panic!("{field} mismatch installed"),
            Err(error) => error,
        };
        assert!(error.contains("disagrees with catalog"));
        assert!(
            flightsim_content::list_installed(&temp.store())
                .unwrap()
                .is_empty()
        );
        assert!(cache.join(CACHE_KEY).join("package.zip").is_file());
        assert!(std::fs::read_dir(temp.store()).unwrap().all(|p| {
            let name = p.unwrap().file_name();
            let name = name.to_string_lossy();
            !name.starts_with(".import-") && !name.starts_with(".archive-")
        }));
        assert!(std::fs::read_dir(&cache).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".download-")
        }));
    }
}

#[test]
fn cancellation_and_stale_completion_do_not_select_install_or_start_a_flight() {
    let temp = TestDirectory::new();
    let cache = seed_cache(&temp);
    assert!(
        download(
            &entry(),
            &cache,
            &temp.store(),
            CacheMode::Offline,
            &AtomicBool::new(true),
            &Mutex::new(None)
        )
        .is_err()
    );
    assert!(
        flightsim_content::list_installed(&temp.store())
            .unwrap()
            .is_empty()
    );
    let mut world = configured(&temp);
    let before = *world.resource::<FlightSimulation>().0.state();
    let (sender, receiver) = mpsc::sync_channel(1);
    let generation = world.resource::<RegionRuntime>().generation;
    world.resource_mut::<RegionRuntime>().pending = Some(Worker {
        generation,
        cancel: Arc::new(AtomicBool::new(false)),
        progress: Arc::new(Mutex::new(Some(Progress::Download(
            DownloadProgress::Receiving {
                bytes_done: 20,
                bytes_total: Some(100),
            },
        )))),
        receiver: Mutex::new(receiver),
    });
    for offline in [false, true, false] {
        action(&mut world, RegionAction::Download { offline });
        assert_eq!(world.resource::<RegionRuntime>().generation, generation);
    }
    action(&mut world, RegionAction::Cancel);
    assert!(
        world
            .resource_mut::<RegionRuntime>()
            .start(Job::List)
            .is_err()
    );
    assert!(
        sender
            .send(Ok(Outcome::Downloaded {
                installed: Vec::new(),
                key: KEY.into(),
                cache_hit: true
            }))
            .is_ok()
    );
    world.run_system_once(update).unwrap();
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .status
            .starts_with("Cancelled")
    );
    assert!(world.resource::<WorldMapState>().regions.progress.is_none());
    assert!(world.resource::<WorldMapState>().regions.selected.is_none());
    assert!(world.resource::<RegionRuntime>().pending.is_none());
    assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
}

#[test]
fn download_progress_handles_unknown_totals_and_u64_limits_without_overflow() {
    assert_eq!(
        progress_text(DownloadProgress::Receiving {
            bytes_done: 4,
            bytes_total: None
        })
        .0,
        None
    );
    assert_eq!(
        progress_text(DownloadProgress::Receiving {
            bytes_done: 0,
            bytes_total: Some(0)
        })
        .0,
        None
    );
    assert_eq!(
        progress_text(DownloadProgress::Receiving {
            bytes_done: u64::MAX,
            bytes_total: Some(u64::MAX)
        })
        .0,
        Some(100)
    );
    assert_eq!(
        progress_text(DownloadProgress::VerifyingCache {
            bytes_done: 50,
            bytes_total: 100
        })
        .0,
        Some(50)
    );
}

#[test]
fn closing_map_discards_download_result_and_requires_explicit_retry_after_reopen() {
    let temp = TestDirectory::new();
    let mut world = configured(&temp);
    let (sender, receiver) = mpsc::sync_channel(1);
    let generation = world.resource::<RegionRuntime>().generation;
    world.resource_mut::<RegionRuntime>().pending = Some(Worker {
        generation,
        cancel: Arc::new(AtomicBool::new(false)),
        progress: Arc::new(Mutex::new(None)),
        receiver: Mutex::new(receiver),
    });
    world.resource_mut::<WorldMapState>().visible = false;
    world.run_system_once(update).unwrap();
    assert!(
        world
            .resource::<RegionRuntime>()
            .pending
            .as_ref()
            .unwrap()
            .cancel
            .load(Ordering::Relaxed)
    );
    assert!(
        sender
            .send(Ok(Outcome::Downloaded {
                installed: Vec::new(),
                key: KEY.into(),
                cache_hit: true
            }))
            .is_ok()
    );
    world.resource_mut::<WorldMapState>().visible = true;
    world.run_system_once(update).unwrap();
    assert!(world.resource::<RegionRuntime>().pending.is_none());
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .status
            .contains("cancelled")
    );
    assert!(world.resource::<Startup>().active_region.is_none());
    assert!(world.resource::<WorldMapState>().regions.selected.is_none());
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    action(&mut world, RegionAction::Download { offline: true });
    await_worker(&mut world);
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .error
            .contains("not available offline")
    );
}

#[test]
fn failed_installed_listing_still_reconciles_changed_and_invalid_catalogs() {
    let temp = TestDirectory::new();
    let mut world = configured(&temp);
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    std::fs::write(temp.store(), b"not a store directory").unwrap();
    let mut changed = catalog();
    changed["regions"][0]["provenance"] = json!("Updated catalog claims");
    std::fs::write(
        temp.0.join("catalog.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    action(&mut world, RegionAction::Refresh);
    await_worker(&mut world);
    assert!(
        world
            .resource::<WorldMapState>()
            .regions
            .download_selected
            .is_none()
    );
    assert_eq!(
        world.resource::<WorldMapState>().regions.downloads().len(),
        1
    );
    assert!(!world.resource::<WorldMapState>().regions.error.is_empty());
    action(&mut world, RegionAction::SelectDownload(KEY.into()));
    std::fs::write(temp.0.join("catalog.json"), b"invalid catalog").unwrap();
    action(&mut world, RegionAction::Refresh);
    await_worker(&mut world);
    let map = world.resource::<WorldMapState>();
    assert!(map.regions.download_selected.is_none());
    assert!(map.regions.downloads().is_empty());
    assert!(map.regions.error.contains("Invalid region catalog"));
    assert!(
        world
            .resource::<RegionRuntime>()
            .downloads
            .as_ref()
            .unwrap()
            .job(true)
            .is_err()
    );
}
