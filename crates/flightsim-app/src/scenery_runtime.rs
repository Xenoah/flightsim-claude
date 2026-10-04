//! Optional regional scenery, with one bounded CPU preparation and staged GPU
//! upload. Geometry is visual only; the simulation and recording never read it.

use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use flightsim_render::scenery::{
    SceneryGroundMesh, SceneryMesh, SceneryMeshOptions, SceneryMeshStatistics,
};
use flightsim_render::terrain_drape::{ATTRIBUTE_OVERLAY_LIFT, TerrainOverlay};
use flightsim_render::{TerrainOverlayRegistration, TerrainOverlaySwap};
use flightsim_world::draw_distance::DrawDistancePolicy;
use flightsim_world::scenery::{
    LandCoverClass, SceneryDatabase, SceneryFeatureRef, ScenerySourceKind,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Includes the 1.8 km tree radius plus the largest road-width/clearance margin.
const TREE_EXCLUSION_RADIUS: Meters = Meters(2_300.0);
const RESELECT_DISTANCE: Meters = Meters(450.0);
const CELL_METRES: f64 = 1_024.0;
const MAX_SELECTED: usize = 4_096;
const FEATURES_PER_BATCH: usize = 64;
const MAX_BATCHES: usize = 128;
const MAX_UPLOAD_BATCHES: usize = 8;
const MAX_UPLOAD_VERTICES: usize = flightsim_render::scenery::MAX_BATCH_VERTICES;
const MAX_GROUND_BATCHES: usize = 24;
const MAX_GROUND_VERTICES: usize = 90_000;
const GROUND_CELL_METRES: f64 = 1_024.0;
const MAX_VERTICES: usize = 600_000;
const MAX_TREES: usize = 2_048;
const MAX_TREE_CANDIDATES: usize = 8_192;
const MAX_EXCLUSION_SEGMENTS: usize = 32_768;
const MAX_EXCLUSION_SPHERES: usize = 8_192;
const EXCLUSION_CELL_METRES: f64 = 128.0;
const MAX_EXCLUSION_INDEX_REFS: usize = 262_144;
const MAX_EXCLUSION_CELL_REFS: usize = 512;
const MAX_BROAD_EXCLUSIONS: usize = 512;

#[derive(Resource)]
pub(super) struct SceneryRuntime {
    database: Option<Arc<SceneryDatabase>>,
    build_config: Option<Arc<SceneryBuildConfig>>,
    material: Option<Handle<StandardMaterial>>,
    selected_at: Option<Geodetic>,
    generation: u64,
    reset_revision: u64,
    draw_distance: DrawDistancePolicy,
    distance_dirty: bool,
    pending: Option<PendingBuild>,
    prepared: VecDeque<SceneryMesh>,
    staged: Vec<(Entity, Handle<Mesh>)>,
    visible: Vec<(Entity, Handle<Mesh>)>,
    staged_ground: Vec<TerrainOverlayRegistration>,
    incoming_ground: Vec<(Entity, Handle<Mesh>)>,
    visible_ground: Vec<(Entity, Handle<Mesh>)>,
    overlay_swap: Option<TerrainOverlaySwap>,
    ready_to_admit: bool,
    prepared_stats: BuildStatistics,
}

#[derive(Resource, Debug, Default)]
pub(super) struct SceneryReset(pub u64);
impl SceneryReset {
    pub fn request(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

struct PendingBuild {
    generation: u64,
    cancel: Arc<AtomicBool>,
    task: Task<BuildResult>,
}

#[derive(Debug, Default, Clone, Copy)]
struct BuildStatistics {
    selected: usize,
    batches: usize,
    vertices: usize,
    ground_vertices: usize,
    roads: usize,
    buildings: usize,
    land_polygons: usize,
    trees: usize,
    tree_candidates: usize,
    skipped_features: usize,
    truncated: bool,
    cpu_ms: f64,
}

#[derive(Debug)]
struct BuildResult {
    meshes: VecDeque<SceneryMesh>,
    statistics: BuildStatistics,
}

impl SceneryRuntime {
    /// Invalidate prepared/staged scenery before changing the query radius.
    /// Keep the single cancelled worker until it finishes, so rapid changes
    /// cannot accumulate background jobs. `stream` reclaims the old assets
    /// before polling/uploading and starts only the latest requested policy.
    pub(super) fn set_draw_distance(&mut self, policy: DrawDistancePolicy) -> bool {
        if self.draw_distance == policy {
            return false;
        }
        self.draw_distance = policy;
        self.distance_dirty = true;
        self.generation = self.generation.wrapping_add(1);
        if let Some(pending) = &self.pending {
            pending.cancel.store(true, Ordering::Relaxed);
        }
        true
    }

    /// Derive the local refinement request from immutable loaded coverage, not
    /// from asynchronously changing render batches. Leaving/re-entering a pack
    /// or relocating the map therefore cannot retain an old regional request.
    pub(super) fn terrain_selector(&self, selector: LodSelector, camera: Geodetic) -> LodSelector {
        let radius = self.draw_distance.terrain_detail_radius();
        if self
            .database
            .as_ref()
            .is_some_and(|database| database.bounds().intersects(camera, radius))
        {
            selector.with_near_detail(radius, 13)
        } else {
            selector.without_near_detail()
        }
    }

    /// A stable per-tile policy: never change an existing mesh just because the
    /// observer moved. The bundled global fallback has 33 samples and remains
    /// at its previous resolution. Grid size is a density heuristic, not a
    /// claim that an arbitrary user-supplied tile has verified provenance.
    pub(super) fn terrain_mesh_options(
        &self,
        id: flightsim_world::TileId,
        dem: &flightsim_world::DemTile,
    ) -> flightsim_world::MeshOptions {
        let mut options = flightsim_render::mesh_options_for(id.level);
        if id.level < 12 || dem.grid().width() < 65 || dem.grid().height() < 65 {
            return options;
        }
        let bounds = id.bounds();
        let center = bounds.center();
        let radius = [
            (bounds.north, bounds.west),
            (bounds.north, bounds.east),
            (bounds.south, bounds.west),
            (bounds.south, bounds.east),
        ]
        .into_iter()
        .map(|(latitude, longitude)| {
            center
                .to_ecef()
                .distance_to(Geodetic::new(latitude, longitude, Meters::ZERO).to_ecef())
                .get()
        })
        .fold(0.0, f64::max);
        if self
            .database
            .as_ref()
            .is_some_and(|database| database.bounds().intersects(center, Meters(radius)))
        {
            options.resolution = 65;
        }
        options
    }

    pub(super) fn is_loading(&self) -> bool {
        self.pending.is_some()
            || !self.prepared.is_empty()
            || self.overlay_swap.is_some()
            || self.ready_to_admit
    }
    pub fn new(startup: &Startup) -> Result<Self, String> {
        Self::new_with_draw_distance(startup, startup.draw_distance.policy())
    }

    pub(super) fn new_with_draw_distance(
        startup: &Startup,
        draw_distance: DrawDistancePolicy,
    ) -> Result<Self, String> {
        if startup.scenery.is_some() && !startup.world.global_terrain && startup.tiles.is_none() {
            return Err("regional scenery needs --global-terrain on or an explicit --tiles directory for displayed ground support".into());
        }
        let database = startup
            .scenery
            .as_ref()
            .map(|path| {
                SceneryDatabase::read_path(path)
                    .map(Arc::new)
                    .map_err(|error| format!("{}: {error}", path.display()))
            })
            .transpose()?;
        let mut runtime = Self {
            build_config: database
                .as_ref()
                .map(|_| Arc::new(SceneryBuildConfig::new(startup))),
            database,
            material: None,
            selected_at: None,
            generation: 0,
            reset_revision: 0,
            draw_distance: DrawDistancePolicy::default(),
            distance_dirty: false,
            pending: None,
            prepared: VecDeque::new(),
            staged: Vec::new(),
            visible: Vec::new(),
            staged_ground: Vec::new(),
            incoming_ground: Vec::new(),
            visible_ground: Vec::new(),
            overlay_swap: None,
            ready_to_admit: false,
            prepared_stats: BuildStatistics::default(),
        };
        runtime.set_draw_distance(draw_distance);
        runtime.distance_dirty = false;
        Ok(runtime)
    }

    /// A regional free flight does not retain depiction built with the old
    /// terrain or airport exclusions. No stale worker can upload after clear.
    pub(super) fn suppress_after_source_change(&mut self) {
        self.database = None;
        self.build_config = None;
        self.pending = None;
    }

    pub fn attribution(&self) -> Option<&'static str> {
        self.database
            .as_ref()
            .map(|database| match database.source().kind {
                ScenerySourceKind::OpenStreetMap => {
                    "(c) OpenStreetMap contributors / ODbL: openstreetmap.org/copyright; procedural heights/trees"
                }
                ScenerySourceKind::Synthetic => {
                    "Scenery: synthetic test fixture; procedural depiction"
                }
            })
    }
}

/// Relocation drops every owned mesh and cancels stale CPU work. A worker may
/// finish its current capped batch, but its old generation can never be uploaded.
pub(super) fn clear(world: &mut World) {
    let Some(mut runtime) = world.remove_resource::<SceneryRuntime>() else {
        return;
    };
    let assets = reset_owned_assets(
        &mut runtime,
        world.get_resource_mut::<TerrainTiles>().as_deref_mut(),
    );
    for (entity, mesh) in assets {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
        world.resource_mut::<Assets<Mesh>>().remove(&mesh);
    }
    world.insert_resource(runtime);
}

fn reset_owned_assets(
    runtime: &mut SceneryRuntime,
    tiles: Option<&mut TerrainTiles>,
) -> Vec<(Entity, Handle<Mesh>)> {
    runtime.generation = runtime.generation.wrapping_add(1);
    if let Some(pending) = &runtime.pending {
        pending.cancel.store(true, Ordering::Relaxed);
    }
    runtime.prepared.clear();
    runtime.selected_at = None;
    runtime.ready_to_admit = false;
    runtime.distance_dirty = false;
    let mut owned = BTreeMap::new();
    for pair in runtime
        .visible
        .drain(..)
        .chain(runtime.staged.drain(..))
        .chain(runtime.incoming_ground.drain(..))
        .chain(runtime.visible_ground.drain(..))
    {
        owned.insert(pair.0, pair.1);
    }
    for registration in runtime.staged_ground.drain(..) {
        owned.insert(registration.entity, registration.mesh);
    }
    if let Some(swap) = runtime.overlay_swap.take() {
        for pair in swap.retired {
            owned.insert(pair.0, pair.1);
        }
    }
    if let Some(tiles) = tiles {
        for pair in tiles.clear_optional_overlays() {
            owned.insert(pair.0, pair.1);
        }
    }
    owned.into_iter().collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "bounded scenery lifecycle uses existing render resources without touching physics"
)]
pub(super) fn stream(
    mut commands: Commands,
    camera: Res<CameraWorldPosition>,
    origin: Res<RenderOrigin>,
    reset: Res<SceneryReset>,
    mut runtime: ResMut<SceneryRuntime>,
    mut tiles: ResMut<TerrainTiles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let runtime = &mut *runtime;
    if runtime.database.is_none() {
        return;
    }
    if runtime.reset_revision != reset.0 || runtime.distance_dirty {
        runtime.reset_revision = reset.0;
        for (entity, mesh) in reset_owned_assets(runtime, Some(&mut tiles)) {
            commands.entity(entity).despawn();
            meshes.remove(&mesh);
        }
    }
    if runtime.material.is_none() {
        runtime.material = Some(materials.add(flightsim_render::scenery::scenery_material()));
    }
    if let Some(pending) = &mut runtime.pending
        && let Some(result) = block_on(poll_once(&mut pending.task))
    {
        if pending.generation == runtime.generation && !pending.cancel.load(Ordering::Relaxed) {
            runtime.prepared = result.meshes;
            runtime.prepared_stats = result.statistics;
            runtime.ready_to_admit = runtime.prepared.is_empty();
        }
        runtime.pending = None;
    }
    // Bound both entity/asset attempts and total geometry per frame. Several
    // small batches can share the same old one-batch worst-case vertex allowance.
    // Keep admission deferred until these commands have reached the world.
    let mut uploaded_vertices = 0;
    let mut attempted_upload = false;
    for _ in 0..MAX_UPLOAD_BATCHES {
        let Some(next) = runtime.prepared.front() else {
            break;
        };
        let next_vertices = next.ground.as_ref().map_or_else(
            || next.mesh.count_vertices(),
            |ground| ground.mesh.count_vertices(),
        );
        if next_vertices > MAX_UPLOAD_VERTICES || next_vertices == 0 {
            // Impossible output must not permanently block later valid batches.
            // Empty and malformed work still consumes an attempt in this loop.
            runtime.prepared.pop_front();
            runtime.prepared_stats.truncated = true;
            attempted_upload = true;
            warn!("scenery upload skipped invalid vertex count: {next_vertices}");
            continue;
        }
        if next_vertices > MAX_UPLOAD_VERTICES - uploaded_vertices {
            break; // Leave this complete batch for a fresh frame's allowance.
        }
        let mut batch = runtime.prepared.pop_front().expect("peeked prepared batch");
        attempted_upload = true;
        let mut overlay = None;
        let mesh = if let Some(mut ground) = batch.ground.take() {
            let Some(source) = ground.overlay.take() else {
                warn!("scenery ground was not prepared; refusing render-thread template work");
                runtime.prepared_stats.truncated = true;
                continue;
            };
            overlay = Some(source);
            ground.mesh
        } else {
            batch.mesh
        };
        uploaded_vertices += next_vertices;
        let handle = meshes.add(mesh);
        let entity = commands
            .spawn((
                flightsim_render::terrain_mesh_bundle(
                    handle.clone(),
                    runtime.material.clone().expect("material initialized"),
                    batch.origin,
                ),
                Name::new("regional scenery batch"),
            ))
            .insert((
                Transform {
                    translation: origin.0.to_render(batch.origin),
                    rotation: origin.0.rotation_to_render(bevy::math::DQuat::IDENTITY),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .id();
        if let Some(source) = overlay {
            runtime.staged_ground.push(TerrainOverlayRegistration {
                entity,
                mesh: handle,
                source,
            });
        } else {
            runtime.staged.push((entity, handle));
        }
    }
    if attempted_upload {
        if runtime.prepared.is_empty() {
            runtime.ready_to_admit = true;
        }
        return;
    }
    if runtime.ready_to_admit
        && runtime.overlay_swap.is_none()
        && !tiles.overlay_usage().optional_swap_pending
    {
        let targets: Vec<_> = runtime
            .staged_ground
            .iter()
            .map(|r| (r.entity, r.mesh.clone()))
            .collect();
        match tiles.replace_optional_overlays(std::mem::take(&mut runtime.staged_ground)) {
            Ok(swap) => {
                runtime.incoming_ground = targets;
                runtime.overlay_swap = Some(swap);
            }
            Err(error) => {
                warn!("scenery overlay admission rejected; previous scene retained: {error}");
                for (entity, mesh) in targets.into_iter().chain(runtime.staged.drain(..)) {
                    commands.entity(entity).despawn();
                    meshes.remove(&mesh);
                }
            }
        }
        runtime.ready_to_admit = false;
    }
    if runtime
        .overlay_swap
        .as_ref()
        .is_some_and(|swap| tiles.overlay_usage().committed_revision >= swap.revision)
    {
        let swap = runtime.overlay_swap.take().expect("checked swap");
        for (entity, mesh) in swap.retired {
            commands.entity(entity).despawn();
            meshes.remove(&mesh);
        }
        runtime.visible_ground = std::mem::take(&mut runtime.incoming_ground);
        commit_scene(&mut commands, &mut meshes, runtime);
    }
    if runtime.pending.is_some() || runtime.overlay_swap.is_some() || runtime.ready_to_admit {
        return;
    }
    let needs_selection = runtime
        .selected_at
        .is_none_or(|last| last.great_circle_distance(camera.0).get() >= RESELECT_DISTANCE.get());
    if !needs_selection {
        return;
    }
    runtime.selected_at = Some(camera.0);
    runtime.generation = runtime.generation.wrapping_add(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let job_cancel = Arc::clone(&cancel);
    let database = Arc::clone(runtime.database.as_ref().expect("loaded database"));
    let config = Arc::clone(
        runtime
            .build_config
            .as_ref()
            .expect("loaded scenery configuration"),
    );
    let observer = camera.0;
    let draw_distance = runtime.draw_distance;
    let task = AsyncComputeTaskPool::get().spawn(async move {
        build_scene_config(&database, &config, observer, &job_cancel, draw_distance)
    });
    runtime.pending = Some(PendingBuild {
        generation: runtime.generation,
        cancel,
        task,
    });
}

fn commit_scene(commands: &mut Commands, meshes: &mut Assets<Mesh>, runtime: &mut SceneryRuntime) {
    for (entity, _) in &runtime.staged {
        commands.entity(*entity).insert(Visibility::Inherited);
    }
    for (entity, mesh) in runtime.visible.drain(..) {
        commands.entity(entity).despawn();
        meshes.remove(&mesh);
    }
    runtime.visible = std::mem::take(&mut runtime.staged);
    let s = runtime.prepared_stats;
    info!(
        "scenery prepared: {} batches, {} selected features, {} roads, {} buildings, {} land polygons, {} procedural trees, {} vertices; skipped {}, truncated {}; CPU preparation {:.1} ms; visual-only regional data",
        runtime.visible.len() + runtime.visible_ground.len(),
        s.selected,
        s.roads,
        s.buildings,
        s.land_polygons,
        s.trees,
        s.vertices,
        s.skipped_features,
        s.truncated,
        s.cpu_ms
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cell([i32; 3]);

fn feature_points(feature: SceneryFeatureRef<'_>) -> &[Geodetic] {
    match feature {
        SceneryFeatureRef::Road(f) => &f.points,
        SceneryFeatureRef::Building(f) => &f.footprint,
        SceneryFeatureRef::LandCover(f) => &f.boundary,
    }
}

fn feature_center(feature: SceneryFeatureRef<'_>) -> Geodetic {
    let points = feature_points(feature);
    let sum = points
        .iter()
        .fold(bevy::math::DVec3::ZERO, |sum, p| sum + p.to_ecef().as_vec());
    let count = u32::try_from(points.len()).expect("validated regional point limit");
    let p = flightsim_core::Ecef::from_vec(sum / f64::from(count)).to_geodetic();
    Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
}

fn feature_key(feature: SceneryFeatureRef<'_>) -> (u8, i64) {
    match feature {
        SceneryFeatureRef::Building(f) => (0, f.source_id),
        SceneryFeatureRef::Road(f) => (1, f.source_id),
        SceneryFeatureRef::LandCover(f) => (2, f.source_id),
    }
}

fn cell_for(position: Geodetic) -> Cell {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated ECEF Earth coordinates divided by1024 lie safely inside i32"
    )]
    Cell(
        position
            .to_ecef()
            .as_vec()
            .to_array()
            .map(|x| (x / CELL_METRES).floor() as i32),
    )
}

/// Only a small source configuration and a capped precomputed exclusion index
/// are retained; reselection never clones the potentially large airport DB.
struct SceneryBuildConfig {
    source: Startup,
    airport_exclusions: Exclusions,
    ground_exclusions: flightsim_render::scenery_exclusions::GroundSceneryExclusions,
}
impl SceneryBuildConfig {
    fn new(startup: &Startup) -> Self {
        let source = Startup {
            tiles: startup.tiles.clone(),
            active_region: startup.active_region.clone(),
            airport_enabled: startup.airport_enabled,
            world: world_runtime::WorldOptions {
                global_terrain: startup.world.global_terrain,
                ..Default::default()
            },
            min_level: startup.min_level,
            max_level: startup.max_level,
            runway: startup.runway,
            ..Default::default()
        };
        let mut ground_exclusions =
            flightsim_render::scenery_exclusions::GroundSceneryExclusions::default();
        ground_exclusions.add_segment(
            startup.runway.threshold,
            startup.runway.opposite_threshold(),
            Meters(startup.runway.width.get() * 0.5),
        );
        for taxiway in &startup.taxiways {
            for pair in taxiway.points().windows(2) {
                ground_exclusions.add_segment(pair[0], pair[1], Meters(taxiway.width.get() * 0.5));
            }
            if ground_exclusions.is_suppressed() {
                break;
            }
        }
        for apron in &startup.aprons {
            for triangle in apron.triangles() {
                ground_exclusions.add_polygon(triangle);
            }
            if ground_exclusions.is_suppressed() {
                break;
            }
        }
        Self {
            source,
            airport_exclusions: Exclusions::new(&[], startup),
            ground_exclusions,
        }
    }
}

#[cfg(test)]
fn build_scene(
    database: &SceneryDatabase,
    startup: &Startup,
    observer: Geodetic,
    cancel: &AtomicBool,
) -> BuildResult {
    build_scene_config(
        database,
        &SceneryBuildConfig::new(startup),
        observer,
        cancel,
        DrawDistancePolicy::default(),
    )
}

fn build_scene_config(
    database: &SceneryDatabase,
    config: &SceneryBuildConfig,
    observer: Geodetic,
    cancel: &AtomicBool,
    draw_distance: DrawDistancePolicy,
) -> BuildResult {
    let startup = &config.source;
    let start = std::time::Instant::now();
    let selected = database.query_near(observer, draw_distance.scenery_radius(), MAX_SELECTED);
    let tree_neighbours = database.query_near(observer, TREE_EXCLUSION_RADIUS, MAX_SELECTED);
    let mut exclusions = Exclusions::new(&tree_neighbours, startup);
    // Truncation may omit a road, building or water exclusion. Suppress trees
    // rather than claim their placement was checked against absent features.
    exclusions.suppress_all |= tree_neighbours.len() == MAX_SELECTED;
    let mut groups: BTreeMap<Cell, Vec<SceneryFeatureRef<'_>>> = BTreeMap::new();
    for &feature in &selected {
        groups
            .entry(cell_for(feature_center(feature)))
            .or_default()
            .push(feature);
    }
    let mut batches = Vec::new();
    for features in groups.values_mut() {
        features.sort_by_key(|f| feature_key(*f));
        for group in features.chunks(FEATURES_PER_BATCH) {
            let anchor = feature_center(group[0]);
            batches.push((anchor, group.to_vec()));
        }
    }
    batches.sort_by(|a, b| {
        a.0.great_circle_distance(observer)
            .get()
            .total_cmp(&b.0.great_circle_distance(observer).get())
            .then_with(|| feature_key(a.1[0]).cmp(&feature_key(b.1[0])))
    });
    let mut statistics = BuildStatistics {
        selected: selected.len(),
        truncated: selected.len() == MAX_SELECTED
            || batches.len() > MAX_BATCHES - MAX_GROUND_BATCHES,
        ..Default::default()
    };
    let mut output = VecDeque::new();
    let mut ground_parts = Vec::new();
    let mut terrain = Terrain::new(
        make_source(startup),
        16 * 1024 * 1024,
        world_runtime::terrain_levels(startup),
    );
    for (anchor, features) in batches.into_iter().take(MAX_BATCHES - MAX_GROUND_BATCHES) {
        if cancel.load(Ordering::Relaxed) {
            return BuildResult {
                meshes: VecDeque::new(),
                statistics: BuildStatistics::default(),
            };
        }
        if statistics.vertices >= MAX_VERTICES - 3 {
            statistics.truncated = true;
            break;
        }
        let mut options = SceneryMeshOptions::near(observer);
        options.vegetation_distance = Meters(
            options
                .vegetation_distance
                .get()
                .min(draw_distance.scenery_radius().get()),
        );
        options.max_vertices = MAX_VERTICES - statistics.vertices;
        options.max_ground_vertices =
            MAX_GROUND_VERTICES.saturating_sub(statistics.ground_vertices);
        options.max_trees = MAX_TREES.saturating_sub(statistics.trees);
        options.max_tree_candidates =
            MAX_TREE_CANDIDATES.saturating_sub(statistics.tree_candidates);
        options.cancellation = Some(cancel);
        options.airport_ground_exclusions = Some(&config.ground_exclusions);
        options.vegetation = !exclusions.suppress_all && !config.airport_exclusions.suppress_all;
        options.facades = anchor.great_circle_distance(observer).get() < 2_400.0;
        let batch = flightsim_render::scenery::scenery_mesh(
            &features,
            anchor,
            options,
            &mut |point| {
                terrain.elevation_at(point).or_else(|| {
                    (!startup.world.global_terrain && startup.tiles.is_none())
                        .then_some(Meters::ZERO)
                })
            },
            &|point| exclusions.contains(point) || config.airport_exclusions.contains(point),
        );
        if let Some(mut batch) = batch {
            let SceneryMeshStatistics {
                roads,
                buildings,
                land_polygons,
                procedural_trees,
                tree_candidates,
                skipped_features,
                vertices,
                ..
            } = batch.statistics;
            statistics.roads += roads;
            statistics.buildings += buildings;
            statistics.land_polygons += land_polygons;
            statistics.trees += procedural_trees;
            statistics.tree_candidates += tree_candidates;
            statistics.skipped_features += skipped_features;
            statistics.vertices += vertices;
            if let Some(ground) = batch.ground.take() {
                statistics.ground_vertices += ground.mesh.count_vertices();
                ground_parts.push((batch.origin, ground));
            }
            if batch.mesh.count_vertices() > 0 {
                output.push_back(batch);
            }
        } else {
            statistics.skipped_features += features.len();
            // Empty/rejected geometry may still have tested tree candidates.
            // Conservatively charge its assigned allowance, not zero, so the
            // scene-wide budget remains true even when no mesh is returned.
            if options.vegetation {
                statistics.tree_candidates += options
                    .max_tree_candidates
                    .min(flightsim_render::scenery::MAX_BATCH_TREE_CANDIDATES);
            }
        }
    }
    let (grounds, truncated) = merge_ground(ground_parts, observer);
    statistics.truncated |= truncated;
    output.extend(grounds);
    statistics.batches = output.len();
    statistics.cpu_ms = start.elapsed().as_secs_f64() * 1000.0;
    BuildResult {
        meshes: output,
        statistics,
    }
}

/// Merge nearby ground geometry before registry admission. Solids retain finer
/// cell culling; optional terrain overlays never consume more than24 slots.
fn merge_ground(
    parts: Vec<(flightsim_core::Ecef, SceneryGroundMesh)>,
    observer: Geodetic,
) -> (Vec<SceneryMesh>, bool) {
    let mut groups: BTreeMap<Cell, Vec<(flightsim_core::Ecef, SceneryGroundMesh)>> =
        BTreeMap::new();
    for (origin, mut ground) in parts {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "bounded Earth ECEF/4096 safely fits i32"
        )]
        let key = Cell(
            origin
                .as_vec()
                .to_array()
                .map(|v| (v / GROUND_CELL_METRES).floor() as i32),
        );
        let group = groups.entry(key).or_default();
        if let Some((target_origin, target)) = group.last_mut()
            && target.mesh.count_vertices() + ground.mesh.count_vertices() <= 65_532
        {
            ground
                .mesh
                .translate_by((origin.as_vec() - target_origin.as_vec()).as_vec3());
            target
                .mesh
                .merge(&ground.mesh)
                .expect("identical internal scenery mesh layout");
            target.lifts.extend(ground.lifts);
        } else {
            group.push((origin, ground));
        }
    }
    let mut all: Vec<_> = groups.into_values().flatten().collect();
    all.sort_by(|a, b| {
        a.0.to_geodetic()
            .great_circle_distance(observer)
            .get()
            .total_cmp(&b.0.to_geodetic().great_circle_distance(observer).get())
    });
    // The global90k vertex budget guarantees spare capacity in this24-slot
    // partition. Merge overflow into its nearest admissible neighbour instead
    // of silently dropping roads or land polygons solely due to cell metadata.
    let mut truncated = false;
    while all.len() > MAX_GROUND_BATCHES {
        let (origin, mut ground) = all.pop().expect("excess ground group");
        let target = (0..all.len())
            .filter(|&i| all[i].1.mesh.count_vertices() + ground.mesh.count_vertices() <= 65_532)
            .min_by(|&a, &b| {
                all[a]
                    .0
                    .distance_to(origin)
                    .get()
                    .total_cmp(&all[b].0.distance_to(origin).get())
            });
        if let Some(index) = target {
            let (target_origin, target) = &mut all[index];
            ground
                .mesh
                .translate_by((origin.as_vec() - target_origin.as_vec()).as_vec3());
            target
                .mesh
                .merge(&ground.mesh)
                .expect("identical internal scenery mesh layout");
            target.lifts.extend(ground.lifts);
        } else {
            truncated = true;
        }
    }
    let meshes = all
        .into_iter()
        .take(MAX_GROUND_BATCHES)
        .filter_map(|(origin, mut ground)| {
            ground
                .mesh
                .insert_attribute(ATTRIBUTE_OVERLAY_LIFT, ground.lifts.clone());
            match TerrainOverlay::surface(&ground.mesh, origin, |_| Meters::ZERO) {
                Ok(source) => ground.overlay = Some(source),
                Err(error) => {
                    warn!("scenery ground preparation rejected: {error}");
                    truncated = true;
                    return None;
                }
            }
            Some(SceneryMesh {
                origin,
                mesh: Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    bevy::asset::RenderAssetUsages::default(),
                ),
                ground: Some(ground),
                statistics: SceneryMeshStatistics::default(),
            })
        })
        .collect();
    (meshes, truncated)
}

/// Conservative exclusions use the same horizontal ECEF space as source data.
/// A dense input exceeding the segment budget disables decorative trees rather
/// than silently placing them over an unchecked road or taxiway.
struct Exclusions {
    runway: Runway,
    segments: Vec<(bevy::math::DVec3, bevy::math::DVec3, f64)>,
    spheres: Vec<(bevy::math::DVec3, f64)>,
    index: BTreeMap<Cell, Vec<ExclusionRef>>,
    broad: Vec<ExclusionRef>,
    index_refs: usize,
    suppress_all: bool,
}

#[derive(Debug, Clone, Copy)]
enum ExclusionRef {
    Segment(usize),
    Sphere(usize),
}

fn exclusion_cell(point: bevy::math::DVec3) -> Cell {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated Earth ECEF coordinates/128 safely fit i32"
    )]
    Cell(
        point
            .to_array()
            .map(|v| (v / EXCLUSION_CELL_METRES).floor() as i32),
    )
}

impl Exclusions {
    fn new(features: &[SceneryFeatureRef<'_>], startup: &Startup) -> Self {
        let mut result = Self {
            runway: startup.runway,
            segments: Vec::new(),
            spheres: Vec::new(),
            index: BTreeMap::new(),
            broad: Vec::new(),
            index_refs: 0,
            suppress_all: false,
        };
        for &feature in features {
            match feature {
                SceneryFeatureRef::Road(road) => {
                    result.add_line(&road.points, road.width.get() * 0.5 + 5.0)
                }
                SceneryFeatureRef::Building(_) => result.add_sphere(feature_points(feature), 5.0),
                SceneryFeatureRef::LandCover(land) if land.class == LandCoverClass::Water => {
                    result.add_sphere(&land.boundary, 5.0)
                }
                _ => {}
            }
        }
        for taxiway in &startup.taxiways {
            result.add_line(taxiway.points(), taxiway.width.get() * 0.5 + 8.0);
        }
        for apron in &startup.aprons {
            for triangle in apron.triangles() {
                result.add_sphere(triangle, 8.0);
            }
        }
        result
    }
    fn add_line(&mut self, points: &[Geodetic], width: f64) {
        for pair in points.windows(2) {
            if self.segments.len() >= MAX_EXCLUSION_SEGMENTS {
                self.suppress_all = true;
                return;
            }
            let Some(pieces) = flightsim_render::scenery_exclusions::horizontal_surface_segments(
                pair[0],
                pair[1],
                MAX_EXCLUSION_SEGMENTS.saturating_sub(self.segments.len()),
            ) else {
                self.suppress_all = true;
                return;
            };
            for (a, b) in pieces {
                let a = a.as_vec();
                let b = b.as_vec();
                let index = self.segments.len();
                self.segments.push((a, b, width * width));
                self.insert_bounds(
                    a.min(b) - bevy::math::DVec3::splat(width),
                    a.max(b) + bevy::math::DVec3::splat(width),
                    ExclusionRef::Segment(index),
                );
            }
        }
    }
    fn add_sphere(&mut self, points: &[Geodetic], padding: f64) {
        if self.spheres.len() >= MAX_EXCLUSION_SPHERES {
            self.suppress_all = true;
            return;
        }
        if points.is_empty() {
            return;
        }
        let n = u32::try_from(points.len()).expect("validated point budget");
        let center = points
            .iter()
            .map(|&p| horizontal(p))
            .sum::<bevy::math::DVec3>()
            / f64::from(n);
        let radius = points
            .iter()
            .map(|&p| horizontal(p).distance(center))
            .fold(0.0, f64::max)
            + padding;
        let index = self.spheres.len();
        self.spheres.push((center, radius * radius));
        self.insert_bounds(
            center - bevy::math::DVec3::splat(radius),
            center + bevy::math::DVec3::splat(radius),
            ExclusionRef::Sphere(index),
        );
    }
    fn insert_bounds(
        &mut self,
        minimum: bevy::math::DVec3,
        maximum: bevy::math::DVec3,
        reference: ExclusionRef,
    ) {
        if self.suppress_all {
            return;
        }
        let low = exclusion_cell(minimum).0;
        let high = exclusion_cell(maximum).0;
        let count = (0..3).try_fold(1_i64, |n, axis| {
            n.checked_mul(i64::from(high[axis]) - i64::from(low[axis]) + 1)
        });
        if count.is_none_or(|n| n > 512) {
            if self.broad.len() >= MAX_BROAD_EXCLUSIONS {
                self.suppress_all = true;
                return;
            }
            self.broad.push(reference);
            return;
        }
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    if self.index_refs >= MAX_EXCLUSION_INDEX_REFS {
                        self.suppress_all = true;
                        return;
                    }
                    let cell = self.index.entry(Cell([x, y, z])).or_default();
                    if cell.len() >= MAX_EXCLUSION_CELL_REFS {
                        self.suppress_all = true;
                        return;
                    }
                    cell.push(reference);
                    self.index_refs += 1;
                }
            }
        }
    }
    fn matches(&self, p: bevy::math::DVec3, reference: ExclusionRef) -> bool {
        match reference {
            ExclusionRef::Sphere(index) => {
                let (center, r2) = self.spheres[index];
                center.distance_squared(p) <= r2
            }
            ExclusionRef::Segment(index) => {
                let (a, b, r2) = self.segments[index];
                let d = b - a;
                let length = d.length_squared();
                let t = if length > 0.0 {
                    ((p - a).dot(d) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + d * t).distance_squared(p) <= r2
            }
        }
    }
    fn contains(&self, point: Geodetic) -> bool {
        if self.suppress_all {
            return true;
        }
        if self.runway.is_same_hemisphere(point) {
            let offset = self.runway.offsets(point);
            if (-60.0..=self.runway.length.get() + 60.0).contains(&offset.longitudinal.get())
                && offset.lateral.get().abs() < self.runway.width.get() * 0.5 + 35.0
            {
                return true;
            }
        }
        let p = horizontal(point);
        self.broad.iter().any(|&r| self.matches(p, r))
            || self
                .index
                .get(&exclusion_cell(p))
                .is_some_and(|refs| refs.iter().any(|&r| self.matches(p, r)))
    }
}

fn horizontal(p: Geodetic) -> bevy::math::DVec3 {
    Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
        .to_ecef()
        .as_vec()
}

#[cfg(test)]
#[path = "scenery_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "scenery_terrain_policy_tests.rs"]
mod terrain_policy_tests;
