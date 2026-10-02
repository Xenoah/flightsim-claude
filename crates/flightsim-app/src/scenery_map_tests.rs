//! World-map relocation through the actual private transaction entry point.
use super::*;
use bevy::ecs::world::CommandQueue;
use flightsim_core::RenderFrame;
use flightsim_render::terrain_drape::TerrainOverlay;
use flightsim_render::{TerrainOverlayRegistration, TerrainUpdate};
use flightsim_world::{DemTile, HeightGrid, TileId};
use std::time::{Duration, Instant};

fn world() -> World {
    let mut world = World::new();
    let startup = Startup::default();
    world.insert_resource(scenery_runtime::SceneryRuntime::new(&startup).unwrap());
    world.insert_resource(startup);
    world.insert_resource(WorldMapActions::default());
    world.insert_resource(WorldMapState::default());
    world.insert_resource(CameraRig::default());
    world.insert_resource(TerrainTiles::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(TerrainStreaming {
        selector: LodSelector::new(
            16.0,
            720.0,
            Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        ),
        source: Box::new(MemoryTileSource::new()) as BoxedSource,
        cache: TileCache::new(1024 * 1024),
        live: flightsim_render::TerrainSelectionState::default(),
        material: Handle::default(),
    });
    world
}

fn advance(world: &mut World, initial: bool) {
    let frame = RenderFrame::new(world.resource::<Startup>().start);
    let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
    let mut tiles = world.remove_resource::<TerrainTiles>().unwrap();
    let mut queue = CommandQueue::default();
    {
        let mut commands = Commands::new(&mut queue, world);
        if initial {
            let id = TileId::containing(12, frame.anchor());
            let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters::ZERO));
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
                TerrainUpdate {
                    prepared: vec![id],
                    spawned: vec![id],
                    ..default()
                },
                1,
                None,
            );
        } else if tiles.is_stitching() {
            flightsim_render::terrain_stitching::advance_stitched_update(
                &mut commands,
                &mut meshes,
                &mut tiles,
                Handle::<StandardMaterial>::default(),
                &frame,
                1,
                None,
            );
        } else if tiles.overlay_usage().dirty {
            flightsim_render::terrain_stitching::apply_stitched_update(
                &mut commands,
                &mut meshes,
                &mut tiles,
                Handle::<StandardMaterial>::default(),
                &frame,
                TerrainUpdate::default(),
                1,
                None,
            );
        }
    }
    queue.apply(world);
    world.insert_resource(meshes);
    world.insert_resource(tiles);
}

fn finish(world: &mut World) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while world.resource::<TerrainTiles>().is_stitching()
        || world.resource::<TerrainTiles>().overlay_usage().dirty
    {
        advance(world, false);
        assert!(
            Instant::now() < deadline,
            "terrain overlay transaction did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn registration(world: &mut World) -> TerrainOverlayRegistration {
    let start = world.resource::<Startup>().start;
    let start = Geodetic::new(start.latitude, start.longitude, Meters::ZERO);
    let (mesh, origin) =
        flightsim_render::runway::runway_mesh(start, Radians::ZERO, Meters(80.0), Meters(10.0));
    let source = TerrainOverlay::surface(&mesh, origin, |_| Meters::ZERO).unwrap();
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let entity = world.spawn((Mesh3d(mesh.clone()), Visibility::Hidden)).id();
    TerrainOverlayRegistration {
        entity,
        mesh,
        source,
    }
}

#[test]
fn map_relocation_cancels_pending_ground_swap_and_removes_retained_old_assets() {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut world = world();
    advance(&mut world, true);
    finish(&mut world);
    let first = registration(&mut world);
    let first_pair = (first.entity, first.mesh.clone());
    world
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![first])
        .unwrap();
    finish(&mut world);
    assert_eq!(
        world.get::<Visibility>(first_pair.0),
        Some(&Visibility::Inherited)
    );
    let second = registration(&mut world);
    let second_pair = (second.entity, second.mesh.clone());
    let swap = world
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![second])
        .unwrap();
    assert_eq!(swap.retired, vec![first_pair.clone()]);
    advance(&mut world, false);
    assert!(world.resource::<TerrainTiles>().is_stitching());
    world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
        position: Geodetic::from_degrees(-33.95, 151.18, 0.0),
        month: 7,
    });
    apply_world_map_start(&mut world);
    assert!(world.resource::<TerrainTiles>().is_empty());
    assert!(!world.resource::<TerrainTiles>().is_stitching());
    assert_eq!(
        world
            .resource::<TerrainTiles>()
            .overlay_usage()
            .optional_registered,
        0
    );
    assert!(
        !world
            .resource::<TerrainTiles>()
            .overlay_usage()
            .optional_swap_pending
    );
    assert!(world.resource::<Assets<Mesh>>().is_empty());
    for (entity, handle) in [first_pair, second_pair] {
        assert!(world.get_entity(entity).is_err());
        assert!(!world.resource::<Assets<Mesh>>().contains(handle.id()));
    }
    // Progress the real transaction boundary after cancellation; no retired
    // target can be reintroduced by a finished old worker.
    finish(&mut world);
    assert!(world.resource::<Assets<Mesh>>().is_empty());
    let new_start = world.resource::<Startup>().start;
    assert!((new_start.latitude_degrees() + 33.95).abs() < 1e-9);
    assert!((new_start.longitude_degrees() - 151.18).abs() < 1e-9);
}

#[test]
fn map_relocation_cancels_eight_uploaded_and_eight_queued_scenery_batches() {
    let mut app = scenery_runtime::SceneryRuntime::partial_staging_test_app();
    let world = app.world_mut();
    world.resource_mut::<Startup>().world.global_terrain = true;
    world.insert_resource(WorldMapActions::default());
    world.insert_resource(WorldMapState::default());
    world.insert_resource(TerrainStreaming {
        selector: LodSelector::new(
            16.0,
            720.0,
            Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        ),
        source: Box::new(MemoryTileSource::new()) as BoxedSource,
        cache: TileCache::new(1024 * 1024),
        live: flightsim_render::TerrainSelectionState::default(),
        material: Handle::default(),
    });
    let owned: Vec<_> = world
        .query::<(Entity, &Mesh3d)>()
        .iter(world)
        .map(|(entity, mesh)| (entity, mesh.0.clone()))
        .collect();
    assert_eq!(
        owned.len(),
        11,
        "terrain, previous scene, eight staged assets"
    );
    let cancelled = world
        .resource_mut::<scenery_runtime::SceneryRuntime>()
        .queue_ready_test_worker();
    world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
        position: Geodetic::from_degrees(-33.95, 151.18, 0.0),
        month: 7,
    });
    apply_world_map_start(world);
    assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    world
        .resource::<scenery_runtime::SceneryRuntime>()
        .assert_test_uploads_cleared();
    assert!(world.resource::<Assets<Mesh>>().is_empty());
    assert!(world.resource::<TerrainTiles>().is_empty());
    assert_eq!(
        world
            .resource::<TerrainTiles>()
            .overlay_usage()
            .optional_registered,
        0
    );
    assert!(
        !world
            .resource::<TerrainTiles>()
            .overlay_usage()
            .optional_swap_pending
    );
    for (entity, handle) in &owned {
        assert!(world.get_entity(*entity).is_err());
        assert!(!world.resource::<Assets<Mesh>>().contains(handle.id()));
    }
    // Poll the cancelled ready worker and the old transaction after relocation.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        if !app
            .world()
            .resource::<scenery_runtime::SceneryRuntime>()
            .is_loading()
            && !app.world().resource::<TerrainTiles>().is_stitching()
            && !app.world().resource::<TerrainTiles>().overlay_usage().dirty
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "post-relocation work did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let world = app.world_mut();
    finish(world);
    world
        .resource::<scenery_runtime::SceneryRuntime>()
        .assert_test_uploads_cleared();
    assert!(world.resource::<Assets<Mesh>>().is_empty());
    for (entity, _) in owned {
        assert!(world.get_entity(entity).is_err());
    }
}
