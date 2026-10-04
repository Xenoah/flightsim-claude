//! Prepared terrain imports and opt-in downloads. One worker; explicit new-flight activation.
#[cfg(feature = "region-downloads")]
#[path = "region_downloads.rs"]
mod downloads;
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

pub(super) const REPLAY_NOTICE: &str = "Regional terrain: replay recording/export and playback are unavailable (replay formats do not identify packages).";

#[derive(Debug, Clone, Default)]
pub(super) struct Options {
    pub import: Option<PathBuf>,
    pub select: Option<String>,
    pub store: Option<PathBuf>,
    pub list: bool,
    pub catalog: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub offline: bool,
    pub error: Option<String>,
}

pub(super) fn validate_options(startup: &mut Startup) {
    let options = &startup.regions;
    let modes = usize::from(options.import.is_some())
        + usize::from(options.select.is_some())
        + usize::from(options.list);
    let download_options = options.catalog.is_some() || options.cache.is_some() || options.offline;
    let error = if download_options && !cfg!(feature = "region-downloads") {
        Some("region downloads require a build with --features region-downloads")
    } else if options.catalog.is_none() && (options.cache.is_some() || options.offline) {
        Some("--region-cache and --region-offline require --region-catalog FILE.json")
    } else if options
        .catalog
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
        || options
            .cache
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
    {
        Some("--region-catalog and --region-cache need nonempty paths")
    } else if options.catalog.is_some() && (options.import.is_some() || options.list) {
        Some("--region-catalog cannot be combined with --import-region or --list-regions")
    } else if options.catalog.is_some() && startup.replay.is_some() {
        Some("region downloads cannot be combined with --replay")
    } else if options.catalog.is_some() && !startup.world.global_terrain {
        Some("--region-catalog requires --global-terrain on for fallback")
    } else if modes > 1 {
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
            "region operations cannot be combined with --replay; package-backed replay is unsupported",
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
    if startup.regions.select.is_some() || startup.regions.catalog.is_some() {
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
    #[cfg(feature = "region-downloads")]
    downloads: Option<downloads::DownloadRuntime>,
}
struct Worker {
    generation: u64,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Option<Progress>>>,
    receiver: Mutex<Receiver<Result<Outcome, String>>>,
}
#[derive(Clone, Copy)]
enum Progress {
    Import(ImportProgress),
    #[cfg(feature = "region-downloads")]
    Download(flightsim_content::download::DownloadProgress),
}
enum Job {
    List,
    #[cfg(feature = "region-downloads")]
    RefreshCatalog(PathBuf),
    #[cfg(feature = "region-downloads")]
    Download {
        entry: downloads::Entry,
        cache: PathBuf,
        mode: flightsim_content::download::CacheMode,
    },
    Import(PathBuf),
    Inspect {
        directory: PathBuf,
        expected: flightsim_content::RegionalIdentity,
        start: WorldMapStart,
    },
}
enum Outcome {
    Listed(Vec<InstalledSummary>),
    #[cfg(feature = "region-downloads")]
    RefreshedCatalog {
        installed: Result<Vec<InstalledSummary>, String>,
        catalog: Result<Vec<downloads::Entry>, String>,
    },
    #[cfg(feature = "region-downloads")]
    Downloaded {
        installed: Vec<InstalledSummary>,
        key: String,
        cache_hit: bool,
    },
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
            #[cfg(feature = "region-downloads")]
            downloads: downloads::DownloadRuntime::new(&startup.regions),
        }
    }
    fn refresh_job(&self) -> Job {
        #[cfg(feature = "region-downloads")]
        if let Some(downloads) = &self.downloads {
            return Job::RefreshCatalog(downloads.path.clone());
        }
        Job::List
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
    progress: &Mutex<Option<Progress>>,
) -> Result<Outcome, String> {
    let report = |next| {
        if let Ok(mut progress) = progress.lock() {
            *progress = Some(Progress::Import(next));
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
        #[cfg(feature = "region-downloads")]
        Job::RefreshCatalog(path) => Outcome::RefreshedCatalog {
            // Refresh the catalog even if the independent installed-store listing
            // fails, so changed or invalid claims cannot leave a stale preview.
            installed: flightsim_content::list_installed(store).map_err(|e| e.to_string()),
            catalog: downloads::read_catalog(&path),
        },
        #[cfg(feature = "region-downloads")]
        Job::Download { entry, cache, mode } => {
            downloads::download(&entry, &cache, store, mode, cancel, progress)?
        }
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
pub(super) fn update(
    mut runtime: ResMut<RegionRuntime>,
    startup: Res<Startup>,
    playback: Option<Res<ReplayPlayback>>,
    simulation: Option<Res<FlightSimulation>>,
    mut map: ResMut<WorldMapState>,
    mut actions: ResMut<WorldMapActions>,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
) {
    let replay = playback.is_some()
        || startup.replay.is_some()
        || simulation.is_some_and(|simulation| simulation.0.is_replay());
    let enabled = !replay && startup.world.global_terrain && runtime.store.is_ok();
    map.regions.operations_enabled = enabled;
    #[cfg(feature = "region-downloads")]
    {
        map.regions.downloads_enabled = runtime.downloads.is_some();
    }
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
        if runtime.initial_selection.is_some() || startup.regions.catalog.is_some() {
            map.show_regions();
        }
        let job = runtime.refresh_job();
        if let Err(error) = runtime.start(job) {
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
                map.regions.status = "Cancelled; current flight unchanged. Refresh installed packages if an install already committed".into();
            }
            RegionAction::Refresh if map.visible && enabled => {
                map.regions.error.clear();
                let job = runtime.refresh_job();
                if let Err(error) = runtime.start(job) {
                    map.regions.error = error;
                }
            }
            #[cfg(feature = "region-downloads")]
            RegionAction::SelectDownload(key)
                if map.visible && enabled && runtime.pending.is_none() =>
            {
                runtime.ready = None;
                actions.start_at = None;
                map.regions.error.clear();
                if let Some(downloads) = &mut runtime.downloads
                    && let Err(error) = downloads.select(&key, &mut map)
                {
                    map.regions.error = error;
                }
            }
            #[cfg(feature = "region-downloads")]
            RegionAction::Download { offline }
                if map.visible && enabled && runtime.pending.is_none() =>
            {
                runtime.ready = None;
                actions.start_at = None;
                map.regions.error.clear();
                let job = runtime
                    .downloads
                    .as_ref()
                    .ok_or_else(|| "Region downloads are not configured".to_owned())
                    .and_then(|downloads| downloads.job(offline));
                if let Err(error) = job.and_then(|job| runtime.start(job)) {
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
            let (percentage, status) = match p {
                Progress::Import(p) => import_progress_text(p),
                #[cfg(feature = "region-downloads")]
                Progress::Download(p) => downloads::progress_text(p),
            };
            map.regions.progress = percentage;
            map.regions.status = status;
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
                    apply_installed_list(&mut runtime, &mut map, installed);
                }
                #[cfg(feature = "region-downloads")]
                Ok(Outcome::RefreshedCatalog { installed, catalog }) => {
                    let mut errors = Vec::new();
                    match installed {
                        Ok(installed) => apply_installed_list(&mut runtime, &mut map, installed),
                        Err(error) => errors.push(error),
                    }
                    if let Some(downloads) = &mut runtime.downloads {
                        match catalog {
                            Ok(entries) => downloads.set_entries(entries, &mut map),
                            Err(error) => {
                                downloads.set_entries(Vec::new(), &mut map);
                                errors.push(error);
                            }
                        }
                    }
                    if !errors.is_empty() {
                        map.regions.error = errors.join("; ");
                        map.regions.status = "Refresh incomplete; current flight unchanged".into();
                    }
                }
                #[cfg(feature = "region-downloads")]
                Ok(Outcome::Downloaded {
                    installed,
                    key,
                    cache_hit,
                }) => {
                    // Installation is never a next-flight selection or activation.
                    runtime.installed = installed;
                    sync_installed_list(&runtime, &mut map);
                    map.regions.show_installed();
                    map.regions.status = format!(
                        "Installed {key} ({}). Select it in Installed, then Start new flight",
                        if cache_hit {
                            "verified cache"
                        } else {
                            "downloaded"
                        }
                    );
                }
                Ok(Outcome::Inspected(start, package)) => {
                    if map.selected == start.position
                        && map.preview_month() == start.month
                        && map.aircraft_choice == start.aircraft_choice
                        && actions.generation == start.generation
                    {
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

fn percentage(done: u64, total: u64) -> Option<u8> {
    (total > 0)
        .then(|| u8::try_from((u128::from(done) * 100 / u128::from(total)).min(100)).unwrap_or(100))
}

fn import_progress_text(p: ImportProgress) -> (Option<u8>, String) {
    (
        percentage(p.bytes_done, p.bytes_total),
        format!(
            "{:?}: {}/{} files, {} / {} bytes",
            p.phase, p.files_done, p.files_total, p.bytes_done, p.bytes_total
        ),
    )
}

fn sync_installed_list(runtime: &RegionRuntime, map: &mut WorldMapState) {
    map.regions
        .set_installed(runtime.installed.iter().map(|s| RegionSummary {
            key: key(s),
            name: s.manifest.title.clone(),
        }));
}

fn apply_installed_list(
    runtime: &mut RegionRuntime,
    map: &mut WorldMapState,
    installed: Vec<InstalledSummary>,
) {
    runtime.installed = installed;
    sync_installed_list(runtime, map);
    map.regions.status = format!(
        "{} installed versions. Select terrain, then return to the map and Start new flight",
        runtime.installed.len()
    );
    if let Some(selected) = runtime.initial_selection.take() {
        select(runtime, map, Some(selected));
    }
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

/// Inspect every source-selection stage without consuming a Start or changing
/// a worker's generation, cancellation state or selected package.
pub(super) fn bounded_start_error(world: &World) -> Option<&'static str> {
    let startup = world.resource::<Startup>();
    if startup.tiles.is_some() {
        return Some("Aircraft requires global or flat terrain\nRemove --tiles before starting");
    }
    let regional = startup.active_region.is_some()
        || startup.regions.select.is_some()
        || world.resource::<WorldMapState>().regions.selected.is_some()
        || world
            .get_resource::<RegionRuntime>()
            .is_some_and(|runtime| {
                runtime.selected.is_some()
                    || runtime.initial_selection.is_some()
                    || runtime.pending.is_some()
                    || runtime.ready.is_some()
            });
    regional.then_some("Aircraft: regional terrain unsupported\nFlight and region are unchanged")
}

/// Compatibility name for existing jet callers; policy is shared by both
/// explicitly bounded physical families.
pub(super) fn jet_start_error(world: &World) -> Option<&'static str> {
    bounded_start_error(world)
}

/// Resolve an explicit Start without changing the current flight. A package Start
/// yields until the one worker has fully inspected the selected identity.
pub(super) fn take_start(
    world: &mut World,
) -> Option<(WorldMapStart, Option<Arc<InstalledPackage>>)> {
    let jet = world.resource::<Startup>().aircraft.uses_bounded_model();
    take_start_for_target(world, jet)
}

pub(super) fn start_is_pending(world: &World) -> bool {
    world
        .get_resource::<RegionRuntime>()
        .is_some_and(|runtime| runtime.pending.is_some())
}

/// Dispatch against the snapshotted target, never the previous active family.
pub(super) fn take_start_for_target(
    world: &mut World,
    target_uses_bounded_model: bool,
) -> Option<(WorldMapStart, Option<Arc<InstalledPackage>>)> {
    if target_uses_bounded_model && let Some(error) = jet_start_error(world) {
        world.resource_mut::<WorldMapState>().regions.error = error.into();
        return None;
    }
    if !world.contains_resource::<RegionRuntime>() {
        return world
            .resource_mut::<WorldMapActions>()
            .start_at
            .take()
            .map(|start| (start, world.resource::<Startup>().active_region.clone()));
    }
    let request = world.resource_mut::<WorldMapActions>().start_at.take();
    let visible = world.resource::<WorldMapState>().visible;
    if !visible
        || world.contains_resource::<ReplayPlayback>()
        || world.resource::<Startup>().replay.is_some()
        || world
            .get_resource::<FlightSimulation>()
            .is_some_and(|simulation| simulation.0.is_replay())
    {
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
    pub(super) struct TestDirectory(pub(super) PathBuf);
    impl TestDirectory {
        pub(super) fn new() -> Self {
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
        pub(super) fn store(&self) -> PathBuf {
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
            aircraft_choice: 0,
            generation: 0,
            position: TileId::new(9, 900, 180).bounds().center(),
            month: 7,
        }
    }
    pub(super) fn world(temp: &TestDirectory) -> World {
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
        world.insert_resource(FlightSimulation(simulation.into()));
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

    #[test]
    fn rejected_jet_map_start_preserves_all_regional_selection_stages() {
        let temp = TestDirectory::new();
        let package = Arc::new(imported(&temp));
        for picker_target in [false, true] {
            if picker_target && cfg!(feature = "commercial-staging") {
                continue;
            }
            for stage in 0..7 {
                let mut world = world(&temp);
                let profile = flightsim_sim::aircraft_profile::AircraftProfileV2::parse(
                    include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"),
                )
                .unwrap();
                let request = if picker_target {
                    world.resource_mut::<Startup>().assets =
                        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
                    crate::aircraft_picker_runtime::initialize(&mut world);
                    world.resource_mut::<WorldMapState>().aircraft_choice = 3;
                    WorldMapStart {
                        aircraft_choice: 3,
                        ..start()
                    }
                } else {
                    world.resource_mut::<Startup>().aircraft =
                        aircraft_profile::SelectedAircraftProfile::Jet(profile);
                    start()
                };
                let (sender, receiver) = mpsc::sync_channel(1);
                let cancel = Arc::new(AtomicBool::new(false));
                match stage {
                    0 => world.resource_mut::<Startup>().tiles = Some(temp.0.join("raw-tiles")),
                    1 => world.resource_mut::<Startup>().active_region = Some(Arc::clone(&package)),
                    2 => world.resource_mut::<RegionRuntime>().selected = Some(KEY.into()),
                    3 => {
                        world.resource_mut::<RegionRuntime>().pending = Some(Worker {
                            generation: 0,
                            cancel: Arc::clone(&cancel),
                            progress: Arc::new(Mutex::new(None)),
                            receiver: Mutex::new(receiver),
                        });
                    }
                    4 => {
                        world.resource_mut::<RegionRuntime>().ready =
                            Some((start(), Arc::clone(&package)));
                    }
                    5 => world.resource_mut::<RegionRuntime>().initial_selection = Some(KEY.into()),
                    6 => world.resource_mut::<WorldMapState>().regions.selected = Some(KEY.into()),
                    _ => unreachable!(),
                }
                let before = *world.resource::<FlightSimulation>().0.state();
                let startup = world.resource::<Startup>().clone();
                let runtime = world.resource::<RegionRuntime>();
                let generation = runtime.generation;
                let selected = runtime.selected.clone();
                let initial = runtime.initial_selection.clone();
                let was_pending = runtime.pending.is_some();
                let was_ready = runtime.ready.is_some();
                world.resource_mut::<WorldMapActions>().start_at = Some(request);
                world_runtime::apply_world_map_start(&mut world);
                assert_eq!(
                    world.resource::<WorldMapActions>().start_at,
                    Some(request),
                    "stage {stage}, picker target {picker_target}"
                );
                assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
                assert_eq!(world.resource::<Startup>().start, startup.start);
                assert_eq!(world.resource::<Startup>().tiles, startup.tiles);
                assert_eq!(
                    world.resource::<Startup>().weather.selection,
                    startup.weather.selection
                );
                assert_eq!(
                    world
                        .resource::<Startup>()
                        .active_region
                        .as_ref()
                        .map(|p| p.identity()),
                    startup.active_region.as_ref().map(|p| p.identity())
                );
                let runtime = world.resource::<RegionRuntime>();
                assert_eq!(runtime.generation, generation);
                assert_eq!(runtime.selected, selected);
                assert_eq!(runtime.initial_selection, initial);
                assert_eq!(runtime.pending.is_some(), was_pending);
                assert_eq!(runtime.ready.is_some(), was_ready);
                assert!(!cancel.load(Ordering::Relaxed));
                let map = world.resource::<WorldMapState>();
                assert!(map.visible);
                assert!(map.navigation_note.starts_with("Aircraft"));
                if stage != 0 {
                    let rendered = flightsim_ui::world_map::format_world_map_text(
                        flightsim_ui::world_map::WorldMapText::Navigation,
                        map,
                        &flightsim_ui::WorldMapRaster::default(),
                    );
                    assert_eq!(rendered, map.navigation_note);
                    assert!(rendered.contains("regional terrain unsupported"));
                    assert!(rendered.contains("Flight and region are unchanged"));
                }
                // Direct consumers share the guard and also retain the request.
                assert!(take_start_for_target(&mut world, true).is_none());
                assert_eq!(world.resource::<WorldMapActions>().start_at, Some(request));
                drop(sender);
            }
        }
    }
    #[test]
    fn rejected_turboprop_map_start_preserves_all_regional_selection_stages() {
        let temp = TestDirectory::new();
        let package = Arc::new(imported(&temp));
        for picker_target in [false, true] {
            if picker_target && cfg!(feature = "commercial-staging") {
                continue;
            }
            for stage in 0..7 {
                let mut world = world(&temp);
                let profile =
                    flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(include_str!(
                        "../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
                    ))
                    .unwrap();
                world.resource_mut::<Startup>().aircraft =
                    aircraft_profile::SelectedAircraftProfile::Turboprop(profile);
                world.resource_mut::<Startup>().aircraft_choice = Some("controlled-v3.json".into());
                world.resource_mut::<Startup>().model = None;
                let request = start();
                if picker_target {
                    world.resource_mut::<Startup>().assets =
                        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
                    crate::aircraft_picker_runtime::initialize(&mut world);
                    // The snapshotted launch target stays turboprop even if the
                    // current flight is a legacy aircraft.
                    world.resource_mut::<Startup>().aircraft =
                        aircraft_profile::SelectedAircraftProfile::builtin("swift-sport").unwrap();
                }
                let (sender, receiver) = mpsc::sync_channel(1);
                let cancel = Arc::new(AtomicBool::new(false));
                match stage {
                    0 => world.resource_mut::<Startup>().tiles = Some(temp.0.join("raw-tiles")),
                    1 => world.resource_mut::<Startup>().active_region = Some(Arc::clone(&package)),
                    2 => world.resource_mut::<RegionRuntime>().selected = Some(KEY.into()),
                    3 => {
                        world.resource_mut::<RegionRuntime>().pending = Some(Worker {
                            generation: 0,
                            cancel: Arc::clone(&cancel),
                            progress: Arc::new(Mutex::new(None)),
                            receiver: Mutex::new(receiver),
                        });
                    }
                    4 => {
                        world.resource_mut::<RegionRuntime>().ready =
                            Some((start(), Arc::clone(&package)));
                    }
                    5 => world.resource_mut::<RegionRuntime>().initial_selection = Some(KEY.into()),
                    6 => world.resource_mut::<WorldMapState>().regions.selected = Some(KEY.into()),
                    _ => unreachable!(),
                }
                let mut active_source = world.resource::<Startup>().clone();
                active_source.aircraft =
                    aircraft_profile::SelectedAircraftProfile::builtin("swift-sport").unwrap();
                active_source.active_region = Some(Arc::clone(&package));
                active_source.tiles = None;
                let active = Simulation::parked(
                    active_source.aircraft.configuration(),
                    start().position,
                    active_source.heading,
                    Terrain::new(make_source(&active_source), 1024 * 1024, 9..=9),
                    GroundSampler::default(),
                );
                world.resource_mut::<FlightSimulation>().0 = active.into();
                world.resource_mut::<TerrainStreaming>().source = make_source(&active_source);
                let source_height = |world: &World| {
                    let tile = world
                        .resource::<FlightSimulation>()
                        .0
                        .legacy()
                        .unwrap()
                        .terrain()
                        .source()
                        .load(TileId::new(9, 900, 180))
                        .unwrap()
                        .unwrap();
                    tile.grid().sample_at(1, 1).get().to_bits()
                };
                let physical_height = source_height(&world);
                assert_eq!(physical_height, 350.0_f64.to_bits());
                let rendered_height = world
                    .resource::<TerrainStreaming>()
                    .source
                    .load(TileId::new(9, 900, 180))
                    .unwrap()
                    .unwrap()
                    .grid()
                    .sample_at(1, 1)
                    .get()
                    .to_bits();
                let before = *world.resource::<FlightSimulation>().0.state();
                let startup = world.resource::<Startup>().clone();
                let runtime = world.resource::<RegionRuntime>();
                let generation = runtime.generation;
                let selected = runtime.selected.clone();
                let initial = runtime.initial_selection.clone();
                let was_pending = runtime.pending.is_some();
                let was_ready = runtime.ready.is_some();
                world.resource_mut::<WorldMapActions>().start_at = Some(request);
                world_runtime::apply_world_map_start(&mut world);
                assert_eq!(
                    world.resource::<WorldMapActions>().start_at,
                    Some(request),
                    "stage {stage}, picker target {picker_target}"
                );
                assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
                assert_eq!(source_height(&world), physical_height);
                assert_eq!(
                    world
                        .resource::<TerrainStreaming>()
                        .source
                        .load(TileId::new(9, 900, 180))
                        .unwrap()
                        .unwrap()
                        .grid()
                        .sample_at(1, 1)
                        .get()
                        .to_bits(),
                    rendered_height
                );

                assert_eq!(world.resource::<Startup>().start, startup.start);
                assert_eq!(world.resource::<Startup>().tiles, startup.tiles);
                assert_eq!(
                    world.resource::<Startup>().weather.selection,
                    startup.weather.selection
                );
                assert_eq!(
                    world
                        .resource::<Startup>()
                        .active_region
                        .as_ref()
                        .map(|p| p.identity()),
                    startup.active_region.as_ref().map(|p| p.identity())
                );
                let runtime = world.resource::<RegionRuntime>();
                assert_eq!(runtime.generation, generation);
                assert_eq!(runtime.selected, selected);
                assert_eq!(runtime.initial_selection, initial);
                assert_eq!(runtime.pending.is_some(), was_pending);
                assert_eq!(runtime.ready.is_some(), was_ready);
                assert!(!cancel.load(Ordering::Relaxed));
                let map = world.resource::<WorldMapState>();
                assert!(map.visible);
                assert!(map.navigation_note.starts_with("Aircraft"));
                if stage != 0 {
                    let rendered = flightsim_ui::world_map::format_world_map_text(
                        flightsim_ui::world_map::WorldMapText::Navigation,
                        map,
                        &flightsim_ui::WorldMapRaster::default(),
                    );
                    assert_eq!(rendered, map.navigation_note);
                    assert!(rendered.contains("regional terrain unsupported"));
                    assert!(rendered.contains("Flight and region are unchanged"));
                }
                // Direct consumers share the guard and also retain the request.
                assert!(take_start_for_target(&mut world, true).is_none());
                assert_eq!(world.resource::<WorldMapActions>().start_at, Some(request));
                drop(sender);
            }
        }
    }
    #[test]
    fn turboprop_direct_preparation_cannot_erase_active_or_supplied_region() {
        let temp = TestDirectory::new();
        let package = Arc::new(imported(&temp));
        let mut startup = world(&temp).resource::<Startup>().clone();
        startup.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
            flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(include_str!(
                "../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
            ))
            .unwrap(),
        );
        startup.active_region = Some(Arc::clone(&package));
        assert!(world_runtime::prepare_world_map_flight(startup.clone(), start(), None).is_err());
        assert_eq!(
            startup.active_region.as_ref().unwrap().identity(),
            package.identity()
        );
        startup.active_region = None;
        assert!(world_runtime::prepare_world_map_flight(startup, start(), Some(package)).is_err());
    }

    #[test]
    fn late_region_inspection_cannot_activate_a_different_aircraft_or_request_generation() {
        let temp = TestDirectory::new();
        let package = Arc::new(imported(&temp));
        for changed in 0..3 {
            let mut world = world(&temp);
            let (sender, receiver) = mpsc::sync_channel(1);
            world.resource_mut::<RegionRuntime>().pending = Some(Worker {
                generation: 0,
                cancel: Arc::new(AtomicBool::new(false)),
                progress: Arc::new(Mutex::new(None)),
                receiver: Mutex::new(receiver),
            });
            if changed == 0 {
                world.resource_mut::<WorldMapActions>().generation = 1;
            }
            if changed == 1 {
                world.resource_mut::<WorldMapState>().aircraft_choice = 1;
            }
            if changed == 2 {
                world.resource_mut::<WorldMapState>().month = 1;
            }
            sender
                .send(Ok(Outcome::Inspected(start(), Arc::clone(&package))))
                .unwrap();
            world.run_system_once(update).unwrap();
            assert!(world.resource::<RegionRuntime>().ready.is_none());
            assert!(world.resource::<Startup>().active_region.is_none());
        }
    }

    pub(super) fn await_worker(world: &mut World) {
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
        use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
        let preset = WeatherPreset::Storm;
        let seed = 0xfedc_ba98_7654_3210;
        let mut startup = world.remove_resource::<Startup>().unwrap();
        startup.weather.requested = Some(preset);
        startup.weather.seed = seed;
        weather_runtime::resolve_departure(&mut startup).unwrap();
        let initial_weather = startup.weather.selection;
        let initial_conditions =
            recording_conditions(&startup, &world_runtime::startup_clock(&startup));
        world.insert_resource(startup);
        world.insert_resource(FlightRecorder(flightsim_sim::CurrentRecorder::new(
            initial_conditions.clone(),
        )));
        world.init_resource::<weather_runtime::PendingWeather>();
        world
            .resource_mut::<weather_runtime::PendingWeather>()
            .requested = Some(preset);
        let before = *world.resource::<FlightSimulation>().0.state();
        list_select(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert_eq!(
            world.resource::<Startup>().weather.selection,
            initial_weather
        );
        assert_eq!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .conditions(),
            &initial_conditions
        );
        world.resource_mut::<WorldMapActions>().regions.pending = Some(RegionAction::Cancel);
        world.run_system_once(update).unwrap();
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
        assert_eq!(
            world.resource::<Startup>().weather.selection,
            initial_weather
        );
        assert_eq!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .conditions(),
            &initial_conditions
        );
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
        let package_reference = Geodetic::new(
            start().position.latitude,
            start().position.longitude,
            Meters(350.0),
        );
        let package_weather = WeatherSelection::Modeled(
            WeatherScenario::from_preset(preset, package_reference, seed).unwrap(),
        );
        assert_eq!(
            world.resource::<Startup>().weather.selection,
            package_weather
        );
        assert_eq!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .conditions()
                .weather,
            package_weather
        );
        // F9 follows the same package block even though the exact initial weather
        // and complete aircraft identity remain available in CurrentRecorder.
        world.init_resource::<ButtonInput<KeyCode>>();
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F9);
        world.run_system_once(control_replay).unwrap();
        world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
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
        let package_state = *world.resource::<FlightSimulation>().0.state();
        let package_conditions = world
            .resource::<FlightRecorder>()
            .0
            .recording()
            .conditions()
            .clone();
        world.resource_mut::<WorldMapActions>().regions.pending = Some(RegionAction::Select(None));
        world.run_system_once(update).unwrap();
        world.resource_mut::<WorldMapActions>().regions.pending = Some(RegionAction::Cancel);
        world.run_system_once(update).unwrap();
        assert_eq!(
            *world.resource::<FlightSimulation>().0.state(),
            package_state
        );
        assert_eq!(
            world.resource::<Startup>().weather.selection,
            package_weather
        );
        assert_eq!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .conditions(),
            &package_conditions
        );
        assert!(!replay_allowed(world.resource::<Startup>()));
        let mut baseline = world.resource::<Startup>().clone();
        baseline.active_region = None;
        baseline.start = start().position;
        weather_runtime::resolve_departure(&mut baseline).unwrap();
        let baseline_weather = baseline.weather.selection;
        assert_ne!(baseline_weather, package_weather);
        world.resource_mut::<WorldMapActions>().start_at = Some(start());
        world_runtime::apply_world_map_start(&mut world);
        assert!(world.resource::<Startup>().active_region.is_none());
        assert!(!world.contains_resource::<ActiveRunway>());
        assert!(!world.resource::<Startup>().airport_enabled);
        assert!(replay_allowed(world.resource::<Startup>()));
        assert_eq!(
            world.resource::<Startup>().weather.selection,
            baseline_weather
        );
        assert_eq!(world.resource::<Startup>().weather.requested, Some(preset));
        assert_eq!(world.resource::<Startup>().weather.seed, seed);
        world.run_system_once(advance_simulation).unwrap();
        let recording = world.resource::<FlightRecorder>().0.recording();
        assert!(!recording.frames().is_empty());
        let mut bytes = Vec::new();
        recording.write_to(&mut bytes).unwrap();
        let decoded = flightsim_sim::ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded.format_version(), 3);
        assert_eq!(decoded.weather(), baseline_weather);
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
            progress: Arc::new(Mutex::new(Some(Progress::Import(ImportProgress {
                phase: flightsim_content::ImportPhase::Validating,
                files_done: 1,
                files_total: 2,
                bytes_done: 512,
                bytes_total: 1024,
            })))),
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

    #[test]
    fn app_owned_region_open_precedes_weather_shortcuts_in_combined_schedule() {
        use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
        use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
        use flightsim_ui::world_map::{self, WorldMapSystems};
        for lan in [false, true] {
            for catalog in [false, true] {
                for weather_registered_first in [false, true] {
                    let temp = TestDirectory::new();
                    let mut startup = Startup {
                        regions: Options {
                            store: Some(temp.store()),
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    startup.weather.requested = Some(WeatherPreset::Rain);
                    startup.weather.seed = 54;
                    startup.weather.selection = WeatherSelection::Modeled(
                        WeatherScenario::from_preset(
                            WeatherPreset::Rain,
                            Geodetic::from_degrees(35.55, 139.78, 0.0),
                            54,
                        )
                        .unwrap(),
                    );
                    if lan {
                        startup.traffic.join = Some("127.0.0.1:5678".parse().unwrap());
                    }
                    if catalog {
                        let path = temp.0.join("catalog.json");
                        std::fs::write(&path, br#"{"schema_version":1,"regions":[]}"#).unwrap();
                        startup.regions.catalog = Some(path);
                    }
                    let actual_weather = startup.weather.selection;
                    let conditions =
                        recording_conditions(&startup, &world_runtime::startup_clock(&startup));
                    let mut app = crate::controls_runtime_tests::control_app("light-single");
                    app.insert_resource(startup)
                        .insert_resource(FlightRecorder(flightsim_sim::CurrentRecorder::new(
                            conditions.clone(),
                        )))
                        .init_resource::<WorldMapState>()
                        .init_resource::<WorldMapActions>()
                        .init_resource::<ButtonInput<KeyCode>>()
                        .init_resource::<ButtonInput<MouseButton>>()
                        .add_message::<KeyboardInput>()
                        .add_message::<KeyboardFocusLost>()
                        .add_message::<bevy::window::FileDragAndDrop>()
                        .add_systems(
                            Update,
                            world_map::handle_world_map_input.in_set(WorldMapSystems::Input),
                        )
                        .add_systems(
                            Update,
                            world_runtime::capture_map_input
                                .after(WorldMapSystems::Input)
                                .before(advance_simulation)
                                .before(world_runtime::apply_world_map_start),
                        )
                        .add_systems(
                            Update,
                            world_runtime::apply_world_map_start.before(advance_simulation),
                        );
                    crate::controls_runtime_tests::tick(
                        &mut app,
                        std::time::Duration::from_millis(100),
                        flightsim_input::PilotKeys::default(),
                    );
                    let mut original_recording = Vec::new();
                    app.world()
                        .resource::<FlightRecorder>()
                        .0
                        .recording()
                        .write_to(&mut original_recording)
                        .unwrap();
                    assert!(
                        !app.world()
                            .resource::<FlightRecorder>()
                            .0
                            .recording()
                            .frames()
                            .is_empty()
                    );
                    if weather_registered_first {
                        weather_runtime::configure(&mut app);
                        configure(&mut app);
                    } else {
                        configure(&mut app);
                        weather_runtime::configure(&mut app);
                    }
                    app.world_mut().resource_mut::<WorldMapState>().visible = true;
                    if !catalog {
                        // A drop is processed after ordinary UI input; skip the
                        // independent first-open listing job to make it eligible.
                        app.world_mut().resource_mut::<RegionRuntime>().initialized = true;
                        app.world_mut()
                            .write_message(bevy::window::FileDragAndDrop::DroppedFile {
                                window: Entity::PLACEHOLDER,
                                path_buf: temp.zip(),
                            })
                            .unwrap();
                    }
                    let before = *app.world().resource::<FlightSimulation>().0.state();
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::F12);
                    app.update();
                    assert!(app.world().resource::<WorldMapState>().regions.visible);
                    assert_eq!(
                        app.world()
                            .resource::<weather_runtime::PendingWeather>()
                            .requested,
                        Some(WeatherPreset::Rain)
                    );
                    assert_eq!(
                        app.world().resource::<Startup>().weather.selection,
                        actual_weather
                    );
                    assert_eq!(
                        *app.world().resource::<FlightSimulation>().0.state(),
                        before
                    );
                    assert_eq!(
                        app.world()
                            .resource::<FlightRecorder>()
                            .0
                            .recording()
                            .conditions(),
                        &conditions
                    );
                    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
                    assert!(app.world().resource::<Startup>().active_region.is_none());
                    assert_eq!(
                        app.world()
                            .resource::<flightsim_render::RenderWeather>()
                            .selection,
                        actual_weather
                    );
                    let mut current_recording = Vec::new();
                    app.world()
                        .resource::<FlightRecorder>()
                        .0
                        .recording()
                        .write_to(&mut current_recording)
                        .unwrap();
                    assert_eq!(current_recording, original_recording);
                    if lan {
                        assert!(
                            app.world()
                                .resource::<ButtonInput<KeyCode>>()
                                .just_pressed(KeyCode::F12)
                        );
                    }
                    await_worker(app.world_mut());
                    assert_eq!(
                        app.world().resource::<Startup>().weather.selection,
                        actual_weather
                    );
                    assert_eq!(
                        *app.world().resource::<FlightSimulation>().0.state(),
                        before
                    );
                    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
                }
            }
        }
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

    #[test]
    fn catalog_cli_is_explicit_feature_gated_and_rejects_incomplete_or_conflicting_options() {
        for flag in [
            "--region-catalog",
            "--region-cache",
            "--region-offline",
            "region-downloads feature",
        ] {
            assert!(application_help().contains(flag));
        }
        for args in [
            vec!["--region-catalog"],
            vec!["--region-catalog", ""],
            vec!["--region-cache", "cache"],
            vec!["--region-offline"],
            vec!["--region-catalog", "catalog.json", "--region-cache", ""],
            vec!["--region-catalog", "catalog.json", "--region-cache"],
            vec![
                "--region-catalog",
                "catalog.json",
                "--region-catalog",
                "other.json",
            ],
            vec![
                "--region-catalog",
                "catalog.json",
                "--region-cache",
                "a",
                "--region-cache",
                "b",
            ],
            vec![
                "--region-catalog",
                "catalog.json",
                "--region-offline",
                "--region-offline",
            ],
            vec![
                "--region-catalog",
                "catalog.json",
                "--replay",
                "flight.fsreplay",
            ],
            vec![
                "--region-catalog",
                "catalog.json",
                "--global-terrain",
                "off",
            ],
            vec![
                "--region-catalog",
                "catalog.json",
                "--import-region",
                "data.zip",
            ],
            vec!["--region-catalog", "catalog.json", "--list-regions"],
        ] {
            let (startup, _) = parse_arguments_from(args.into_iter().map(str::to_owned));
            assert!(startup.regions.error.is_some());
        }
        let (startup, _) = parse_arguments_from(
            [
                "--region-catalog",
                "catalog.json",
                "--region-cache",
                "cache",
                "--region-offline",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        assert_eq!(
            startup.regions.error.is_none(),
            cfg!(feature = "region-downloads")
        );
        assert_eq!(
            startup.regions.catalog.as_deref(),
            Some(std::path::Path::new("catalog.json"))
        );
        assert!(startup.regions.offline);
        assert!(startup.world.map_open);
        assert!(startup.active_region.is_none());
        assert!(!run_cli(&startup.regions).unwrap());
    }
}
