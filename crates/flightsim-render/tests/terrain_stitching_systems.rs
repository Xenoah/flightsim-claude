//! Exercise the production mesh/Commands boundary, including deferred cuts.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy system parameters are values"
)]
use bevy::prelude::*;
use flightsim_core::{Degrees, Ecef, Geodetic, Meters};
use flightsim_render::{
    RenderOrigin, RenderSet, TerrainSelectionState, TerrainTiles, TerrainUpdate, WorldPosition,
    apply_world_positions,
    terrain::{TerrainTile, prepare_tile},
    terrain_stitching::{
        StitchProgress, TerrainBridge, advance_stitched_update, apply_stitched_update,
    },
    update_terrain_selection_with_surface,
};
use flightsim_world::global::{GlobalTerrain, GlobalTileSource};
use flightsim_world::{DemTile, HeightGrid, LodSelector, MemoryTileSource, TileCache, TileId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Resource, Default)]
struct Scripted {
    frames: VecDeque<(Vec<(TileId, DemTile)>, TerrainUpdate)>,
    budget: usize,
    progress: StitchProgress,
    surfaces: usize,
    surface_time: std::time::Duration,
    transaction_time: std::time::Duration,
}

fn advance_script(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut script: ResMut<Scripted>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
) {
    script.surfaces = 0;
    if tiles.is_stitching() {
        let started = std::time::Instant::now();
        script.progress = advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::default(),
            &origin.0,
            script.budget,
            None,
        );
        script.transaction_time = started.elapsed();
        return;
    }
    let Some((preparations, update)) = script.frames.pop_front() else {
        return;
    };
    script.surfaces = preparations.len();
    let started = std::time::Instant::now();
    for (id, dem) in preparations {
        tiles.insert_prepared(prepare_tile(
            &mut commands,
            &mut meshes,
            Handle::default(),
            &origin.0,
            id,
            &dem,
            None,
        ));
    }
    script.surface_time = started.elapsed();
    let started = std::time::Instant::now();
    script.progress = apply_stitched_update(
        &mut commands,
        &mut meshes,
        &mut tiles,
        Handle::default(),
        &origin.0,
        update,
        script.budget,
        None,
    );
    script.transaction_time = started.elapsed();
}

fn fixture(budget: usize) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(RenderOrigin::new(Geodetic::from_degrees(
            -5.9765625, -133.5, 12_000.0,
        )))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<TerrainTiles>()
        .insert_resource(Scripted {
            budget,
            ..Default::default()
        })
        .configure_sets(Update, (RenderSet::Transforms, RenderSet::Terrain).chain())
        .add_systems(Update, apply_world_positions.in_set(RenderSet::Transforms))
        .add_systems(Update, advance_script.in_set(RenderSet::Terrain));
    app
}

fn flat(id: TileId, height: f64) -> (TileId, DemTile) {
    (
        id,
        DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(height))),
    )
}

fn enqueue(
    app: &mut App,
    preparations: Vec<(TileId, DemTile)>,
    spawned: Vec<TileId>,
    despawned: Vec<TileId>,
    replaced: Vec<TileId>,
) {
    let prepared = preparations.iter().map(|(id, _)| *id).collect();
    app.world_mut()
        .resource_mut::<Scripted>()
        .frames
        .push_back((
            preparations,
            TerrainUpdate {
                prepared,
                spawned,
                despawned,
                replaced,
                ..Default::default()
            },
        ));
}

fn visible_tiles(app: &mut App) -> BTreeMap<TileId, Entity> {
    let mut query = app
        .world_mut()
        .query::<(Entity, &TerrainTile, &Visibility)>();
    let mut result = BTreeMap::new();
    for (entity, tile, visibility) in query.iter(app.world()) {
        if *visibility != Visibility::Hidden {
            assert!(
                result.insert(tile.0, entity).is_none(),
                "same-ID geometries were both displayed"
            );
        }
    }
    assert_eq!(
        result.keys().copied().collect::<BTreeSet<_>>(),
        app.world()
            .resource::<TerrainTiles>()
            .displayed_ids()
            .collect()
    );
    result
}

fn visible_bridges(app: &mut App) -> BTreeSet<Entity> {
    let mut query = app
        .world_mut()
        .query_filtered::<(Entity, &Visibility), With<TerrainBridge>>();
    query
        .iter(app.world())
        .filter_map(|(entity, visibility)| (*visibility != Visibility::Hidden).then_some(entity))
        .collect()
}

fn check_assets_and_budget(app: &mut App) {
    let script = app.world().resource::<Scripted>();
    assert!(script.progress.prepared + script.surfaces <= script.budget);
    assert!(
        script.progress.planning_work
            <= flightsim_render::terrain_stitching::SEAM_PLANNING_WORK_PER_FRAME
    );
    let usage = app.world().resource::<TerrainTiles>().resource_usage();
    assert_eq!(
        usage.surface_meshes + usage.bridge_meshes,
        app.world().resource::<Assets<Mesh>>().len()
    );
    assert_eq!(
        usage.boundary_vertices,
        app.world().resource::<TerrainTiles>().len() * 4 * 33
    );
    assert!(
        usage.bridge_vertices <= usage.bridge_meshes * 266,
        "fixed-resolution bridge allocation bound"
    );
    assert!(usage.boundary_bytes > 0 || usage.boundary_vertices == 0);
    assert!(usage.geometry_bytes > 0 || usage.surface_meshes + usage.bridge_meshes == 0);
    let origin = app.world().resource::<RenderOrigin>().0;
    let mut query = app.world_mut().query::<(&WorldPosition, &Transform)>();
    for (position, transform) in query.iter(app.world()) {
        assert!((transform.translation - origin.to_render(position.0)).length() < 1.0e-5);
        assert!(
            transform
                .rotation
                .dot(origin.rotation_to_render(glam::DQuat::IDENTITY))
                .abs()
                > 0.999_999
        );
    }
}

fn settle(app: &mut App) {
    for _ in 0..256 {
        app.update();
        check_assets_and_budget(app);
        if !app.world().resource::<TerrainTiles>().is_stitching()
            && app.world().resource::<Scripted>().frames.is_empty()
        {
            return;
        }
    }
    panic!("bounded preparation never committed");
}

#[test]
fn a_mixed_cut_waits_for_bridges_without_exceeding_the_shared_mesh_budget() {
    let mut app = fixture(2);
    let a = TileId::new(3, 1, 4);
    let b = TileId::new(7, 32, 68);
    enqueue(
        &mut app,
        vec![flat(a, 0.0), flat(b, 0.0)],
        vec![a, b],
        vec![],
        vec![],
    );
    app.update();
    check_assets_and_budget(&mut app);
    assert!(app.world().resource::<TerrainTiles>().is_stitching());
    assert!(visible_tiles(&mut app).is_empty());
    assert!(visible_bridges(&mut app).is_empty());
    app.update();
    check_assets_and_budget(&mut app);
    assert!(!app.world().resource::<TerrainTiles>().is_stitching());
    assert_eq!(visible_tiles(&mut app).len(), 2);
    assert_eq!(visible_bridges(&mut app).len(), 1);
}

#[test]
fn old_surface_and_bridges_survive_same_id_replacement_then_commit_together() {
    let mut app = fixture(2);
    let a = TileId::new(3, 1, 4);
    let b = TileId::new(7, 32, 68);
    enqueue(
        &mut app,
        vec![flat(a, 0.0), flat(b, 0.0)],
        vec![a, b],
        vec![],
        vec![],
    );
    settle(&mut app);
    let old_tiles = visible_tiles(&mut app);
    let old_bridges = visible_bridges(&mut app);
    app.world_mut().resource_mut::<Scripted>().budget = 1;
    enqueue(&mut app, vec![flat(b, 2_000.0)], vec![b], vec![], vec![b]);
    app.update();
    check_assets_and_budget(&mut app);
    assert_eq!(visible_tiles(&mut app), old_tiles);
    assert_eq!(visible_bridges(&mut app), old_bridges);
    assert_eq!(
        app.world()
            .resource::<TerrainTiles>()
            .resource_usage()
            .retired_surface_meshes,
        1
    );
    app.update();
    check_assets_and_budget(&mut app);
    let new_tiles = visible_tiles(&mut app);
    assert_eq!(new_tiles[&a], old_tiles[&a]);
    assert_ne!(new_tiles[&b], old_tiles[&b]);
    assert_ne!(visible_bridges(&mut app), old_bridges);
    assert!(app.world().get_entity(old_tiles[&b]).is_err());
    assert_eq!(
        app.world()
            .resource::<TerrainTiles>()
            .resource_usage()
            .retired_surface_meshes,
        0
    );
}

#[test]
fn refinement_keeps_a_complete_old_cut_through_many_bridge_frames_and_rebase() {
    let mut app = fixture(2);
    let a = TileId::new(3, 1, 4);
    let b = TileId::new(3, 2, 4);
    enqueue(
        &mut app,
        vec![flat(a, 0.0), flat(b, 500.0)],
        vec![a, b],
        vec![],
        vec![],
    );
    settle(&mut app);
    let old_tiles = visible_tiles(&mut app);
    let old_bridges = visible_bridges(&mut app);
    app.world_mut().resource_mut::<Scripted>().budget = 4;
    let children = b.children().unwrap();
    enqueue(
        &mut app,
        children.into_iter().map(|id| flat(id, 100.0)).collect(),
        children.to_vec(),
        vec![b],
        vec![],
    );
    app.update();
    assert_eq!(visible_tiles(&mut app), old_tiles);
    assert_eq!(visible_bridges(&mut app), old_bridges);
    app.world_mut().resource_mut::<Scripted>().budget = 1;
    app.world_mut()
        .insert_resource(RenderOrigin::new(Geodetic::from_degrees(
            12.0, 179.99, 500.0,
        )));
    let mut deferred_frames = 0;
    while app.world().resource::<TerrainTiles>().is_stitching() {
        app.update();
        check_assets_and_budget(&mut app);
        if app.world().resource::<TerrainTiles>().is_stitching() {
            assert_eq!(visible_tiles(&mut app), old_tiles);
            assert_eq!(visible_bridges(&mut app), old_bridges);
            deferred_frames += 1;
        }
    }
    assert!(deferred_frames >= 3);
    assert_eq!(
        visible_tiles(&mut app)
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        std::iter::once(a).chain(children).collect()
    );
    assert!(app.world().get_entity(old_tiles[&b]).is_err());
}

fn clear_terrain(app: &mut App) {
    let mut tiles = app.world_mut().remove_resource::<TerrainTiles>().unwrap();
    for (entity, mesh) in tiles.drain_all() {
        app.world_mut().entity_mut(entity).despawn();
        app.world_mut().resource_mut::<Assets<Mesh>>().remove(&mesh);
    }
    app.world_mut().insert_resource(tiles);
}

#[test]
fn cancel_for_new_flight_releases_visible_pending_and_same_id_retired_assets() {
    let mut app = fixture(4);
    let children = TileId::new(3, 1, 4).children().unwrap();
    enqueue(
        &mut app,
        children.into_iter().map(|id| flat(id, 0.0)).collect(),
        children.to_vec(),
        vec![],
        vec![],
    );
    settle(&mut app);
    app.world_mut().resource_mut::<Scripted>().budget = 1;
    enqueue(
        &mut app,
        vec![flat(children[0], -200.0)],
        vec![children[0]],
        vec![],
        vec![children[0]],
    );
    app.update();
    app.update(); // One new bridge is hidden; several remain to prepare.
    assert!(app.world().resource::<TerrainTiles>().is_stitching());
    let usage = app.world().resource::<TerrainTiles>().resource_usage();
    assert_eq!(usage.retired_surface_meshes, 1);
    assert!(usage.bridge_meshes > usage.visible_bridges);
    clear_terrain(&mut app);
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(app.world().resource::<TerrainTiles>().is_empty());
    assert!(!app.world().resource::<TerrainTiles>().is_stitching());
    assert!(visible_tiles(&mut app).is_empty());
    assert!(visible_bridges(&mut app).is_empty());
    assert_eq!(
        app.world().resource::<TerrainTiles>().resource_usage(),
        Default::default()
    );
    // A second world starts cleanly, including generations and displayed IDs.
    app.world_mut().resource_mut::<Scripted>().budget = 4;
    enqueue(
        &mut app,
        children.into_iter().map(|id| flat(id, 42.0)).collect(),
        children.to_vec(),
        vec![],
        vec![],
    );
    settle(&mut app);
    assert_eq!(visible_tiles(&mut app).len(), 4);
}

#[test]
fn repeated_source_replacements_reuse_unchanged_seams_without_growing_residency() {
    let mut app = fixture(3);
    let a = TileId::new(5, 8, 16);
    let b = TileId::new(5, 9, 16);
    let c = TileId::new(5, 10, 16);
    enqueue(
        &mut app,
        vec![flat(a, 0.0), flat(b, 0.0), flat(c, 0.0)],
        vec![a, b, c],
        vec![],
        vec![],
    );
    settle(&mut app);
    let old_tiles = visible_tiles(&mut app);
    let mut query = app.world_mut().query::<(Entity, &TerrainBridge)>();
    let unchanged_bridge = query
        .iter(app.world())
        .find_map(|(entity, bridge)| {
            (bridge.0.first == b && bridge.0.second == c).then_some(entity)
        })
        .unwrap();
    app.world_mut().resource_mut::<Scripted>().budget = 1;
    for index in 0..20 {
        enqueue(
            &mut app,
            vec![flat(a, f64::from(index) * 100.0 - 500.0)],
            vec![a],
            vec![],
            vec![a],
        );
        settle(&mut app);
        let current = visible_tiles(&mut app);
        assert_eq!(current[&b], old_tiles[&b]);
        assert_eq!(current[&c], old_tiles[&c]);
        assert!(visible_bridges(&mut app).contains(&unchanged_bridge));
        let usage = app.world().resource::<TerrainTiles>().resource_usage();
        assert_eq!(usage.surface_meshes, 3);
        assert_eq!(usage.bridge_meshes, 2);
        assert_eq!(usage.retired_surface_meshes, 0);
        assert_eq!(usage.queued_bridges, 0);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 5);
    }
}

#[derive(Resource)]
struct Automatic {
    source: GlobalTileSource<MemoryTileSource>,
    selector: LodSelector,
    cache: TileCache,
    state: TerrainSelectionState,
    camera: Ecef,
    budget: usize,
    reads: usize,
    surfaces: usize,
    progress: StitchProgress,
}

fn stream_automatic(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut streaming: ResMut<Automatic>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
) {
    let streaming = &mut *streaming;
    streaming.reads = 0;
    streaming.surfaces = 0;
    if tiles.is_stitching() {
        streaming.progress = advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::default(),
            &origin.0,
            streaming.budget,
            None,
        );
        return;
    }
    let mut preparations = Vec::new();
    let update = update_terrain_selection_with_surface(
        &streaming.selector,
        &streaming.source,
        &mut streaming.cache,
        &mut streaming.state,
        streaming.camera,
        Meters::ZERO,
        streaming.budget,
        &mut |id, dem| {
            preparations.push(prepare_tile(
                &mut commands,
                &mut meshes,
                Handle::default(),
                &origin.0,
                id,
                dem,
                None,
            ));
        },
    );
    streaming.reads = update.load_attempts;
    streaming.surfaces = update.prepared.len();
    for tile in preparations {
        tiles.insert_prepared(tile);
    }
    streaming.progress = apply_stitched_update(
        &mut commands,
        &mut meshes,
        &mut tiles,
        Handle::default(),
        &origin.0,
        update,
        streaming.budget,
        None,
    );
}

#[test]
fn actual_default_mixed_cut_reaches_completion_with_one_tile_cache_and_budget_one() {
    let camera = Geodetic::from_degrees(-5.9765625, -133.5, 12_000.0);
    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    );
    let desired: BTreeSet<_> = selector
        .select_with_surface(camera.to_ecef(), Meters::ZERO)
        .tiles
        .into_iter()
        .collect();
    assert!(desired.contains(&TileId::new(3, 1, 4)));
    assert!(desired.contains(&TileId::new(7, 32, 68)));
    let atlas = GlobalTerrain::bundled().unwrap();
    let cache_bytes = atlas.tile(TileId::roots()[0]).unwrap().memory_footprint();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(RenderOrigin::new(camera))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<TerrainTiles>()
        .insert_resource(Automatic {
            source: GlobalTileSource::new(MemoryTileSource::new(), atlas),
            selector,
            cache: TileCache::new(cache_bytes),
            state: TerrainSelectionState::default(),
            camera: camera.to_ecef(),
            budget: 1,
            reads: 0,
            surfaces: 0,
            progress: StitchProgress::default(),
        })
        .configure_sets(Update, (RenderSet::Transforms, RenderSet::Terrain).chain())
        .add_systems(Update, apply_world_positions.in_set(RenderSet::Transforms))
        .add_systems(Update, stream_automatic.in_set(RenderSet::Terrain));
    let mut ever_complete = false;
    for frame in 0..4_096 {
        app.update();
        let streaming = app.world().resource::<Automatic>();
        assert!(streaming.reads <= streaming.budget);
        assert!(streaming.surfaces + streaming.progress.prepared <= streaming.budget);
        assert!(streaming.state.resident_len() <= 8_192);
        let displayed: BTreeSet<_> = app
            .world()
            .resource::<TerrainTiles>()
            .displayed_ids()
            .collect();
        let area: f64 = displayed
            .iter()
            .map(|id| 0.5 * 0.25_f64.powi(i32::from(id.level)))
            .sum();
        if area >= 1.0 - 1.0e-10 {
            ever_complete = true;
        }
        if ever_complete {
            assert!(
                (area - 1.0).abs() < 1.0e-10,
                "coverage lost at frame {frame}"
            );
        }
        let usage = app.world().resource::<TerrainTiles>().resource_usage();
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            usage.surface_meshes + usage.bridge_meshes
        );
        assert!(usage.bridge_meshes <= 8 * 8_192);
        if displayed == desired && !app.world().resource::<TerrainTiles>().is_stitching() {
            assert!(ever_complete);
            assert!(usage.visible_bridges > 0);
            return;
        }
    }
    panic!("mesh-budget-one selection/stitching failed to converge");
}

#[test]
fn planning_is_frame_bounded_preserves_old_cut_and_survives_rebase() {
    use flightsim_render::terrain_stitching::SEAM_PLANNING_WORK_PER_FRAME;
    let mut app = fixture(512);
    let roots = TileId::roots();
    enqueue(
        &mut app,
        roots.into_iter().map(|id| flat(id, 0.0)).collect(),
        roots.to_vec(),
        vec![],
        vec![],
    );
    settle(&mut app);
    let old_tiles = visible_tiles(&mut app);
    let old_bridges = visible_bridges(&mut app);
    let target: Vec<_> = (0..TileId::rows(4))
        .flat_map(|y| (0..TileId::columns(4)).map(move |x| TileId::new(4, x, y)))
        .collect();
    enqueue(
        &mut app,
        target.iter().map(|&id| flat(id, 700.0)).collect(),
        target.clone(),
        roots.to_vec(),
        vec![],
    );
    app.update();
    assert!(app.world().resource::<Scripted>().progress.planning_pending);
    assert_eq!(visible_tiles(&mut app), old_tiles);
    assert_eq!(visible_bridges(&mut app), old_bridges);
    app.world_mut()
        .insert_resource(RenderOrigin::new(Geodetic::from_degrees(
            -89.9, -179.99, 900.0,
        )));
    // Even zero mesh budget cannot starve topology planning. No source update
    // can be consumed while the immutable transaction is in progress.
    app.world_mut().resource_mut::<Scripted>().budget = 0;
    enqueue(
        &mut app,
        vec![flat(target[0], -2_000.0)],
        vec![target[0]],
        vec![],
        vec![target[0]],
    );
    let mut frames = 1;
    while app.world().resource::<Scripted>().progress.planning_pending {
        app.update();
        frames += 1;
        let script = app.world().resource::<Scripted>();
        assert!(script.progress.planning_work > 0);
        assert!(script.progress.planning_work <= SEAM_PLANNING_WORK_PER_FRAME);
        assert_eq!(script.progress.prepared, 0);
        assert_eq!(script.frames.len(), 1);
        assert!(frames < 64, "bounded topology planning starved");
        assert_eq!(visible_tiles(&mut app), old_tiles);
        assert_eq!(visible_bridges(&mut app), old_bridges);
        check_assets_and_budget(&mut app);
        let usage = app.world().resource::<TerrainTiles>().resource_usage();
        assert!(usage.planning.indexed_edges <= 4 * target.len());
        assert!(usage.planning.adjacent_pairs <= 4 * target.len());
        assert!(usage.planning.corners <= 8 * target.len() + 2);
        assert!(usage.planned_bridges <= 4 * target.len());
    }
    assert!(frames > 2);
    assert!(app.world().resource::<TerrainTiles>().is_stitching());
    app.world_mut().resource_mut::<Scripted>().budget = 64;
    settle(&mut app); // Also commits the queued same-ID source replacement.
    assert_eq!(
        visible_tiles(&mut app)
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        target.into_iter().collect()
    );
    let usage = app.world().resource::<TerrainTiles>().resource_usage();
    assert_eq!(usage.planning, Default::default());
    assert_eq!(usage.planned_bridges, 0);
    assert_eq!(usage.retired_surface_meshes, 0);
}

#[test]
fn cancelling_during_topology_planning_drains_scratch_and_retired_sources() {
    let mut app = fixture(512);
    let target: Vec<_> = (0..TileId::rows(4))
        .flat_map(|y| (0..TileId::columns(4)).map(move |x| TileId::new(4, x, y)))
        .collect();
    enqueue(
        &mut app,
        target.iter().map(|&id| flat(id, 0.0)).collect(),
        target.clone(),
        vec![],
        vec![],
    );
    settle(&mut app);
    for cancelled_frame in [0, 2, 4] {
        let replaced = target[0];
        enqueue(
            &mut app,
            vec![flat(replaced, -3_000.0)],
            vec![replaced],
            vec![],
            vec![replaced],
        );
        app.update();
        for _ in 0..cancelled_frame {
            app.update();
        }
        assert!(app.world().resource::<Scripted>().progress.planning_pending);
        assert_eq!(
            app.world()
                .resource::<TerrainTiles>()
                .resource_usage()
                .retired_surface_meshes,
            1
        );
        clear_terrain(&mut app);
        assert_eq!(
            app.world().resource::<TerrainTiles>().resource_usage(),
            Default::default()
        );
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(visible_tiles(&mut app).is_empty());
        assert!(visible_bridges(&mut app).is_empty());
        // Restart at a different pole while retaining no cancelled source state.
        app.world_mut()
            .insert_resource(RenderOrigin::new(Geodetic::from_degrees(
                89.9, 179.99, 1_000.0,
            )));
        enqueue(
            &mut app,
            target.iter().map(|&id| flat(id, 123.0)).collect(),
            target.clone(),
            vec![],
            vec![],
        );
        settle(&mut app);
    }
}

/// An explicit timing probe, never a wall-time assertion or an ordinary test.
/// Use only in a coordinated quiet window with --ignored --exact --nocapture.
/// Production Commands/Assets paths are exercised without a GPU; final commit
/// includes its final single bridge, and full update includes deferred Commands.
#[test]
#[ignore = "manual quiet-window latency evidence; allocates worst-cut real meshes"]
fn measure_bounded_planning_transaction_costs() {
    use std::time::Instant;
    let selector = LodSelector::new(
        16.0,
        1_080.0,
        Degrees(60.0).to_radians(),
        13,
        Meters(20_000.0),
    );
    for label in ["north_pole", "uniform_l6_8192"] {
        let target: Vec<_> = if label == "north_pole" {
            selector
                .select_with_surface(
                    Geodetic::from_degrees(89.999, 30.0, 1_000.0).to_ecef(),
                    Meters::ZERO,
                )
                .tiles
        } else {
            (0..TileId::rows(6))
                .flat_map(|y| (0..TileId::columns(6)).map(move |x| TileId::new(6, x, y)))
                .collect()
        };
        let mut app = fixture(target.len());
        enqueue(
            &mut app,
            target.iter().map(|&id| flat(id, 100.0)).collect(),
            target.clone(),
            vec![],
            vec![],
        );
        let started = Instant::now();
        app.update();
        let initial_update = started.elapsed();
        let script = app.world().resource::<Scripted>();
        println!(
            "render_probe={label} tiles={} setup_surfaces_ms={:.3} target_assembly_plus_first_chunk_ms={:.3} initial_full_update_ms={:.3}",
            target.len(),
            script.surface_time.as_secs_f64() * 1e3,
            script.transaction_time.as_secs_f64() * 1e3,
            initial_update.as_secs_f64() * 1e3
        );
        let mut planning_calls = Vec::new();
        let mut planning_updates = Vec::new();
        let mut staging_frames = 1;
        app.world_mut().resource_mut::<Scripted>().budget = 0;
        while app.world().resource::<Scripted>().progress.planning_pending {
            let started = Instant::now();
            app.update();
            planning_updates.push(started.elapsed());
            planning_calls.push(app.world().resource::<Scripted>().transaction_time);
            staging_frames += 1;
        }
        planning_calls.sort_unstable();
        planning_updates.sort_unstable();
        println!(
            "render_probe={label} planning_frames={staging_frames} chunk_max_ms={:.3} planning_full_update_max_ms={:.3}",
            planning_calls.last().unwrap().as_secs_f64() * 1e3,
            planning_updates.last().unwrap().as_secs_f64() * 1e3
        );
        // Prepare all but the final bridge before the measured atomic commit.
        app.world_mut().resource_mut::<Scripted>().budget = 256;
        while app.world().resource::<TerrainTiles>().is_stitching() {
            let remaining = app.world().resource::<Scripted>().progress.remaining;
            if remaining <= 256 {
                app.world_mut().resource_mut::<Scripted>().budget = 1;
            }
            let started = Instant::now();
            app.update();
            let elapsed = started.elapsed();
            staging_frames += 1;
            if !app.world().resource::<TerrainTiles>().is_stitching() {
                println!(
                    "render_probe={label} staging_frames={staging_frames} commit_plus_one_bridge_ms={:.3} commit_full_update_ms={:.3}",
                    app.world()
                        .resource::<Scripted>()
                        .transaction_time
                        .as_secs_f64()
                        * 1e3,
                    elapsed.as_secs_f64() * 1e3
                );
            }
        }
        assert_eq!(
            app.world()
                .resource::<TerrainTiles>()
                .displayed_ids()
                .count(),
            target.len()
        );
        // Measure replacement target assembly independently of surface building,
        // then cancel while ordinary-corner planning scratch is populated.
        enqueue(
            &mut app,
            vec![flat(target[0], -2_000.0)],
            vec![target[0]],
            vec![],
            vec![target[0]],
        );
        app.update();
        println!(
            "render_probe={label} replacement_target_plus_first_chunk_ms={:.3}",
            app.world()
                .resource::<Scripted>()
                .transaction_time
                .as_secs_f64()
                * 1e3
        );
        while app
            .world()
            .resource::<TerrainTiles>()
            .resource_usage()
            .planning
            .corners
            < target.len() / 2
        {
            assert!(app.world().resource::<Scripted>().progress.planning_pending);
            app.update();
        }
        let usage = app.world().resource::<TerrainTiles>().resource_usage();
        let started = Instant::now();
        clear_terrain(&mut app);
        println!(
            "render_probe={label} cancel_all_assets_ms={:.3} scratch_corners={} scratch_pairs={}",
            started.elapsed().as_secs_f64() * 1e3,
            usage.planning.corners,
            usage.planning.adjacent_pairs
        );
        assert_eq!(
            app.world().resource::<TerrainTiles>().resource_usage(),
            Default::default()
        );
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    }
}
