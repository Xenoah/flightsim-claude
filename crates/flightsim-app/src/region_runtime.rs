//! Local prepared terrain imports. One bounded worker; explicit new-flight activation.
use super::*;
use flightsim_content::{ImportProgress, InstalledPackage, InstalledSummary};
use flightsim_ui::world_map::{
    RegionAction, RegionSummary, WorldMapActions, WorldMapStart, WorldMapState,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

pub(super) const REPLAY_NOTICE: &str = "Regional terrain: replay recording/export and playback are unavailable (v1/v2 cannot identify packages).";

#[derive(Debug, Clone, Default)]
pub(super) struct Options {
    pub import: Option<PathBuf>,
    pub select: Option<String>,
    pub store: Option<PathBuf>,
    pub list: bool,
    pub error: Option<String>,
}

pub(super) fn validate_options(startup: &mut Startup) {
    let options = &startup.regions;
    let modes = usize::from(options.import.is_some())
        + usize::from(options.select.is_some())
        + usize::from(options.list);
    let error = if modes > 1 {
        Some("--import-region, --region and --list-regions are separate operations")
    } else if options
        .store
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        Some("--region-store needs a nonempty directory")
    } else if options.select.as_deref().is_some_and(|key| !valid_key(key)) {
        Some("--region expects a canonical package ID@MAJOR.MINOR.PATCH")
    } else if modes > 0 && startup.replay.is_some() {
        Some(
            "region operations cannot be combined with --replay; package-backed replay v1/v2 is unsupported",
        )
    } else if options.select.is_some() && startup.tiles.is_some() {
        Some("--region and --tiles cannot be combined; choose one regional source")
    } else if options.select.is_some() && !startup.world.global_terrain {
        Some("--region requires --global-terrain on for fallback")
    } else {
        None
    };
    if let Some(error) = error {
        startup.regions.error = Some(error.into());
    }
    if startup.regions.select.is_some() {
        startup.world.map_open = true;
    }
}

fn valid_key(key: &str) -> bool {
    let Some((id, version)) = key.split_once('@') else {
        return false;
    };
    let mut parts = version.split('.');
    let valid_version = (0..3).all(|_| {
        parts.next().is_some_and(|part| {
            !part.is_empty()
                && part.len() <= 9
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
                && part.parse::<u32>().is_ok()
        })
    }) && parts.next().is_none();
    !id.is_empty()
        && id.len() <= 80
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
        && valid_version
}

fn default_store() -> Result<PathBuf, String> {
    default_store_from(std::env::consts::OS, |name| {
        std::env::var_os(name).map(PathBuf::from)
    })
}

fn default_store_from(os: &str, get: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf, String> {
    let absolute = |path: Option<PathBuf>| path.filter(|path| path.is_absolute());
    let base = if os == "windows" {
        absolute(get("LOCALAPPDATA"))
    } else if os == "macos" {
        absolute(get("HOME")).map(|home| home.join("Library/Application Support"))
    } else {
        absolute(get("XDG_DATA_HOME"))
            .or_else(|| absolute(get("HOME")).map(|home| home.join(".local/share")))
    };
    base.map(|base| base.join("flightsim-claude/regions"))
        .ok_or_else(|| "Cannot find per-user application data; specify --region-store DIR".into())
}

pub(super) fn store(options: &Options) -> Result<PathBuf, String> {
    options.store.clone().map_or_else(default_store, Ok)
}

/// CLI maintenance runs before creating a renderer. Import never starts a flight.
pub(super) fn run_cli(options: &Options) -> Result<bool, String> {
    if options.import.is_none() && !options.list {
        return Ok(false);
    }
    let store = store(options)?;
    if let Some(zip) = &options.import {
        let installed = flightsim_content::stage_zip_with_progress(zip, &store, |_| true)
            .and_then(flightsim_content::StagedPackage::commit)
            .map_err(|e| e.to_string())?;
        println!(
            "Installed {}@{} in {}. Select with --region {}@{} and Start new flight on the map.",
            installed.identity().id,
            installed.identity().version,
            store.display(),
            installed.identity().id,
            installed.identity().version
        );
    } else {
        for summary in flightsim_content::list_installed(&store).map_err(|e| e.to_string())? {
            println!(
                "{}\t{}",
                key(&summary),
                summary.manifest.title.escape_default()
            );
        }
    }
    Ok(true)
}

fn key(summary: &InstalledSummary) -> String {
    format!("{}@{}", summary.identity.id, summary.identity.version)
}

#[derive(Resource)]
pub(super) struct RegionRuntime {
    store: Result<PathBuf, String>,
    installed: Vec<InstalledSummary>,
    selected: Option<String>,
    selected_snapshot: Option<InstalledSummary>,
    initial_selection: Option<String>,
    pending: Option<Worker>,
    generation: u64,
    ready: Option<(WorldMapStart, Arc<InstalledPackage>)>,
    was_map_visible: bool,
    initialized: bool,
}
struct Worker {
    generation: u64,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Option<ImportProgress>>>,
    receiver: Mutex<Receiver<Result<Outcome, String>>>,
}
enum Job {
    List,
    Import(PathBuf),
    Inspect {
        directory: PathBuf,
        expected: flightsim_content::RegionalIdentity,
        start: WorldMapStart,
    },
}
enum Outcome {
    Listed(Vec<InstalledSummary>),
    Imported(Vec<InstalledSummary>),
    Inspected(WorldMapStart, Arc<InstalledPackage>),
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl RegionRuntime {
    fn new(startup: &Startup) -> Self {
        Self {
            store: store(&startup.regions),
            installed: Vec::new(),
            selected: None,
            selected_snapshot: None,
            initial_selection: startup.regions.select.clone(),
            pending: None,
            generation: 0,
            ready: None,
            was_map_visible: false,
            initialized: false,
        }
    }
    fn start(&mut self, job: Job) -> Result<(), String> {
        if self.pending.is_some() {
            return Err(
                "A region operation is still finishing; cancel or wait before another operation"
                    .into(),
            );
        }
        let store = self.store.clone()?;
        self.generation = self.generation.wrapping_add(1);
        self.ready = None;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(None));
        let worker_cancel = Arc::clone(&cancel);
        let worker_progress = Arc::clone(&progress);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("region-content".into())
            .spawn(move || {
                let result = run_job(job, &store, &worker_cancel, &worker_progress);
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Cannot start region worker: {error}"))?;
        self.pending = Some(Worker {
            generation: self.generation,
            cancel,
            progress,
            receiver: Mutex::new(receiver),
        });
        Ok(())
    }
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.ready = None;
        if let Some(worker) = &self.pending {
            worker.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn run_job(
    job: Job,
    store: &std::path::Path,
    cancel: &AtomicBool,
    progress: &Mutex<Option<ImportProgress>>,
) -> Result<Outcome, String> {
    let report = |next| {
        if let Ok(mut progress) = progress.lock() {
            *progress = Some(next);
        }
        !cancel.load(Ordering::Relaxed)
    };
    let check = || -> Result<(), String> {
        if cancel.load(Ordering::Relaxed) {
            Err("Region operation cancelled".into())
        } else {
            Ok(())
        }
    };
    check()?;
    let result = match job {
        Job::List => {
            Outcome::Listed(flightsim_content::list_installed(store).map_err(|e| e.to_string())?)
        }
        Job::Import(path) => {
            let staged = flightsim_content::stage_zip_with_progress(&path, store, report)
                .map_err(|e| e.to_string())?;
            check()?;
            staged.commit().map_err(|e| e.to_string())?;
            Outcome::Imported(flightsim_content::list_installed(store).map_err(|e| e.to_string())?)
        }
        Job::Inspect {
            directory,
            expected,
            start,
        } => {
            let installed = flightsim_content::inspect_installed_with_progress(&directory, report)
                .map_err(|e| e.to_string())?;
            if installed.identity() != &expected {
                return Err(
                    "Selected package metadata changed. Refresh and select it again".into(),
                );
            }
            Outcome::Inspected(start, Arc::new(installed))
        }
    };
    check()?;
    Ok(result)
}

pub(super) fn configure(app: &mut App) {
    let runtime = RegionRuntime::new(app.world().resource::<Startup>());
    app.insert_resource(runtime).add_systems(
        Update,
        update
            .after(flightsim_ui::world_map::WorldMapSystems::Input)
            .before(world_runtime::apply_world_map_start),
    );
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut runtime: ResMut<RegionRuntime>,
    startup: Res<Startup>,
    playback: Option<Res<ReplayPlayback>>,
    mut map: ResMut<WorldMapState>,
    mut actions: ResMut<WorldMapActions>,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
) {
    let enabled = playback.is_none() && startup.world.global_terrain && runtime.store.is_ok();
    map.regions.operations_enabled = enabled;
    map.regions.active = startup
        .active_region
        .as_ref()
        .map(|p| format!("{}@{}", p.identity().id, p.identity().version));
    map.regions.store = runtime
        .store
        .as_ref()
        .map_or_else(Clone::clone, |path| path.display().to_string());
    let mut cancelled_this_frame = runtime.was_map_visible && !map.visible;
    if runtime.was_map_visible
        && !map.visible
        && (runtime.pending.is_some() || runtime.ready.is_some())
    {
        runtime.cancel();
        actions.start_at = None;
        map.regions.progress = None;
        map.regions.status =
            "Operation cancelled when the map closed; current flight unchanged".into();
    }
    runtime.was_map_visible = map.visible;
    if !runtime.initialized && map.visible {
        runtime.initialized = true;
        if runtime.initial_selection.is_some() {
            map.show_regions();
        }
        if let Err(error) = runtime.start(Job::List) {
            map.regions.error = error;
        }
    }
    if let Some(action) = actions.regions.pending.take() {
        match action {
            RegionAction::Cancel => {
                cancelled_this_frame = true;
                runtime.cancel();
                actions.start_at = None;
                map.regions.progress = None;
                map.regions.status = "Cancelled; current flight unchanged. Refresh installed packages if an import already committed".into();
            }
            RegionAction::Refresh if map.visible && enabled => {
                map.regions.error.clear();
                if let Err(error) = runtime.start(Job::List) {
                    map.regions.error = error;
                }
            }
            RegionAction::Select(selected)
                if map.visible && enabled && runtime.pending.is_none() =>
            {
                select(&mut runtime, &mut map, selected);
            }
            _ => {}
        }
    }
    for event in drops.read() {
        if let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            if !map.visible || !enabled || cancelled_this_frame {
                continue;
            }
            actions.start_at = None;
            if runtime.pending.is_some() {
                map.regions.error =
                    "A region operation is already running; no drop was queued".into();
                continue;
            }
            map.show_regions();
            if !path_buf
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
            {
                map.regions.error = "Drop a prepared regional .zip package (raw DEMs and repository ZIPs are unsupported)".into();
                continue;
            }
            map.regions.error.clear();
            if let Err(error) = runtime.start(Job::Import(path_buf.clone())) {
                map.regions.error = error;
            }
        }
    }
    let completion = runtime.pending.as_ref().and_then(|worker| {
        if worker.generation == runtime.generation
            && let Ok(progress) = worker.progress.lock()
            && let Some(p) = *progress
        {
            map.regions.progress = (p.bytes_total > 0).then(|| {
                u8::try_from((p.bytes_done.saturating_mul(100) / p.bytes_total).min(100))
                    .unwrap_or(100)
            });
            map.regions.status = format!(
                "{:?}: {}/{} files, {} / {} bytes",
                p.phase, p.files_done, p.files_total, p.bytes_done, p.bytes_total
            );
        }
        match worker
            .receiver
            .lock()
            .expect("region receiver lock")
            .try_recv()
        {
            Ok(result) => Some((worker.generation, result)),
            Err(TryRecvError::Disconnected) => Some((
                worker.generation,
                Err("Region worker stopped unexpectedly".into()),
            )),
            Err(TryRecvError::Empty) => None,
        }
    });
    if let Some((generation, result)) = completion {
        runtime.pending = None;
        map.regions.progress = None;
        if generation == runtime.generation && map.visible {
            match result {
                Ok(Outcome::Listed(installed)) | Ok(Outcome::Imported(installed)) => {
                    // Selection remains explicit even after import. CLI selection is a
                    // one-time pending choice, never an activation request.
                    runtime.installed = installed;
                    map.regions
                        .set_installed(runtime.installed.iter().map(|s| RegionSummary {
                            key: key(s),
                            name: s.manifest.title.clone(),
                        }));
                    map.regions.status = format!(
                        "{} installed versions. Select terrain, then return to the map and Start new flight",
                        runtime.installed.len()
                    );
                    if let Some(selected) = runtime.initial_selection.take() {
                        runtime.selected = Some(selected.clone());
                        select(&mut runtime, &mut map, Some(selected));
                    }
                }
                Ok(Outcome::Inspected(start, package)) => {
                    if map.selected == start.position && map.preview_month() == start.month {
                        runtime.ready = Some((start, package));
                        map.regions.status =
                            "Inspection complete; starting the requested new flight".into();
                    } else {
                        map.regions.status = "Departure changed during inspection. Current flight unchanged; press Start again".into();
                    }
                }
                Err(error) => {
                    map.regions.error = error;
                    map.regions.status = "Current flight unchanged".into();
                }
            }
        }
    }
    map.regions.busy = runtime.pending.is_some();
    map.regions.selected = runtime.selected.clone();
}

fn select(runtime: &mut RegionRuntime, map: &mut WorldMapState, selected: Option<String>) {
    runtime.ready = None;
    map.regions.error.clear();
    if let Some(key) = &selected {
        let Some(summary) = runtime.installed.iter().find(|s| key == &self::key(s)) else {
            map.regions.error =
                format!("Region {key} is not installed. Import it or refresh the list");
            return;
        };
        let bounds = &summary.manifest.terrain.bounds_degrees;
        let east = if bounds.east < bounds.west {
            bounds.east + 360.0
        } else {
            bounds.east
        };
        let position = Geodetic::new(
            Degrees((bounds.south + bounds.north) * 0.5).to_radians(),
            Degrees((bounds.west + east) * 0.5)
                .to_radians()
                .wrap_signed(),
            Meters::ZERO,
        );
        map.select(position, &summary.manifest.title);
        map.regions.credits = credits(&summary.manifest);
        runtime.selected_snapshot = Some(summary.clone());
    } else {
        runtime.selected_snapshot = None;
        map.regions.credits =
            "Bundled global data and any original --tiles source. See Data credits on the map."
                .into();
    }
    runtime.selected = selected;
    map.regions.selected = runtime.selected.clone();
    map.regions.status = "Pending selection only. Return to the map, choose a point and Start new flight to apply it".into();
}

pub(super) fn credits(manifest: &flightsim_content::Manifest) -> String {
    let mut text = format!(
        "{}\n{}@{}\nDeclared coverage: {:.4},{:.4} to {:.4},{:.4}; nominal resolution {} m; EPSG:4979\nURLs are metadata only. No download or rights clearance is implied.\n",
        manifest.title,
        manifest.id,
        manifest.version,
        manifest.terrain.bounds_degrees.west,
        manifest.terrain.bounds_degrees.south,
        manifest.terrain.bounds_degrees.east,
        manifest.terrain.bounds_degrees.north,
        manifest.terrain.nominal_resolution_m
    );
    for source in &manifest.sources {
        text.push_str(&format!(
            "\n{}\n{}\nRevision: {}\n{}\n{}\nLicense: {}\n{}\nLocal license file: {}\n",
            source.id,
            source.url,
            source.revision,
            source.provenance,
            source.credits,
            source.license.name,
            source.license.url,
            source.license.text_path
        ));
    }
    text
}

/// Resolve an explicit Start without changing the current flight. A package Start
/// yields until the one worker has fully inspected the selected identity.
pub(super) fn take_start(
    world: &mut World,
) -> Option<(WorldMapStart, Option<Arc<InstalledPackage>>)> {
    if !world.contains_resource::<RegionRuntime>() {
        return world
            .resource_mut::<WorldMapActions>()
            .start_at
            .take()
            .map(|start| (start, world.resource::<Startup>().active_region.clone()));
    }
    let request = world.resource_mut::<WorldMapActions>().start_at.take();
    let visible = world.resource::<WorldMapState>().visible;
    if !visible || world.contains_resource::<ReplayPlayback>() {
        return None;
    }
    let mut runtime = world
        .remove_resource::<RegionRuntime>()
        .expect("checked region runtime");
    let result = if let Some(request) = request {
        if world.resource::<WorldMapState>().regions.visible {
            None
        } else if runtime.pending.is_some() {
            world.resource_mut::<WorldMapState>().regions.error =
                "Wait for the current region operation or cancel it before starting".into();
            None
        } else if let Some(selected) = &runtime.selected {
            if let Some(summary) = runtime
                .selected_snapshot
                .as_ref()
                .filter(|s| key(s) == *selected)
            {
                let job = Job::Inspect {
                    directory: summary.directory.clone(),
                    expected: summary.identity.clone(),
                    start: request,
                };
                if let Err(error) = runtime.start(job) {
                    world.resource_mut::<WorldMapState>().regions.error = error;
                }
            } else {
                world.resource_mut::<WorldMapState>().regions.error =
                    "Selected region is no longer listed; refresh and select it again".into();
            }
            None
        } else {
            Some((request, None))
        }
    } else {
        runtime
            .ready
            .take()
            .map(|(start, package)| (start, Some(package)))
    };
    world.insert_resource(runtime);
    result
}

pub(super) fn replay_allowed(startup: &Startup) -> bool {
    startup.active_region.is_none()
}

/// Regional source switches create free flight. Airport overlays were authored
/// against the old source, so remove both registered surfaces and free signs.
pub(super) fn clear_airport(world: &mut World) {
    let owned: Vec<_> = world
        .query_filtered::<(Entity, &Mesh3d), With<airport_drape_runtime::AirportGeometry>>()
        .iter(world)
        .map(|(e, m)| (e, m.0.clone()))
        .collect();
    for (entity, mesh) in owned {
        if let Some(mut tiles) = world.get_resource_mut::<TerrainTiles>() {
            tiles.unregister_overlay(entity);
        }
        world.entity_mut(entity).despawn();
        world.resource_mut::<Assets<Mesh>>().remove(&mesh);
    }
    world.remove_resource::<ActiveRunway>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use flightsim_world::TileId;
    use std::sync::atomic::AtomicU64;
    const KEY: &str = "org.example.fixture@1.0.0";
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flightsim-region-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn zip(&self) -> PathBuf {
            let path = self.0.join("fixture.zip");
            std::fs::write(
                &path,
                include_bytes!("../tests/fixtures/region-synthetic.zip"),
            )
            .unwrap();
            path
        }
        fn store(&self) -> PathBuf {
            self.0.join("store")
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn imported(temp: &TestDirectory) -> InstalledPackage {
        flightsim_content::install_zip(&temp.zip(), &temp.store()).unwrap()
    }
    fn start() -> WorldMapStart {
        WorldMapStart {
            position: TileId::new(9, 900, 180).bounds().center(),
            month: 7,
        }
    }
    fn world(temp: &TestDirectory) -> World {
        let startup = Startup {
            regions: Options {
                store: Some(temp.store()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut runtime = RegionRuntime::new(&startup);
        runtime.initialized = true;
        runtime.was_map_visible = true;
        let simulation = Simulation::parked(
            startup.aircraft.configuration(),
            startup.start,
            startup.heading,
            Terrain::new(
                make_source(&startup),
                1024 * 1024,
                world_runtime::terrain_levels(&startup),
            ),
            GroundSampler::default(),
        );
        let mut world = World::new();
        world.insert_resource(runtime);
        world.insert_resource(ActiveRunway(startup.runway));
        world.insert_resource(startup);
        world.insert_resource(FlightSimulation(simulation));
        let mut map = WorldMapState::default();
        map.visible = true;
        map.selected = start().position;
        map.month = 7;
        world.insert_resource(map);
        world.init_resource::<WorldMapActions>();
        world.init_resource::<CameraRig>();
        world.init_resource::<TerrainTiles>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Messages<bevy::window::FileDragAndDrop>>();
        world.insert_resource(TerrainStreaming {
            selector: LodSelector::new(
                16.0,
                720.0,
                Degrees(60.0).to_radians(),
                13,
                Meters(20000.0),
            ),
            source: make_source(world.resource::<Startup>()),
            cache: TileCache::new(1024 * 1024),
            live: Default::default(),
            material: Handle::default(),
        });
        world
    }
    fn await_worker(world: &mut World) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while world.resource::<RegionRuntime>().pending.is_some() {
            world.run_system_once(update).unwrap();
            assert!(
                std::time::Instant::now() < deadline,
                "region worker did not terminate"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn list_select(world: &mut World) {
        world
            .resource_mut::<RegionRuntime>()
            .start(Job::List)
            .unwrap();
        await_worker(world);
        world.resource_mut::<WorldMapActions>().regions.pending =
            Some(RegionAction::Select(Some(KEY.into())));
        world.run_system_once(update).unwrap();
        world.resource_mut::<WorldMapState>().selected = start().position;
    }
    #[test]
    fn import_list_select_new_flight_shares_identity_and_height_and_removes_old_airport() {
        let temp = TestDirectory::new();
        let outcome = run_job(
            Job::Import(temp.zip()),
            &temp.store(),
            &AtomicBool::new(false),
            &Mutex::new(None),
        )
        .unwrap();
        assert!(matches!(outcome, Outcome::Imported(_)));
        let mut world = world(&temp);
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        let airport = world
            .spawn((airport_drape_runtime::AirportGeometry, Mesh3d(mesh.clone())))
            .id();
        let overlay = flightsim_render::terrain_drape::TerrainOverlay::surface(
            world.resource::<Assets<Mesh>>().get(&mesh).unwrap(),
            start().position.to_ecef(),
            |_| Meters::ZERO,
        )
        .unwrap();
        world
            .resource_mut::<TerrainTiles>()
            .register_overlay(airport, mesh.clone(), overlay)
            .unwrap();
        assert_eq!(
            world.resource::<TerrainTiles>().overlay_usage().registered,
            1
        );
        let before = *world.resource::<FlightSimulation>().0.state();
        list_select(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert_eq!(
            world.resource::<WorldMapState>().regions.installed().len(),
            1
        );
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        await_worker(&mut world);
        world_runtime::apply_world_map_start(&mut world);
        let active = world.resource::<Startup>().active_region.as_ref().unwrap();
        assert_eq!(active.identity().id, "org.example.fixture");
        let tile = world
            .resource::<TerrainStreaming>()
            .source
            .load(TileId::new(9, 900, 180))
            .unwrap()
            .unwrap();
        assert!((tile.grid().sample_at(1, 1).get() - 350.0).abs() < 1e-9);
        assert!(
            (world
                .resource::<FlightSimulation>()
                .0
                .state()
                .geodetic()
                .altitude
                .get()
                - 1350.0)
                .abs()
                < 1e-5
        );
        assert!(!world.contains_resource::<ActiveRunway>());
        assert!(world.get_entity(airport).is_err());
        assert_eq!(
            world.resource::<TerrainTiles>().overlay_usage().registered,
            0
        );
        assert!(world.resource::<Assets<Mesh>>().get(&mesh).is_none());
        assert!(!world.resource::<Startup>().airport_enabled);
        assert!(!world.resource::<WorldMapState>().visible);
        assert!(!replay_allowed(world.resource::<Startup>()));
        assert!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(100));
        world.insert_resource(time);
        world.run_system_once(advance_simulation).unwrap();
        assert!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );
        assert!(
            world
                .resource::<TerrainStreaming>()
                .source
                .load_fallback(TileId::new(0, 0, 0))
                .unwrap()
                .is_some()
        );
        // A later baseline choice is an explicit free flight; old airport geometry
        // and runway evaluation are never resurrected against a different source.
        world.resource_mut::<WorldMapState>().visible = true;
        world.resource_mut::<WorldMapActions>().regions.pending = Some(RegionAction::Select(None));
        world.run_system_once(update).unwrap();
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        assert!(world.resource::<Startup>().active_region.is_none());
        assert!(!world.contains_resource::<ActiveRunway>());
        assert!(!world.resource::<Startup>().airport_enabled);
        assert!(replay_allowed(world.resource::<Startup>()));
    }
    #[test]
    fn failed_or_cancelled_import_and_failed_inspection_keep_current_flight() {
        let temp = TestDirectory::new();
        let package = imported(&temp);
        let mut world = world(&temp);
        let before = *world.resource::<FlightSimulation>().0.state();
        world
            .resource_mut::<RegionRuntime>()
            .start(Job::Import(temp.0.join("absent.zip")))
            .unwrap();
        await_worker(&mut world);
        assert!(!world.resource::<WorldMapState>().regions.error.is_empty());
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert!(
            run_job(
                Job::Import(temp.zip()),
                &temp.store(),
                &AtomicBool::new(true),
                &Mutex::new(None)
            )
            .is_err()
        );
        list_select(&mut world);
        std::fs::write(
            package.directory().join("terrain/9/900/180.fsdem"),
            b"corrupt",
        )
        .unwrap();
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        await_worker(&mut world);
        world_runtime::apply_world_map_start(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert!(world.resource::<Startup>().active_region.is_none());
        assert!(world.contains_resource::<ActiveRunway>());
    }
    #[test]
    fn cancellation_retains_one_worker_and_discards_stale_completion() {
        let temp = TestDirectory::new();
        imported(&temp);
        let mut world = world(&temp);
        list_select(&mut world);
        let before = *world.resource::<FlightSimulation>().0.state();
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        let generation = world.resource::<RegionRuntime>().generation;
        world.resource_mut::<RegionRuntime>().cancel();
        assert_ne!(generation, world.resource::<RegionRuntime>().generation);
        assert!(
            world
                .resource_mut::<RegionRuntime>()
                .start(Job::List)
                .is_err()
        );
        await_worker(&mut world);
        world_runtime::apply_world_map_start(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert!(world.resource::<RegionRuntime>().ready.is_none());
    }
    #[test]
    fn cancelled_progress_and_completion_cannot_overwrite_the_cancellation_notice() {
        let temp = TestDirectory::new();
        let mut world = world(&temp);
        let (sender, receiver) = mpsc::sync_channel(1);
        let generation = world.resource::<RegionRuntime>().generation;
        world.resource_mut::<RegionRuntime>().pending = Some(Worker {
            generation,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(Some(ImportProgress {
                phase: flightsim_content::ImportPhase::Validating,
                files_done: 1,
                files_total: 2,
                bytes_done: 512,
                bytes_total: 1024,
            }))),
            receiver: Mutex::new(receiver),
        });
        world.resource_mut::<WorldMapState>().regions.progress = Some(50);
        world.resource_mut::<WorldMapActions>().regions.pending = Some(RegionAction::Cancel);
        world.run_system_once(update).unwrap();
        assert!(
            world
                .resource::<WorldMapState>()
                .regions
                .status
                .starts_with("Cancelled")
        );
        assert!(world.resource::<WorldMapState>().regions.progress.is_none());
        assert!(world.resource::<RegionRuntime>().pending.is_some());
        assert!(sender.send(Ok(Outcome::Listed(Vec::new()))).is_ok());
        world.run_system_once(update).unwrap();
        assert!(
            world
                .resource::<WorldMapState>()
                .regions
                .status
                .starts_with("Cancelled")
        );
        assert!(world.resource::<RegionRuntime>().pending.is_none());
    }

    #[test]
    fn map_dismissal_and_newer_departure_discard_inspected_start() {
        let temp = TestDirectory::new();
        imported(&temp);
        let mut world = world(&temp);
        list_select(&mut world);
        let before = *world.resource::<FlightSimulation>().0.state();
        for close in [true, false, true, false] {
            world.resource_mut::<WorldMapState>().visible = true;
            world.resource_mut::<WorldMapState>().selected = start().position;
            world.run_system_once(update).unwrap();
            world.resource_mut::<WorldMapActions>().start_at = Some(start());
            world_runtime::apply_world_map_start(&mut world);
            if close {
                world.resource_mut::<WorldMapState>().visible = false;
            } else {
                world.resource_mut::<WorldMapState>().month = 8;
            }
            await_worker(&mut world);
            world_runtime::apply_world_map_start(&mut world);
            assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
            assert!(world.resource::<Startup>().active_region.is_none());
            world.resource_mut::<WorldMapState>().month = 7;
        }
    }
    #[test]
    fn cli_import_then_list_exits_without_activating_or_replacing() {
        let temp = TestDirectory::new();
        let options = Options {
            import: Some(temp.zip()),
            store: Some(temp.store()),
            ..Default::default()
        };
        assert!(run_cli(&options).unwrap());
        assert!(
            run_cli(&Options {
                list: true,
                store: Some(temp.store()),
                ..Default::default()
            })
            .unwrap()
        );
        assert_eq!(
            flightsim_content::list_installed(&temp.store())
                .unwrap()
                .len(),
            1
        );
        assert!(run_cli(&options).unwrap_err().contains("already exists"));
        assert!(
            !run_cli(&Options {
                select: Some(KEY.into()),
                store: Some(temp.store()),
                ..Default::default()
            })
            .unwrap()
        );
    }

    #[test]
    fn region_cli_conflicts_and_legacy_raw_tiles_are_explicit() {
        for args in [
            vec!["--region", KEY, "--replay", "flight.fsreplay"],
            vec!["--region", KEY, "--tiles", "tiles"],
            vec!["--region", KEY, "--global-terrain", "off"],
            vec!["--region", "../bad@1.0.0"],
            vec!["--region", KEY, "--import-region", "a.zip"],
            vec!["--region", KEY, "--list-regions"],
            vec!["--region"],
            vec!["--region-store"],
        ] {
            let (startup, _) = parse_arguments_from(args.into_iter().map(str::to_owned));
            assert!(startup.regions.error.is_some());
        }
        let (startup, _) = parse_arguments_from(
            ["--tiles", "legacy", "--replay", "flight.fsreplay"]
                .into_iter()
                .map(str::to_owned),
        );
        assert!(startup.regions.error.is_none());
        assert!(replay_allowed(&startup));
        let (startup, _) = parse_arguments_from(["--region", KEY].into_iter().map(str::to_owned));
        assert!(startup.regions.error.is_none());
        assert!(startup.world.map_open);
        assert!(startup.active_region.is_none());
    }
    #[test]
    fn package_descriptor_blocks_legacy_replay_before_file_read() {
        let temp = TestDirectory::new();
        let mut startup = Startup {
            active_region: Some(Arc::new(imported(&temp))),
            replay: Some(temp.0.join("must-not-read.fsreplay")),
            ..Default::default()
        };
        let mut diagnostics = StartupDiagnostics::default();
        assert!(resolve_replay(&mut startup, &mut diagnostics).is_none());
        assert_eq!(diagnostics.0, vec![REPLAY_NOTICE]);
        assert!(!replay_allowed(&startup));
    }
    #[test]
    fn drops_are_map_only_and_cannot_restart_after_cancel_or_submit_a_stale_start() {
        let temp = TestDirectory::new();
        let mut world = world(&temp);
        let before = *world.resource::<FlightSimulation>().0.state();
        for (visible, cancel, name) in [
            (false, false, "fixture.zip"),
            (true, true, "fixture.zip"),
            (true, false, "raw.tif"),
        ] {
            world.resource_mut::<WorldMapState>().visible = visible;
            world.resource_mut::<WorldMapActions>().regions.pending =
                cancel.then_some(RegionAction::Cancel);
            world.resource_mut::<WorldMapActions>().start_at = Some(start());
            world
                .resource_mut::<Messages<bevy::window::FileDragAndDrop>>()
                .write(bevy::window::FileDragAndDrop::DroppedFile {
                    window: Entity::PLACEHOLDER,
                    path_buf: temp.0.join(name),
                });
            world.run_system_once(update).unwrap();
            world_runtime::apply_world_map_start(&mut world);
            assert!(world.resource::<RegionRuntime>().pending.is_none());
            assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
            world
                .resource_mut::<Messages<bevy::window::FileDragAndDrop>>()
                .clear();
        }
    }

    #[test]
    fn refreshed_metadata_cannot_change_an_existing_pending_identity() {
        let temp = TestDirectory::new();
        let package = imported(&temp);
        let mut world = world(&temp);
        list_select(&mut world);
        let before = *world.resource::<FlightSimulation>().0.state();
        let path = package.directory().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["title"] = serde_json::Value::String("Changed identity".into());
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        world
            .resource_mut::<RegionRuntime>()
            .start(Job::List)
            .unwrap();
        await_worker(&mut world);
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        await_worker(&mut world);
        world_runtime::apply_world_map_start(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert!(
            world
                .resource::<WorldMapState>()
                .regions
                .error
                .contains("metadata changed")
        );
    }

    fn credit_paging_app(temp: &TestDirectory) -> (App, Entity) {
        use flightsim_ui::world_map::{self, RegionsText, WorldMapRaster, WorldMapText};
        let installed = imported(temp);
        let mut manifest = installed.manifest().clone();
        manifest.sources[0].provenance =
            vec!["Synthetic provenance and attribution line"; 40].join("\n");
        std::fs::write(
            installed.directory().join(flightsim_content::MANIFEST_NAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let startup = Startup {
            regions: Options {
                store: Some(temp.store()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut runtime = RegionRuntime::new(&startup);
        runtime.initialized = true;
        runtime.was_map_visible = true;
        runtime.installed = flightsim_content::list_installed(&temp.store()).unwrap();
        let mut map = WorldMapState::default();
        map.show_regions();
        select(&mut runtime, &mut map, Some(KEY.into()));
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(startup)
            .insert_resource(runtime)
            .insert_resource(map)
            .init_resource::<WorldMapActions>()
            .init_resource::<WorldMapRaster>()
            .init_resource::<Assets<Image>>()
            .add_message::<bevy::window::FileDragAndDrop>()
            .add_systems(
                Update,
                (
                    world_map::handle_world_map_input,
                    update,
                    world_map::update_world_map,
                )
                    .chain(),
            );
        let page = app
            .world_mut()
            .spawn((
                WorldMapText::Regions(RegionsText::CreditsPage),
                Text::new(""),
            ))
            .id();
        app.update();
        assert!(
            app.world().get::<Text>(page).unwrap().contains(" / 4"),
            "fixture must have four pages: {}",
            app.world().get::<Text>(page).unwrap().as_str()
        );
        (app, page)
    }

    fn navigation_event(app: &mut App, physical: KeyCode, logical: bevy::input::keyboard::Key) {
        use bevy::input::{ButtonState, keyboard::KeyboardInput};
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut()
                .write_message(KeyboardInput {
                    key_code: physical,
                    logical_key: logical.clone(),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                })
                .unwrap();
            app.update();
        }
    }

    #[test]
    fn physical_credit_navigation_survives_app_sync_and_idle_frames() {
        let temp = TestDirectory::new();
        let (mut app, page) = credit_paging_app(&temp);
        assert!(
            app.world()
                .get::<Text>(page)
                .unwrap()
                .starts_with("Page 1 /")
        );
        navigation_event(
            &mut app,
            KeyCode::ArrowRight,
            bevy::input::keyboard::Key::ArrowRight,
        );
        for _ in 0..90 {
            app.update();
            assert!(
                app.world()
                    .get::<Text>(page)
                    .unwrap()
                    .starts_with("Page 2 /")
            );
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
            assert_eq!(
                app.world().resource::<RegionRuntime>().selected.as_deref(),
                Some(KEY)
            );
            assert!(app.world().resource::<Startup>().active_region.is_none());
        }
        navigation_event(
            &mut app,
            KeyCode::ArrowLeft,
            bevy::input::keyboard::Key::ArrowLeft,
        );
        assert!(
            app.world()
                .get::<Text>(page)
                .unwrap()
                .starts_with("Page 1 /")
        );
    }

    #[test]
    fn logical_credit_navigation_with_keypad_physical_code_survives_app_sync() {
        let temp = TestDirectory::new();
        let (mut app, page) = credit_paging_app(&temp);
        // NumLock-off keypad/remapped navigation supplies semantic arrows even
        // when its physical switch is not in the dedicated arrow-key cluster.
        navigation_event(
            &mut app,
            KeyCode::Numpad6,
            bevy::input::keyboard::Key::ArrowRight,
        );
        for _ in 0..90 {
            app.update();
            assert!(
                app.world()
                    .get::<Text>(page)
                    .unwrap()
                    .starts_with("Page 2 /")
            );
        }
        navigation_event(
            &mut app,
            KeyCode::Numpad4,
            bevy::input::keyboard::Key::ArrowLeft,
        );
        assert!(
            app.world()
                .get::<Text>(page)
                .unwrap()
                .starts_with("Page 1 /")
        );
    }

    #[test]
    fn semantic_navigation_blocks_modifiers_repeats_and_numeric_keypad_selection() {
        use bevy::input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        };
        use flightsim_ui::world_map::{RegionSummary, RegionsText, WorldMapText};
        let temp = TestDirectory::new();
        let (mut app, credits_page) = credit_paging_app(&temp);
        let send = |app: &mut App, physical, logical, state, repeat| {
            app.world_mut()
                .write_message(KeyboardInput {
                    key_code: physical,
                    logical_key: logical,
                    state,
                    repeat,
                    text: None,
                    window: Entity::PLACEHOLDER,
                })
                .unwrap();
        };
        // Two representations and duplicate messages from one press are still
        // one action, not two pages. Repeats and release do not advance again.
        for _ in 0..2 {
            send(
                &mut app,
                KeyCode::ArrowRight,
                Key::ArrowRight,
                ButtonState::Pressed,
                false,
            );
        }
        app.update();
        for _ in 0..3 {
            assert!(
                app.world()
                    .get::<Text>(credits_page)
                    .unwrap()
                    .starts_with("Page 2 /")
            );
            send(
                &mut app,
                KeyCode::ArrowRight,
                Key::ArrowRight,
                ButtonState::Pressed,
                true,
            );
            app.update();
        }
        send(
            &mut app,
            KeyCode::ArrowRight,
            Key::ArrowRight,
            ButtonState::Released,
            false,
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(credits_page)
                .unwrap()
                .starts_with("Page 2 /")
        );
        for (physical, logical) in [
            (KeyCode::ControlLeft, Key::Control),
            (KeyCode::AltRight, Key::Alt),
            (KeyCode::SuperLeft, Key::Super),
        ] {
            send(
                &mut app,
                physical,
                logical.clone(),
                ButtonState::Pressed,
                false,
            );
            navigation_event(&mut app, KeyCode::Numpad6, Key::ArrowRight);
            assert!(
                app.world()
                    .get::<Text>(credits_page)
                    .unwrap()
                    .starts_with("Page 2 /")
            );
            send(&mut app, physical, logical, ButtonState::Released, false);
            app.update();
        }
        navigation_event(&mut app, KeyCode::Numpad4, Key::ArrowLeft);
        send(
            &mut app,
            KeyCode::Numpad6,
            Key::ArrowRight,
            ButtonState::Pressed,
            false,
        );
        app.update();
        for _ in 0..3 {
            send(
                &mut app,
                KeyCode::Numpad6,
                Key::ArrowRight,
                ButtonState::Pressed,
                true,
            );
            app.update();
            assert!(
                app.world()
                    .get::<Text>(credits_page)
                    .unwrap()
                    .starts_with("Page 2 /")
            );
        }
        send(
            &mut app,
            KeyCode::Numpad6,
            Key::ArrowRight,
            ButtonState::Released,
            false,
        );
        app.update();
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .regions
            .set_installed((0..7).map(|index| RegionSummary {
                key: format!("row-{index}@1.0.0"),
                name: format!("Row {index}"),
            }));
        let list_page = app
            .world_mut()
            .spawn((WorldMapText::Regions(RegionsText::Page), Text::new("")))
            .id();
        navigation_event(&mut app, KeyCode::Numpad3, Key::PageDown);
        assert!(
            app.world()
                .get::<Text>(list_page)
                .unwrap()
                .starts_with("2 / 2")
        );
        navigation_event(&mut app, KeyCode::Numpad9, Key::PageUp);
        assert!(
            app.world()
                .get::<Text>(list_page)
                .unwrap()
                .starts_with("1 / 2")
        );
        navigation_event(&mut app, KeyCode::Numpad1, Key::Character("1".into()));
        assert_eq!(
            app.world().resource::<RegionRuntime>().selected.as_deref(),
            Some(KEY)
        );
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .regions
                .error
                .is_empty()
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        assert!(app.world().resource::<Startup>().active_region.is_none());
    }

    #[test]
    fn store_defaults_to_absolute_user_data_and_override_is_explicit() {
        // PathBuf validates absolute paths using the host platform's syntax.
        // A POSIX-looking fixture has no drive prefix on Windows.
        let user_data = TestDirectory::new();
        let root = user_data.0.canonicalize().unwrap();
        let path = default_store_from("linux", |name| (name == "HOME").then(|| root.to_path_buf()))
            .unwrap();
        assert_eq!(path, root.join(".local/share/flightsim-claude/regions"));
        for (os, variable, suffix) in [
            ("windows", "LOCALAPPDATA", "flightsim-claude/regions"),
            (
                "macos",
                "HOME",
                "Library/Application Support/flightsim-claude/regions",
            ),
            ("linux", "XDG_DATA_HOME", "flightsim-claude/regions"),
        ] {
            let selected =
                default_store_from(os, |name| (name == variable).then(|| root.to_path_buf()))
                    .unwrap();
            assert_eq!(selected, root.join(suffix));
            assert!(default_store_from(os, |_| None).is_err());
            assert!(default_store_from(os, |_| Some(PathBuf::from("relative"))).is_err());
        }
        assert_eq!(
            store(&Options {
                store: Some(PathBuf::from("explicit")),
                ..Default::default()
            })
            .unwrap(),
            PathBuf::from("explicit")
        );
    }
}
