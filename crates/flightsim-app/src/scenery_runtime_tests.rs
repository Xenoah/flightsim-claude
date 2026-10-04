//! Independent CPU/ECS lifecycle regressions. These tests allocate Bevy assets,
//! but deliberately create no renderer, device, window or GPU submission.
#![allow(clippy::float_cmp)]

use super::*;
use bevy::mesh::VertexAttributeValues;
use bevy::tasks::TaskPool;
use flightsim_world::scenery::{BuildingHeightSource, SceneryBuilding, ScenerySource};
use std::time::{Duration, Instant};

fn anchor() -> Geodetic {
    Geodetic::from_degrees(47.13, 9.53, 0.0)
}

fn point(north: f64, east: f64) -> Geodetic {
    let p = LocalFrame::new(anchor())
        .ned_to_ecef_position(Ned::new(north, east, 0.0))
        .to_geodetic();
    Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
}

fn startup() -> Startup {
    let mut startup = Startup::default();
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    startup
}

fn building(id: u32) -> SceneryBuilding {
    let north = f64::from(id / 64) * 24.0;
    let east = f64::from(id % 64) * 30.0;
    SceneryBuilding {
        source_id: i64::from(id) + 1,
        height: Meters(12.0),
        height_source: BuildingHeightSource::Default,
        footprint: vec![
            point(north, east),
            point(north, east + 20.0),
            point(north + 16.0, east + 20.0),
            point(north + 16.0, east),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    }
}

fn database(buildings: Vec<SceneryBuilding>) -> SceneryDatabase {
    SceneryDatabase::new(
        ScenerySource {
            kind: ScenerySourceKind::Synthetic,
            name: "CPU lifecycle fixture".into(),
            url: "urn:flightsim:test:scenery-runtime".into(),
            input_fingerprint: 1,
        },
        vec![],
        buildings,
        vec![],
    )
    .unwrap()
}

fn batch() -> SceneryMesh {
    flightsim_render::scenery::scenery_mesh(
        &[SceneryFeatureRef::Building(&building(0))],
        anchor(),
        SceneryMeshOptions::near(anchor()),
        &mut |_| Some(Meters::ZERO),
        &|_| false,
    )
    .unwrap()
}

fn fixture_app() -> App {
    AsyncComputeTaskPool::get_or_init(TaskPool::new);
    let startup = startup();
    let mut runtime = SceneryRuntime::new(&startup).unwrap();
    let mut outside_coverage = building(0);
    for p in &mut outside_coverage.footprint {
        *p = p.offset_by(Meters(20_000.0), Meters::ZERO);
    }
    runtime.database = Some(Arc::new(database(vec![outside_coverage])));
    runtime.build_config = Some(Arc::new(SceneryBuildConfig::new(&startup)));
    runtime.selected_at = Some(anchor());
    runtime.generation = 7;
    runtime.prepared_stats.batches = 1;
    let mut app = App::new();
    app.insert_resource(startup)
        .insert_resource(CameraWorldPosition(anchor()))
        .insert_resource(RenderOrigin::new(anchor()))
        .insert_resource(runtime)
        .init_resource::<SceneryReset>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<TerrainTiles>()
        .insert_resource(OverlayDriveEnabled(true))
        .add_systems(Update, drive_overlay_cut.before(stream))
        .add_systems(Update, stream);
    app
}

#[derive(Resource)]
struct OverlayDriveEnabled(bool);

fn drive_overlay_cut(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
    origin: Res<RenderOrigin>,
    enabled: Res<OverlayDriveEnabled>,
) {
    if !enabled.0 {
        return;
    }
    if tiles.is_stitching() {
        flightsim_render::terrain_stitching::advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            1,
            None,
        );
    } else if tiles.overlay_usage().dirty {
        flightsim_render::terrain_stitching::apply_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            flightsim_render::TerrainUpdate::default(),
            1,
            None,
        );
    }
}

fn finish_scene(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        if runtime.pending.is_none()
            && runtime.prepared.is_empty()
            && runtime.staged.is_empty()
            && runtime.overlay_swap.is_none()
            && !runtime.ready_to_admit
            && !app.world().resource::<TerrainTiles>().is_stitching()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "scenery/terrain transaction did not finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn owned_mesh(world: &mut World, staged: bool) -> (Entity, Handle<Mesh>) {
    let handle = world.resource_mut::<Assets<Mesh>>().add(batch().mesh);
    let entity = world
        .spawn((
            Mesh3d(handle.clone()),
            if staged {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        ))
        .id();
    let pair = (entity, handle);
    let mut runtime = world.resource_mut::<SceneryRuntime>();
    if staged {
        runtime.staged.push(pair.clone());
    } else {
        runtime.visible.push(pair.clone());
    }
    pair
}

fn ready_build(result: BuildResult, generation: u64, cancelled: bool) -> PendingBuild {
    let task = AsyncComputeTaskPool::get().spawn(async move { result });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !task.is_finished() {
        assert!(Instant::now() < deadline, "fixture worker did not finish");
        std::thread::yield_now();
    }
    PendingBuild {
        generation,
        task,
        cancel: Arc::new(AtomicBool::new(cancelled)),
    }
}

fn result(count: usize) -> BuildResult {
    BuildResult {
        meshes: (0..count).map(|_| batch()).collect(),
        statistics: BuildStatistics {
            batches: count,
            ..default()
        },
    }
}

/// Deliberately leave statistics at zero: upload accounting must inspect actual
/// mesh vertices, not trust preparation telemetry. Repeated valid triangles keep
/// the fixture's geometry cheap without weakening the ownership/count check.
fn counted_batch(vertices: usize, ground: bool) -> SceneryMesh {
    assert_eq!(vertices % 3, 0);
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    let triangle = [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        (0..vertices).map(|i| triangle[i % 3]).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; vertices]);
    mesh.insert_indices(bevy::mesh::Indices::U32(
        (0..u32::try_from(vertices).unwrap()).collect(),
    ));
    let mut batch = SceneryMesh {
        origin: anchor().to_ecef(),
        mesh,
        ground: None,
        statistics: SceneryMeshStatistics::default(),
    };
    if ground {
        let mut mesh = std::mem::replace(
            &mut batch.mesh,
            Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            ),
        );
        let lifts = vec![0.1; vertices];
        mesh.insert_attribute(ATTRIBUTE_OVERLAY_LIFT, lifts.clone());
        let overlay = Some(TerrainOverlay::surface(&mesh, batch.origin, |_| Meters::ZERO).unwrap());
        batch.ground = Some(SceneryGroundMesh {
            mesh,
            lifts,
            overlay,
        });
    }
    batch
}

fn owned_vertex_count(app: &App) -> usize {
    app.world()
        .resource::<Assets<Mesh>>()
        .iter()
        .map(|(_, mesh)| mesh.count_vertices())
        .sum()
}

#[test]
fn large_batches_split_updates_using_actual_vertices_not_statistics() {
    let mut app = fixture_app();
    app.world_mut().resource_mut::<SceneryRuntime>().prepared =
        VecDeque::from([counted_batch(40_002, false), counted_batch(40_002, false)]);
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(runtime.prepared.len(), 1);
    assert_eq!(runtime.staged.len(), 1);
    assert!(!runtime.ready_to_admit);
    assert!(runtime.overlay_swap.is_none());
    assert_eq!(owned_vertex_count(&app), 40_002);
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.prepared.is_empty());
    assert_eq!(runtime.staged.len(), 2);
    assert!(runtime.ready_to_admit);
    assert!(runtime.overlay_swap.is_none());
    assert_eq!(owned_vertex_count(&app), 80_004);
}

#[test]
fn solid_and_ground_uploads_share_one_exact_vertex_budget_in_either_order() {
    for ground_first in [false, true] {
        let mut app = fixture_app();
        app.world_mut().resource_mut::<SceneryRuntime>().prepared = VecDeque::from([
            counted_batch(32_766, ground_first),
            counted_batch(32_766, !ground_first),
            counted_batch(3, false),
        ]);
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        assert_eq!(runtime.prepared.len(), 1);
        assert_eq!(runtime.staged.len(), 1);
        assert_eq!(runtime.staged_ground.len(), 1);
        assert!(!runtime.ready_to_admit);
        assert!(runtime.overlay_swap.is_none());
        assert_eq!(owned_vertex_count(&app), 65_532);
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        assert!(runtime.prepared.is_empty());
        assert_eq!(runtime.staged.len(), 2);
        assert_eq!(runtime.staged_ground.len(), 1);
        assert!(runtime.ready_to_admit);
        assert!(runtime.overlay_swap.is_none());
        assert_eq!(owned_vertex_count(&app), 65_535);
    }
}

#[test]
fn rejected_batches_count_toward_eight_attempts_without_blocking_valid_uploads() {
    let mut app = fixture_app();
    let mut prepared = VecDeque::new();
    for _ in 0..2 {
        prepared.push_back(counted_batch(0, false));
        let mut unprepared = ground_batch();
        unprepared.ground.as_mut().unwrap().overlay = None;
        prepared.push_back(unprepared);
        prepared.push_back(counted_batch(65_535, false));
    }
    prepared.push_back(counted_batch(3, false));
    prepared.push_back(counted_batch(0, false));
    prepared.push_back(counted_batch(6, false));
    app.world_mut().resource_mut::<SceneryRuntime>().prepared = prepared;
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(
        runtime.prepared.len(),
        1,
        "all eight attempts must be charged"
    );
    assert_eq!(
        runtime.staged.len(),
        1,
        "invalid entries must not block the seventh entry"
    );
    assert!(runtime.staged_ground.is_empty());
    assert!(runtime.prepared_stats.truncated);
    assert!(!runtime.ready_to_admit);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    assert_eq!(owned_vertex_count(&app), 3);
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.prepared.is_empty());
    assert_eq!(runtime.staged.len(), 2);
    assert!(runtime.ready_to_admit);
    assert!(runtime.overlay_swap.is_none());
    assert_eq!(owned_vertex_count(&app), 9);
}

#[test]
fn completed_worker_stages_eight_small_batches_then_one_and_commits_atomically() {
    let mut app = fixture_app();
    let old = owned_mesh(app.world_mut(), false);
    app.world_mut().resource_mut::<SceneryRuntime>().pending =
        Some(ready_build(result(9), 7, false));
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.pending.is_none());
    assert_eq!(runtime.prepared.len(), 1);
    assert_eq!(runtime.staged.len(), 8);
    assert_eq!(runtime.visible, vec![old.clone()]);
    let staged = runtime.staged[0].0;
    assert_eq!(
        app.world().get::<Visibility>(staged),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        app.world().get::<Visibility>(old.0),
        Some(&Visibility::Inherited)
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 9);
    app.update();
    // Upload completion alone does not expose solid meshes. Even an empty ground
    // cohort waits for its registry revision to commit with the terrain cut.
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.prepared.is_empty());
    assert_eq!(runtime.staged.len(), 9);
    assert_eq!(runtime.visible, vec![old.clone()]);
    assert!(runtime.ready_to_admit);
    assert!(
        runtime.overlay_swap.is_none(),
        "admission must be deferred after uploads"
    );
    finish_scene(&mut app);
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.prepared.is_empty() && runtime.staged.is_empty());
    assert_eq!(runtime.visible.len(), 9);
    for (entity, _) in &runtime.visible {
        assert_eq!(
            app.world().get::<Visibility>(*entity),
            Some(&Visibility::Inherited)
        );
    }
    assert!(app.world().get_entity(old.0).is_err());
    assert!(!app.world().resource::<Assets<Mesh>>().contains(old.1.id()));
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 9);
}

#[test]
fn stale_generation_and_cancelled_worker_results_never_upload() {
    for (generation, cancelled) in [(6, false), (7, true)] {
        let mut app = fixture_app();
        let old = owned_mesh(app.world_mut(), false);
        app.world_mut().resource_mut::<SceneryRuntime>().pending =
            Some(ready_build(result(2), generation, cancelled));
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        assert!(
            runtime.pending.is_none() && runtime.prepared.is_empty() && runtime.staged.is_empty()
        );
        assert_eq!(runtime.visible, vec![old]);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    }
}

#[test]
fn empty_completed_region_removes_previous_entities_and_assets() {
    let mut app = fixture_app();
    let old = owned_mesh(app.world_mut(), false);
    app.world_mut().resource_mut::<SceneryRuntime>().pending =
        Some(ready_build(result(0), 7, false));
    app.update();
    assert_eq!(
        app.world().resource::<SceneryRuntime>().visible,
        vec![old.clone()]
    );
    finish_scene(&mut app);
    let runtime = app.world().resource::<SceneryRuntime>();
    assert!(runtime.visible.is_empty() && runtime.staged.is_empty() && runtime.prepared.is_empty());
    assert!(app.world().get_entity(old.0).is_err());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn relocation_cancels_inflight_work_and_drops_visible_staged_and_cpu_assets() {
    let mut app = fixture_app();
    let old = owned_mesh(app.world_mut(), false);
    let staged = owned_mesh(app.world_mut(), true);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut runtime = app.world_mut().resource_mut::<SceneryRuntime>();
        runtime.prepared.push_back(batch());
        runtime.pending = Some(PendingBuild {
            generation: 7,
            cancel: Arc::clone(&cancel),
            task: AsyncComputeTaskPool::get().spawn(std::future::pending()),
        });
    }
    clear(app.world_mut());
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(runtime.generation, 8);
    assert!(runtime.selected_at.is_none());
    assert!(runtime.visible.is_empty() && runtime.staged.is_empty() && runtime.prepared.is_empty());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(app.world().get_entity(old.0).is_err());
    assert!(app.world().get_entity(staged.0).is_err());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    // A late completion from the old observer must also be ignored after clear.
    app.world_mut().resource_mut::<SceneryRuntime>().selected_at = Some(anchor());
    app.world_mut().resource_mut::<SceneryRuntime>().pending =
        Some(ready_build(result(2), 7, false));
    app.update();
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn reset_revision_discards_partial_upload_before_polling_ready_old_worker() {
    let mut app = fixture_app();
    let old = owned_mesh(app.world_mut(), false);
    let staged = owned_mesh(app.world_mut(), true);
    let pending = ready_build(result(2), 7, false);
    let cancel = Arc::clone(&pending.cancel);
    {
        let mut runtime = app.world_mut().resource_mut::<SceneryRuntime>();
        runtime.prepared.push_back(batch());
        runtime.pending = Some(pending);
    }
    app.world_mut().resource_mut::<SceneryReset>().request();
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(runtime.reset_revision, 1);
    assert!(runtime.generation > 7);
    assert!(runtime.visible.is_empty() && runtime.staged.is_empty() && runtime.prepared.is_empty());
    assert!(cancel.load(Ordering::Relaxed));
    assert!(app.world().get_entity(old.0).is_err());
    assert!(app.world().get_entity(staged.0).is_err());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn full_old_and_new_scenes_keep_resident_assets_within_transition_bound() {
    let mut app = fixture_app();
    for _ in 0..MAX_BATCHES {
        owned_mesh(app.world_mut(), false);
    }
    {
        let mut runtime = app.world_mut().resource_mut::<SceneryRuntime>();
        runtime.prepared = (0..MAX_BATCHES).map(|_| batch()).collect();
        runtime.prepared_stats.batches = MAX_BATCHES;
    }
    for uploaded in (8..=MAX_BATCHES).step_by(8) {
        let remaining = MAX_BATCHES - uploaded;
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        assert_eq!(runtime.prepared.len(), remaining);
        assert!(runtime.visible.len() <= MAX_BATCHES);
        assert!(runtime.staged.len() <= MAX_BATCHES);
        assert!(runtime.visible.len() + runtime.staged.len() <= MAX_BATCHES * 2);
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            runtime.visible.len() + runtime.staged.len()
        );
    }
    assert_eq!(
        app.world().resource::<Assets<Mesh>>().len(),
        MAX_BATCHES * 2
    );
    finish_scene(&mut app);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), MAX_BATCHES);
    clear(app.world_mut());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn dense_selection_clips_before_geometry_and_keeps_scene_budgets() {
    let database = database((0..5_000).map(building).collect());
    let result = build_scene(&database, &startup(), anchor(), &AtomicBool::new(false));
    let s = result.statistics;
    assert_eq!(s.selected, MAX_SELECTED);
    assert!(s.truncated);
    assert!(s.batches > 0 && s.batches <= MAX_BATCHES);
    assert!(s.vertices > 0 && s.vertices <= MAX_VERTICES);
    assert!(s.trees <= MAX_TREES && s.tree_candidates <= MAX_TREE_CANDIDATES);
    assert_eq!(result.meshes.len(), s.batches);
    assert_eq!(
        result
            .meshes
            .iter()
            .map(|m| m.statistics.vertices)
            .sum::<usize>(),
        s.vertices
    );
    for mesh in result.meshes {
        assert!(mesh.statistics.vertices <= flightsim_render::scenery::MAX_BATCH_VERTICES);
    }
}

#[test]
fn outside_coverage_and_precancelled_builds_have_no_geometry() {
    let database = database(vec![building(0)]);
    for (observer, cancelled) in [
        (Geodetic::from_degrees(-30.0, 130.0, 0.0), false),
        (anchor(), true),
    ] {
        let result = build_scene(&database, &startup(), observer, &AtomicBool::new(cancelled));
        assert!(result.meshes.is_empty());
        assert_eq!(result.statistics.vertices, 0);
        assert_eq!(result.statistics.batches, 0);
    }
}

#[test]
fn source_order_cannot_change_runtime_batch_order_or_geometry() {
    let forward: Vec<_> = (0..200).map(building).collect();
    let mut reverse = forward.clone();
    reverse.reverse();
    let cancelled = AtomicBool::new(false);
    let a = build_scene(&database(forward), &startup(), anchor(), &cancelled);
    let b = build_scene(&database(reverse), &startup(), anchor(), &cancelled);
    assert_eq!(a.meshes.len(), b.meshes.len());
    assert_eq!(a.statistics.vertices, b.statistics.vertices);
    for (a, b) in a.meshes.iter().zip(&b.meshes) {
        assert_eq!(a.origin, b.origin);
        assert_eq!(a.statistics, b.statistics);
        for attribute in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_COLOR,
        ] {
            assert_eq!(
                format!("{:?}", a.mesh.attribute(attribute)),
                format!("{:?}", b.mesh.attribute(attribute))
            );
        }
        assert!(matches!(
            a.mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(VertexAttributeValues::Float32x3(_))
        ));
    }
}

fn empty_exclusions() -> Exclusions {
    Exclusions::new(&[], &startup())
}

fn assert_index_bounds(exclusions: &Exclusions) {
    assert!(exclusions.index_refs <= MAX_EXCLUSION_INDEX_REFS);
    assert_eq!(
        exclusions.index.values().map(Vec::len).sum::<usize>(),
        exclusions.index_refs
    );
    assert!(
        exclusions
            .index
            .values()
            .all(|refs| refs.len() <= MAX_EXCLUSION_CELL_REFS)
    );
    assert!(exclusions.broad.len() <= MAX_BROAD_EXCLUSIONS);
    assert!(exclusions.segments.len() <= MAX_EXCLUSION_SEGMENTS);
    assert!(exclusions.spheres.len() <= MAX_EXCLUSION_SPHERES);
}

#[test]
fn crowded_exclusion_cell_fails_closed_at_its_reference_limit() {
    let mut exclusions = empty_exclusions();
    let p = point(50.0, 50.0);
    for _ in 0..=MAX_EXCLUSION_CELL_REFS {
        exclusions.add_sphere(&[p], 0.1);
    }
    assert_index_bounds(&exclusions);
    assert!(exclusions.suppress_all);
    assert!(
        exclusions.contains(point(10_000.0, 10_000.0)),
        "overflow must not place unchecked trees elsewhere"
    );
}

#[test]
fn broad_and_total_spatial_index_reference_caps_fail_closed() {
    let mut broad = empty_exclusions();
    for _ in 0..=MAX_BROAD_EXCLUSIONS {
        broad.add_sphere(&[point(0.0, 0.0), point(8_000.0, 0.0)], 5.0);
    }
    assert_index_bounds(&broad);
    assert!(broad.suppress_all);
    let mut indexed = empty_exclusions();
    for id in 0..=MAX_EXCLUSION_INDEX_REFS / 512 {
        // Eight cells on every axis, with disjoint x slabs: exactly 512 refs per insert.
        let x = f64::from(u32::try_from(id).unwrap()) * EXCLUSION_CELL_METRES * 10.0;
        indexed.insert_bounds(
            bevy::math::DVec3::new(x + 1.0, 1.0, 1.0),
            bevy::math::DVec3::new(x + 1.0, 1.0, 1.0)
                + bevy::math::DVec3::splat(EXCLUSION_CELL_METRES * 7.0),
            ExclusionRef::Sphere(0),
        );
    }
    assert_index_bounds(&indexed);
    assert_eq!(indexed.index_refs, MAX_EXCLUSION_INDEX_REFS);
    assert!(indexed.suppress_all);
    assert!(indexed.contains(anchor()));
}

#[test]
fn indexed_exclusion_lookup_matches_exhaustive_geometric_reference() {
    let mut exclusions = empty_exclusions();
    exclusions.add_line(&[point(-600.0, 30.0), point(600.0, 30.0)], 8.0);
    exclusions.add_sphere(&[point(200.0, 200.0), point(260.0, 240.0)], 5.0);
    exclusions.add_sphere(&[point(1_000.0, 1_000.0), point(5_000.0, 1_000.0)], 5.0);
    assert!(!exclusions.suppress_all);
    assert_index_bounds(&exclusions);
    for north in -20..=40 {
        for east in -10..=20 {
            let p = point(f64::from(north) * 32.0, f64::from(east) * 32.0);
            let h = horizontal(p);
            let exhaustive = (0..exclusions.segments.len())
                .any(|i| exclusions.matches(h, ExclusionRef::Segment(i)))
                || (0..exclusions.spheres.len())
                    .any(|i| exclusions.matches(h, ExclusionRef::Sphere(i)));
            assert_eq!(
                exclusions.contains(p),
                exhaustive,
                "north={north}, east={east}"
            );
        }
    }
}

fn physics_app() -> App {
    let mut app = fixture_app();
    let startup = app.world().resource::<Startup>().clone();
    let initial = flightsim_fdm::RigidBodyState::from_geodetic(
        Geodetic::from_degrees(47.13, 9.53, 1_500.0),
        Attitude::from_degrees(0.0, 4.0, 0.0),
        Ned::new(45.0, 0.0, 0.0),
    );
    let source: BoxedSource = Box::new(MemoryTileSource::new());
    let simulation = Simulation::from_state(
        startup.aircraft.configuration(),
        initial,
        Terrain::new(source, 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    let clock = world_runtime::startup_clock(&startup);
    let conditions = recording_conditions(&startup, &clock);
    app.insert_resource(FlightSimulation(simulation))
        .insert_resource(StartCondition::InFlight(initial))
        .insert_resource(FlightRecorder(flightsim_sim::Recorder::new(conditions)))
        .insert_resource(Time::<()>::default())
        .insert_resource(clock)
        .insert_resource(world_runtime::initial_controls(&startup))
        .init_resource::<SampledPilotInput>()
        .init_resource::<flightsim_ui::Paused>()
        .init_resource::<CameraRig>()
        .init_resource::<flightsim_ui::TutorialState>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(flightsim_audio::SoundBridge(Arc::new(
            flightsim_audio::SharedSound::default(),
        )))
        .add_systems(Update, advance_simulation.before(stream));
    app
}

fn recording_bytes(app: &App) -> Vec<u8> {
    let mut bytes = Vec::new();
    app.world()
        .resource::<FlightRecorder>()
        .0
        .recording()
        .write_to(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn actual_restart_and_rewind_controls_invalidate_scenery() {
    for replay in [false, true] {
        let mut app = physics_app();
        let old = owned_mesh(app.world_mut(), false);
        let staged = owned_mesh(app.world_mut(), true);
        if replay {
            let state = *app.world().resource::<FlightSimulation>().0.state();
            app.world_mut().resource_mut::<FlightRecorder>().0.record(
                Seconds(1.0 / 120.0),
                flightsim_fdm::ControlInputs::neutral(),
                Some(&state),
            );
            let recording =
                flightsim_sim::Recording::read_from(&mut recording_bytes(&app).as_slice()).unwrap();
            app.insert_resource(ReplayPlayback::new(recording));
            app.add_systems(Update, control_replay.before(stream));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F8);
        } else {
            app.add_systems(Update, control_flight.before(stream));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyR);
        }
        app.update();
        assert_eq!(app.world().resource::<SceneryReset>().0, 1);
        assert_eq!(app.world().resource::<SceneryRuntime>().reset_revision, 1);
        assert!(app.world().get_entity(old.0).is_err());
        assert!(app.world().get_entity(staged.0).is_err());
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    }
}

#[test]
fn draw_distance_and_scenery_resets_leave_physics_and_replay_bytes_exact() {
    let mut baseline = physics_app();
    baseline
        .world_mut()
        .resource_mut::<SceneryRuntime>()
        .database = None;
    let mut detailed = physics_app();
    detailed
        .insert_resource(flightsim_render::terrain_detail::SurfaceDetailSettings { enabled: true });
    detailed.world_mut().resource_mut::<Startup>().scenery =
        Some(PathBuf::from("visual-only-fixture.fsscenery"));
    // The recorded world/aircraft identity must not gain a scenery dependency.
    assert_eq!(
        recording_conditions(
            baseline.world().resource::<Startup>(),
            baseline.world().resource::<flightsim_render::TimeOfDay>()
        ),
        recording_conditions(
            detailed.world().resource::<Startup>(),
            detailed.world().resource::<flightsim_render::TimeOfDay>()
        )
    );
    for frame in 0..120 {
        detailed
            .world_mut()
            .resource_mut::<SceneryRuntime>()
            .set_draw_distance(
                flightsim_world::draw_distance::DrawDistancePreset::ALL[frame % 3].policy(),
            );
        detailed
            .world_mut()
            .resource_mut::<flightsim_render::terrain_detail::SurfaceDetailSettings>()
            .enabled = frame % 2 == 0;
        if frame % 20 == 0 {
            let mut runtime = detailed.world_mut().resource_mut::<SceneryRuntime>();
            runtime.prepared = result(16).meshes;
            runtime.prepared_stats.batches = 16;
        }
        if frame % 29 == 0 {
            detailed
                .world_mut()
                .resource_mut::<SceneryReset>()
                .request();
        }
        for app in [&mut baseline, &mut detailed] {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_nanos(16_666_667));
            app.update();
        }
        let a = &baseline.world().resource::<FlightSimulation>().0;
        let b = &detailed.world().resource::<FlightSimulation>().0;
        assert_eq!(a.state(), b.state(), "frame {frame}");
        assert_eq!(a.elapsed(), b.elapsed());
        assert_eq!(a.ground().elevation, b.ground().elevation);
        assert_eq!(
            recording_bytes(&baseline),
            recording_bytes(&detailed),
            "recording bytes at {frame}"
        );
    }
    let expected = *baseline.world().resource::<FlightSimulation>().0.state();
    let bytes = recording_bytes(&baseline);
    for app in [&mut baseline, &mut detailed] {
        let recording = flightsim_sim::Recording::read_from(&mut bytes.as_slice()).unwrap();
        let playback = ReplayPlayback::new(recording);
        let initial = playback.initial_state();
        app.world_mut()
            .resource_mut::<FlightSimulation>()
            .0
            .restart_at(initial);
        app.insert_resource(playback);
    }
    for frame in 0..130 {
        detailed
            .world_mut()
            .resource_mut::<SceneryRuntime>()
            .set_draw_distance(
                flightsim_world::draw_distance::DrawDistancePreset::ALL[frame % 3].policy(),
            );
        if frame % 23 == 0 {
            detailed
                .world_mut()
                .resource_mut::<SceneryReset>()
                .request();
        }
        for app in [&mut baseline, &mut detailed] {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_nanos(16_666_667));
            app.update();
        }
        assert_eq!(
            baseline.world().resource::<FlightSimulation>().0.state(),
            detailed.world().resource::<FlightSimulation>().0.state()
        );
    }
    for app in [&baseline, &detailed] {
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.state(),
            &expected
        );
        assert!(app.world().resource::<ReplayPlayback>().fault.is_none());
        assert!(
            app.world()
                .resource::<ReplayPlayback>()
                .player
                .is_finished()
        );
    }
}

#[test]
fn primitive_exclusion_limits_never_expand_past_their_caps() {
    let mut segments = empty_exclusions();
    segments.segments.resize(
        MAX_EXCLUSION_SEGMENTS,
        (horizontal(anchor()), horizontal(point(1.0, 0.0)), 1.0),
    );
    segments.add_line(&[anchor(), point(2.0, 0.0)], 5.0);
    assert_index_bounds(&segments);
    assert!(segments.suppress_all && segments.contains(point(20_000.0, 20_000.0)));
    let mut spheres = empty_exclusions();
    spheres
        .spheres
        .resize(MAX_EXCLUSION_SPHERES, (horizontal(anchor()), 1.0));
    spheres.add_sphere(&[anchor()], 5.0);
    assert_index_bounds(&spheres);
    assert!(spheres.suppress_all && spheres.contains(point(20_000.0, 20_000.0)));
}

#[test]
fn rejected_forest_batches_still_consume_the_scene_candidate_budget() {
    use flightsim_world::scenery::SceneryLandCover;
    // Every land polygon exceeds the renderer's tessellation cap. A mapped
    // water polygon excludes every nearby tree without disabling vegetation via
    // selection truncation. More than 16 batches could otherwise spend >8,192
    // candidates; rejected meshes must conservatively retain their work charge.
    let count = FEATURES_PER_BATCH * 17;
    let mut land: Vec<_> = (0..count)
        .map(|id| SceneryLandCover {
            source_id: i64::try_from(id).unwrap() + 1,
            class: LandCoverClass::Forest,
            boundary: vec![
                point(-3_000.0, -3_000.0),
                point(-3_000.0, 3_000.0),
                point(3_000.0, 3_000.0),
                point(3_000.0, -3_000.0),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        })
        .collect();
    let mut water = land[0].clone();
    water.class = LandCoverClass::Water;
    water.source_id = i64::try_from(count).unwrap() + 1;
    land.push(water);
    let database = SceneryDatabase::new(
        ScenerySource {
            kind: ScenerySourceKind::Synthetic,
            name: "All rejected work still counts".into(),
            url: "urn:flightsim:test:rejected-tree-work".into(),
            input_fingerprint: 2,
        },
        vec![],
        vec![],
        land,
    )
    .unwrap();
    let result = build_scene(&database, &startup(), anchor(), &AtomicBool::new(false));
    assert_eq!(result.statistics.selected, count + 1);
    assert!(result.statistics.selected < MAX_SELECTED);
    assert!(result.meshes.is_empty());
    assert_eq!(result.statistics.vertices, 0);
    assert_eq!(result.statistics.trees, 0);
    assert_eq!(
        result.statistics.tree_candidates, MAX_TREE_CANDIDATES,
        "rejected batches must retain work statistics and stop at the scene cap"
    );
}

#[test]
fn split_ground_scene_has_exact_geometry_totals_and_separate_resident_caps() {
    use flightsim_world::scenery::{RoadClass, SceneryLandCover, SceneryRoad};
    let roads = (0..256)
        .map(|id| {
            let north = f64::from(id / 16) * 50.0;
            let east = f64::from(id % 16) * 50.0;
            SceneryRoad {
                source_id: i64::from(id) + 10_000,
                class: RoadClass::Residential,
                width: Meters(6.0),
                width_inferred: false,
                points: vec![point(north, east), point(north + 600.0, east)],
            }
        })
        .collect();
    let land = (0..64)
        .map(|id| {
            let north = f64::from(id / 8) * 200.0;
            let east = f64::from(id % 8) * 200.0;
            SceneryLandCover {
                source_id: i64::from(id) + 20_000,
                class: LandCoverClass::Forest,
                boundary: vec![
                    point(north, east),
                    point(north, east + 900.0),
                    point(north + 900.0, east + 900.0),
                    point(north + 900.0, east),
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            }
        })
        .collect();
    let database = SceneryDatabase::new(
        database(vec![building(0)]).source().clone(),
        roads,
        (0..128).map(building).collect(),
        land,
    )
    .unwrap();
    let result = build_scene(&database, &startup(), anchor(), &AtomicBool::new(false));
    let solids = result
        .meshes
        .iter()
        .filter(|batch| batch.mesh.count_vertices() > 0)
        .count();
    let grounds = result
        .meshes
        .iter()
        .filter(|batch| batch.ground.is_some())
        .count();
    let solid_vertices: usize = result
        .meshes
        .iter()
        .map(|batch| batch.mesh.count_vertices())
        .sum();
    let ground_vertices: usize = result
        .meshes
        .iter()
        .filter_map(|batch| batch.ground.as_ref())
        .map(|ground| ground.mesh.count_vertices())
        .sum();
    assert!(solids > 0 && solids <= MAX_BATCHES - MAX_GROUND_BATCHES);
    assert!(grounds > 0 && grounds <= MAX_GROUND_BATCHES);
    assert_eq!(result.meshes.len(), solids + grounds);
    assert_eq!(result.statistics.vertices, solid_vertices + ground_vertices);
    assert_eq!(result.statistics.ground_vertices, ground_vertices);
    assert!(ground_vertices <= MAX_GROUND_VERTICES);
    assert!(solid_vertices + ground_vertices <= MAX_VERTICES);
    for batch in result.meshes {
        let vertices = batch.mesh.count_vertices()
            + batch
                .ground
                .as_ref()
                .map_or(0, |ground| ground.mesh.count_vertices());
        assert!(vertices > 0 && vertices <= flightsim_render::scenery::MAX_BATCH_VERTICES);
        if let Some(ground) = batch.ground {
            assert_eq!(ground.lifts.len(), ground.mesh.count_vertices());
        }
    }
}

#[test]
fn visual_startup_flags_do_not_change_recorded_flight_conditions() {
    let (plain, plain_diagnostics) =
        parse_arguments_from(["--surface-detail", "off"].map(str::to_owned));
    let (detailed, detailed_diagnostics) = parse_arguments_from(
        ["--surface-detail", "on", "--scenery", "regional.fsscenery"].map(str::to_owned),
    );
    assert!(plain_diagnostics.0.is_empty() && detailed_diagnostics.0.is_empty());
    assert!(!plain.surface_detail && detailed.surface_detail);
    assert!(plain.scenery.is_none() && detailed.scenery.is_some());
    let clock = world_runtime::startup_clock(&plain);
    assert_eq!(
        recording_conditions(&plain, &clock),
        recording_conditions(&detailed, &clock)
    );
    for option in ["", "yes", "false", "0"] {
        let (invalid, _) = parse_arguments_from(["--surface-detail".to_owned(), option.to_owned()]);
        assert!(invalid.scenery_error.is_some());
    }
}

#[test]
#[ignore = "set FLIGHTSIM_SCENERY_FIXTURE and FLIGHTSIM_SCENERY_TILES to an optional regional sample"]
fn optional_real_region_build_reports_cpu_work_and_respects_scene_caps() {
    let path =
        std::env::var_os("FLIGHTSIM_SCENERY_FIXTURE").expect("set FLIGHTSIM_SCENERY_FIXTURE");
    let database = SceneryDatabase::read_path(std::path::Path::new(&path)).unwrap();
    let mut startup = startup();
    startup.tiles = std::env::var_os("FLIGHTSIM_SCENERY_TILES").map(PathBuf::from);
    let observer = database.bounds().center;
    let result = build_scene(&database, &startup, observer, &AtomicBool::new(false));
    let s = result.statistics;
    assert!(s.selected > 0 && s.selected <= MAX_SELECTED);
    assert!(s.batches <= MAX_BATCHES);
    assert!(s.vertices <= MAX_VERTICES);
    assert!(s.ground_vertices <= MAX_GROUND_VERTICES);
    assert!(s.trees <= MAX_TREES && s.tree_candidates <= MAX_TREE_CANDIDATES);
    let vertices: usize = result
        .meshes
        .iter()
        .map(|batch| {
            batch.mesh.count_vertices()
                + batch
                    .ground
                    .as_ref()
                    .map_or(0, |ground| ground.mesh.count_vertices())
        })
        .sum();
    assert_eq!(vertices, s.vertices);
    eprintln!(
        "optional regional CPU build: source roads={}, buildings={}, land={}, observer={:.6},{:.6}, selected={}, batches={}, vertices={}, ground_vertices={}, roads={}, buildings={}, land={}, trees={}, candidates={}, skipped={}, truncated={}, cpu_ms={:.3}; no GPU or frame-time claim",
        database.roads().len(),
        database.buildings().len(),
        database.landcover().len(),
        observer.latitude_degrees(),
        observer.longitude_degrees(),
        s.selected,
        s.batches,
        s.vertices,
        s.ground_vertices,
        s.roads,
        s.buildings,
        s.land_polygons,
        s.trees,
        s.tree_candidates,
        s.skipped_features,
        s.truncated,
        s.cpu_ms
    );
}

fn ground_batch() -> SceneryMesh {
    use flightsim_world::scenery::{RoadClass, SceneryRoad};
    let road = SceneryRoad {
        source_id: 8_001,
        class: RoadClass::Residential,
        width: Meters(6.0),
        width_inferred: false,
        points: vec![anchor(), point(80.0, 0.0)],
    };
    let mut batch = flightsim_render::scenery::scenery_mesh(
        &[SceneryFeatureRef::Road(&road)],
        anchor(),
        SceneryMeshOptions::near(anchor()),
        &mut |_| Some(Meters::ZERO),
        &|_| false,
    )
    .unwrap();
    let ground = batch.ground.as_mut().unwrap();
    ground.mesh.insert_attribute(
        flightsim_render::terrain_drape::ATTRIBUTE_OVERLAY_LIFT,
        ground.lifts.clone(),
    );
    ground.overlay = Some(
        flightsim_render::terrain_drape::TerrainOverlay::surface(
            &ground.mesh,
            batch.origin,
            |_| panic!("authored lifts should avoid callback sampling"),
        )
        .unwrap(),
    );
    batch
}

fn install_supporting_terrain(app: &mut App) {
    use bevy::ecs::world::CommandQueue;
    use flightsim_core::RenderFrame;
    use flightsim_world::{DemTile, HeightGrid, TileId};
    let world = app.world_mut();
    let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
    let mut tiles = world.remove_resource::<TerrainTiles>().unwrap();
    let frame = RenderFrame::new(anchor());
    let id = TileId::containing(12, anchor());
    let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters::ZERO));
    let mut queue = CommandQueue::default();
    {
        let mut commands = Commands::new(&mut queue, world);
        tiles.insert_prepared(flightsim_render::terrain::prepare_tile(
            &mut commands,
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &frame,
            id,
            &dem,
            None,
        ));
        flightsim_render::terrain_stitching::apply_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &frame,
            flightsim_render::TerrainUpdate {
                prepared: vec![id],
                spawned: vec![id],
                ..default()
            },
            1,
            None,
        );
    }
    queue.apply(world);
    world.insert_resource(meshes);
    world.insert_resource(tiles);
    finish_scene(app);
}

fn prepare_ground_replacement(app: &mut App) {
    let mut runtime = app.world_mut().resource_mut::<SceneryRuntime>();
    runtime.prepared = VecDeque::from([batch(), ground_batch()]);
    runtime.prepared_stats.batches = 2;
}

impl SceneryRuntime {
    /// Test-only shared setup for control and actual map-entry-point regression
    /// tests. Its eight uploaded assets and eight queued batches are observed
    /// through real assets/entities, without widening production visibility.
    pub(crate) fn partial_staging_test_app() -> App {
        let mut app = physics_app();
        install_supporting_terrain(&mut app);
        prepare_ground_replacement(&mut app);
        finish_scene(&mut app);
        {
            let mut runtime = app.world_mut().resource_mut::<Self>();
            runtime.prepared = (0..16)
                .map(|index| {
                    if index % 2 == 0 {
                        batch()
                    } else {
                        ground_batch()
                    }
                })
                .collect();
            runtime.prepared_stats.batches = 16;
        }
        app.update();
        let runtime = app.world().resource::<Self>();
        assert_eq!(runtime.prepared.len(), 8);
        assert_eq!(runtime.staged.len(), 4);
        assert_eq!(runtime.staged_ground.len(), 4);
        assert!(runtime.overlay_swap.is_none());
        assert_eq!(runtime.visible.len(), 1);
        assert_eq!(runtime.visible_ground.len(), 1);
        for (entity, _) in &runtime.visible {
            assert_eq!(
                app.world().get::<Visibility>(*entity),
                Some(&Visibility::Inherited)
            );
        }
        for (entity, _) in &runtime.staged {
            assert_eq!(
                app.world().get::<Visibility>(*entity),
                Some(&Visibility::Hidden)
            );
        }
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 11);
        app
    }

    pub(crate) fn queue_ready_test_worker(&mut self) -> Arc<AtomicBool> {
        let pending = ready_build(result(16), self.generation, false);
        let cancelled = Arc::clone(&pending.cancel);
        self.pending = Some(pending);
        cancelled
    }

    pub(crate) fn assert_test_uploads_cleared(&self) {
        assert!(self.prepared.is_empty());
        assert!(self.staged.is_empty() && self.visible.is_empty());
        assert!(self.staged_ground.is_empty());
        assert!(self.incoming_ground.is_empty() && self.visible_ground.is_empty());
        assert!(self.overlay_swap.is_none());
        assert!(!self.ready_to_admit);
    }
}

#[test]
fn clear_restart_and_rewind_cancel_eight_partially_staged_assets_before_more_uploads() {
    for key in [None, Some(KeyCode::KeyR), Some(KeyCode::F8)] {
        let mut app = SceneryRuntime::partial_staging_test_app();
        let owned: Vec<_> = {
            let runtime = app.world().resource::<SceneryRuntime>();
            runtime
                .visible
                .iter()
                .chain(&runtime.staged)
                .chain(&runtime.visible_ground)
                .cloned()
                .chain(
                    runtime
                        .staged_ground
                        .iter()
                        .map(|r| (r.entity, r.mesh.clone())),
                )
                .collect()
        };
        assert_eq!(owned.len(), 10);
        let cancelled = app
            .world_mut()
            .resource_mut::<SceneryRuntime>()
            .queue_ready_test_worker();
        if let Some(key) = key {
            if key == KeyCode::F8 {
                let state = *app.world().resource::<FlightSimulation>().0.state();
                app.world_mut().resource_mut::<FlightRecorder>().0.record(
                    Seconds(1.0 / 120.0),
                    flightsim_fdm::ControlInputs::neutral(),
                    Some(&state),
                );
                let recording =
                    flightsim_sim::Recording::read_from(&mut recording_bytes(&app).as_slice())
                        .unwrap();
                app.insert_resource(ReplayPlayback::new(recording));
                app.add_systems(Update, control_replay.before(stream));
            } else {
                app.add_systems(Update, control_flight.before(stream));
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        } else {
            clear(app.world_mut());
        }
        assert!(cancelled.load(Ordering::Relaxed));
        app.world()
            .resource::<SceneryRuntime>()
            .assert_test_uploads_cleared();
        assert_eq!(
            app.world()
                .resource::<TerrainTiles>()
                .overlay_usage()
                .optional_registered,
            0
        );
        for (entity, handle) in &owned {
            assert!(app.world().get_entity(*entity).is_err());
            assert!(!app.world().resource::<Assets<Mesh>>().contains(handle.id()));
        }
        app.world_mut().resource_mut::<OverlayDriveEnabled>().0 = true;
        finish_scene(&mut app);
        app.world()
            .resource::<SceneryRuntime>()
            .assert_test_uploads_cleared();
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            1,
            "supporting terrain only after {key:?}"
        );
    }
}

#[test]
fn ground_swap_retains_old_ground_and_solids_until_exact_terrain_revision_commits() {
    let mut app = fixture_app();
    install_supporting_terrain(&mut app);
    prepare_ground_replacement(&mut app);
    finish_scene(&mut app);
    let old_solid = app.world().resource::<SceneryRuntime>().visible[0].clone();
    let old_ground = app.world().resource::<SceneryRuntime>().visible_ground[0].clone();
    assert_eq!(
        app.world().get::<Visibility>(old_ground.0),
        Some(&Visibility::Inherited)
    );
    app.world_mut().resource_mut::<OverlayDriveEnabled>().0 = false;
    prepare_ground_replacement(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let runtime = app.world().resource::<SceneryRuntime>();
    let revision = runtime.overlay_swap.as_ref().unwrap().revision;
    assert!(
        app.world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .committed_revision
            < revision
    );
    assert_eq!(runtime.visible, vec![old_solid.clone()]);
    assert_eq!(runtime.visible_ground, vec![old_ground.clone()]);
    assert_eq!(runtime.staged.len(), 1);
    assert_eq!(runtime.incoming_ground.len(), 1);
    assert_eq!(
        app.world().get::<Visibility>(runtime.staged[0].0),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        app.world().get::<Visibility>(runtime.incoming_ground[0].0),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        app.world().get::<Visibility>(old_solid.0),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        app.world().get::<Visibility>(old_ground.0),
        Some(&Visibility::Inherited)
    );
    app.world_mut().resource_mut::<OverlayDriveEnabled>().0 = true;
    finish_scene(&mut app);
    assert!(
        app.world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .committed_revision
            >= revision
    );
    assert!(app.world().get_entity(old_solid.0).is_err());
    assert!(app.world().get_entity(old_ground.0).is_err());
    assert!(
        !app.world()
            .resource::<Assets<Mesh>>()
            .contains(old_solid.1.id())
    );
    assert!(
        !app.world()
            .resource::<Assets<Mesh>>()
            .contains(old_ground.1.id())
    );
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(runtime.visible.len(), 1);
    assert_eq!(runtime.visible_ground.len(), 1);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 3);
}

#[test]
fn draw_distance_clear_restart_and_rewind_remove_pending_and_retained_ground() {
    for (key, distance_change) in [
        (None, false),
        (Some(KeyCode::KeyR), false),
        (Some(KeyCode::F8), false),
        (None, true),
    ] {
        let mut app = physics_app();
        install_supporting_terrain(&mut app);
        prepare_ground_replacement(&mut app);
        finish_scene(&mut app);
        let old_solid = app.world().resource::<SceneryRuntime>().visible[0].clone();
        let old_ground = app.world().resource::<SceneryRuntime>().visible_ground[0].clone();
        app.world_mut().resource_mut::<OverlayDriveEnabled>().0 = false;
        prepare_ground_replacement(&mut app);
        for _ in 0..3 {
            app.update();
        }
        let runtime = app.world().resource::<SceneryRuntime>();
        assert!(runtime.overlay_swap.is_some());
        let new_solid = runtime.staged[0].clone();
        let new_ground = runtime.incoming_ground[0].clone();
        if let Some(key) = key {
            if key == KeyCode::F8 {
                let state = *app.world().resource::<FlightSimulation>().0.state();
                app.world_mut().resource_mut::<FlightRecorder>().0.record(
                    Seconds(1.0 / 120.0),
                    flightsim_fdm::ControlInputs::neutral(),
                    Some(&state),
                );
                let recording =
                    flightsim_sim::Recording::read_from(&mut recording_bytes(&app).as_slice())
                        .unwrap();
                app.insert_resource(ReplayPlayback::new(recording));
                app.add_systems(Update, control_replay.before(stream));
            } else {
                app.add_systems(Update, control_flight.before(stream));
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        } else if distance_change {
            app.world_mut()
                .resource_mut::<SceneryRuntime>()
                .set_draw_distance(
                    flightsim_world::draw_distance::DrawDistancePreset::Short.policy(),
                );
            app.update();
        } else {
            clear(app.world_mut());
        }
        let runtime = app.world().resource::<SceneryRuntime>();
        assert!(runtime.visible.is_empty() && runtime.staged.is_empty());
        assert!(
            runtime.visible_ground.is_empty()
                && runtime.incoming_ground.is_empty()
                && runtime.staged_ground.is_empty()
        );
        assert!(runtime.overlay_swap.is_none());
        assert_eq!(
            app.world()
                .resource::<TerrainTiles>()
                .overlay_usage()
                .optional_registered,
            0
        );
        for (entity, handle) in [&old_solid, &old_ground, &new_solid, &new_ground] {
            assert!(app.world().get_entity(*entity).is_err());
            assert!(!app.world().resource::<Assets<Mesh>>().contains(handle.id()));
        }
        app.world_mut().resource_mut::<OverlayDriveEnabled>().0 = true;
        finish_scene(&mut app);
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            1,
            "only the supporting terrain remains after {key:?}"
        );
    }
}

#[test]
fn rapid_draw_distance_changes_cancel_single_worker_before_any_stale_upload() {
    use flightsim_world::draw_distance::DrawDistancePreset;
    let mut app = fixture_app();
    let old = owned_mesh(app.world_mut(), false);
    let staged = owned_mesh(app.world_mut(), true);
    let pending = ready_build(result(16), 7, false);
    let cancel = Arc::clone(&pending.cancel);
    {
        let mut runtime = app.world_mut().resource_mut::<SceneryRuntime>();
        runtime.prepared.push_back(batch());
        runtime.pending = Some(pending);
        assert!(runtime.set_draw_distance(DrawDistancePreset::Long.policy()));
        assert!(runtime.set_draw_distance(DrawDistancePreset::Short.policy()));
        assert!(runtime.set_draw_distance(DrawDistancePreset::Standard.policy()));
        assert!(!runtime.set_draw_distance(DrawDistancePreset::Standard.policy()));
        assert_eq!(runtime.generation, 10);
        assert_eq!(runtime.pending.as_ref().unwrap().generation, 7);
        assert!(cancel.load(Ordering::Relaxed));
    }
    app.update();
    let runtime = app.world().resource::<SceneryRuntime>();
    assert_eq!(runtime.draw_distance, DrawDistancePolicy::default());
    assert!(runtime.visible.is_empty() && runtime.staged.is_empty() && runtime.prepared.is_empty());
    assert!(app.world().get_entity(old.0).is_err());
    assert!(app.world().get_entity(staged.0).is_err());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
}

#[test]
fn draw_distance_changes_actual_scenery_query_without_expanding_geometry_caps() {
    use flightsim_world::draw_distance::DrawDistancePreset;
    let buildings = [0.0, 3000.0, 6000.0]
        .into_iter()
        .enumerate()
        .map(|(index, north)| {
            let mut value = building(u32::try_from(index).unwrap());
            for p in &mut value.footprint {
                *p = p.offset_by(Meters(north), Meters::ZERO);
            }
            value
        })
        .collect();
    let database = database(buildings);
    let config = SceneryBuildConfig::new(&startup());
    for (preset, count) in [
        (DrawDistancePreset::Short, 1),
        (DrawDistancePreset::Standard, 2),
        (DrawDistancePreset::Long, 3),
    ] {
        let result = build_scene_config(
            &database,
            &config,
            anchor(),
            &AtomicBool::new(false),
            preset.policy(),
        );
        assert_eq!(result.statistics.selected, count);
        assert!(result.statistics.vertices <= MAX_VERTICES);
        assert!(result.meshes.len() <= MAX_BATCHES);
        assert!(result.statistics.ground_vertices <= MAX_GROUND_VERTICES);
        assert!(result.statistics.trees <= MAX_TREES);
        assert!(result.statistics.tree_candidates <= MAX_TREE_CANDIDATES);
    }
}

#[test]
fn repeated_distance_changes_keep_only_one_cancelled_inflight_worker() {
    use flightsim_world::draw_distance::DrawDistancePreset;
    let mut app = fixture_app();
    let cancel = Arc::new(AtomicBool::new(false));
    app.world_mut().resource_mut::<SceneryRuntime>().pending = Some(PendingBuild {
        generation: 7,
        cancel: Arc::clone(&cancel),
        task: AsyncComputeTaskPool::get().spawn(std::future::pending()),
    });
    for preset in DrawDistancePreset::ALL.into_iter().cycle().take(12) {
        app.world_mut()
            .resource_mut::<SceneryRuntime>()
            .set_draw_distance(preset.policy());
        app.update();
        let runtime = app.world().resource::<SceneryRuntime>();
        let pending = runtime
            .pending
            .as_ref()
            .expect("retain worker until completion");
        assert_eq!(pending.generation, 7);
        assert!(Arc::ptr_eq(&pending.cancel, &cancel));
        assert!(pending.cancel.load(Ordering::Relaxed));
        assert_eq!(runtime.draw_distance, preset.policy());
        assert!(runtime.prepared.is_empty() && runtime.staged.is_empty());
    }
}
