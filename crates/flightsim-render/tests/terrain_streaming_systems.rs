//! Exercise the actual Commands/Assets/visibility boundary without a GPU.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy system parameters must be passed by value"
)]
use bevy::prelude::*;
use flightsim_core::{Degrees, Ecef, Geodetic, Meters};
use flightsim_render::{
    RenderOrigin, RenderSet, TerrainSelectionState, TerrainTiles, TerrainUpdate, WorldPosition,
    apply_world_positions,
    terrain::{TerrainTile, apply_terrain_update, spawn_tile},
    update_terrain_selection,
};
use flightsim_world::{DemTile, HeightGrid, LodSelector, MemoryTileSource, TileCache, TileId};
use std::collections::BTreeSet;

#[derive(Resource)]
struct Streaming {
    selector: LodSelector,
    source: MemoryTileSource,
    cache: TileCache,
    state: TerrainSelectionState,
    camera: Ecef,
    budget: usize,
    last: TerrainUpdate,
}

fn stream(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut streaming: ResMut<Streaming>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
) {
    let streaming = &mut *streaming;
    let mut prepared = Vec::new();
    let update = update_terrain_selection(
        &streaming.selector,
        &streaming.source,
        &mut streaming.cache,
        &mut streaming.state,
        streaming.camera,
        streaming.budget,
        &mut |id, dem| {
            let (entity, mesh) = spawn_tile::<StandardMaterial>(
                &mut commands,
                &mut meshes,
                Handle::default(),
                &origin.0,
                id,
                dem,
            );
            prepared.push((id, entity, mesh));
        },
    );
    streaming.last = update.clone();
    for (id, entity, mesh) in prepared {
        tiles.insert(id, entity, mesh);
    }
    apply_terrain_update(&mut commands, &mut meshes, &mut tiles, update);
}

fn fixture(budget: usize) -> App {
    let parent = TileId::roots()[0];
    let camera = Geodetic::from_degrees(0.0, -90.0, 100.0);
    let mut source = MemoryTileSource::new();
    for id in parent.children().unwrap() {
        source.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(42.0))),
        );
    }
    let mut cache = TileCache::new(1_000_000);
    cache.insert(
        parent,
        DemTile::new(parent.bounds(), HeightGrid::flat(9, 9, Meters(42.0))),
    );
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(RenderOrigin::new(camera))
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(TerrainTiles::default())
        .insert_resource(Streaming {
            selector: LodSelector::new(
                0.01,
                1_080.0,
                Degrees(60.0).to_radians(),
                1,
                Meters(20_000.0),
            ),
            source,
            cache,
            state: TerrainSelectionState::default(),
            camera: camera.to_ecef(),
            budget,
            last: TerrainUpdate::default(),
        })
        // Match the production ordering: the regular transform pass has already
        // finished when this frame's hidden terrain meshes are created.
        .configure_sets(Update, (RenderSet::Transforms, RenderSet::Terrain).chain())
        .add_systems(Update, apply_world_positions.in_set(RenderSet::Transforms))
        .add_systems(Update, stream.in_set(RenderSet::Terrain));
    app
}

fn check_entities_match_selection(app: &mut App) -> BTreeSet<TileId> {
    let origin = app.world().resource::<RenderOrigin>().0;
    let streaming = app.world().resource::<Streaming>();
    let expected: BTreeSet<_> = streaming.state.ids().collect();
    let resident = streaming.state.resident_len();
    assert!(streaming.last.load_attempts <= streaming.budget);
    assert!(streaming.last.prepared.len() <= streaming.budget);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), resident);
    assert_eq!(app.world().resource::<TerrainTiles>().len(), resident);
    let mut visible = BTreeSet::new();
    let mut count = 0;
    let mut query = app
        .world_mut()
        .query::<(&TerrainTile, &Visibility, &WorldPosition, &Transform)>();
    for (tile, visibility, position, transform) in query.iter(app.world()) {
        count += 1;
        assert!((transform.translation - origin.to_render(position.0)).length() < 1.0e-5);
        assert!(
            transform
                .rotation
                .dot(origin.rotation_to_render(glam::DQuat::IDENTITY))
                .abs()
                > 0.999_999
        );
        if *visibility != Visibility::Hidden {
            visible.insert(tile.0);
        }
    }
    assert_eq!(count, resident);
    assert_eq!(visible, expected);
    visible
}

#[test]
fn prepared_tiles_are_positioned_even_after_the_normal_transform_pass() {
    let mut app = fixture(1);
    app.update();
    assert_eq!(
        check_entities_match_selection(&mut app),
        BTreeSet::from([TileId::roots()[0]])
    );
    app.update();
    check_entities_match_selection(&mut app);
    let streaming = app.world().resource::<Streaming>();
    assert_eq!(streaming.state.len(), 1);
    assert_eq!(
        streaming.state.resident_len(),
        2,
        "one child must be prepared but hidden"
    );
    let mut query = app.world_mut().query::<(&TerrainTile, &Visibility)>();
    assert!(
        query
            .iter(app.world())
            .any(|(tile, visibility)| tile.0.level == 1 && *visibility == Visibility::Hidden)
    );
}

#[test]
fn parent_and_children_change_visibility_together_after_bounded_preparation() {
    let mut app = fixture(1);
    let parent = TileId::roots()[0];
    for _ in 0..4 {
        app.update();
        assert_eq!(
            check_entities_match_selection(&mut app),
            BTreeSet::from([parent])
        );
    }
    app.update();
    assert_eq!(
        check_entities_match_selection(&mut app),
        BTreeSet::from(parent.children().unwrap())
    );
    assert_eq!(app.world().resource::<Streaming>().last.prepared.len(), 1);
    assert_eq!(app.world().resource::<Streaming>().last.spawned.len(), 4);
}

#[test]
fn same_update_preparation_and_destruction_releases_the_mesh_asset() {
    let mut app = fixture(8);
    app.update();
    let parent = TileId::roots()[0];
    assert_eq!(
        check_entities_match_selection(&mut app),
        BTreeSet::from(parent.children().unwrap())
    );
    let update = &app.world().resource::<Streaming>().last;
    assert!(update.prepared.contains(&parent));
    assert!(update.despawned.contains(&parent));
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 4);
    app.update();
    check_entities_match_selection(&mut app);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 4);
}
