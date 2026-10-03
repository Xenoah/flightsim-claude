//! Exercise exact overlays through the production terrain transaction boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "Bevy systems take owned parameter wrappers"
)]
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use flightsim_core::{Geodetic, Meters, Radians};
use flightsim_render::terrain::{TerrainTile, prepare_tile};
use flightsim_render::terrain_drape::TerrainOverlay;
use flightsim_render::terrain_stitching::{
    StitchProgress, advance_stitched_update, apply_stitched_update,
};
use flightsim_render::{RenderOrigin, TerrainTiles, TerrainUpdate};
use flightsim_world::{DemTile, HeightGrid, TileId};
use std::collections::VecDeque;

#[derive(Resource, Default)]
struct Script {
    updates: VecDeque<(TileId, DemTile, bool)>,
    budget: usize,
    progress: StitchProgress,
}

fn drive(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
    origin: Res<RenderOrigin>,
    mut script: ResMut<Script>,
) {
    if tiles.is_stitching() {
        script.progress = advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            script.budget,
            None,
        );
    } else if let Some((id, dem, replacement)) = script.updates.pop_front() {
        tiles.insert_prepared(prepare_tile(
            &mut commands,
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            id,
            &dem,
            None,
        ));
        script.progress = apply_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            TerrainUpdate {
                prepared: vec![id],
                spawned: vec![id],
                replaced: if replacement { vec![id] } else { vec![] },
                ..Default::default()
            },
            script.budget,
            None,
        );
    } else if tiles.overlay_usage().dirty {
        script.progress = apply_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            TerrainUpdate::default(),
            script.budget,
            None,
        );
    }
}

struct Fixture {
    app: App,
    id: TileId,
    overlay: Entity,
    mesh: Handle<Mesh>,
}

fn fixture() -> Fixture {
    let id = TileId::containing(13, Geodetic::from_degrees(35.55, 139.78, 0.0));
    let centre = id.center();
    let threshold = Geodetic::new(centre.latitude, centre.longitude, Meters(10.0));
    let (mesh, origin) = flightsim_render::runway::runway_mesh(
        threshold,
        Radians::ZERO,
        Meters(100.0),
        Meters(20.0),
    );
    let template = TerrainOverlay::surface(&mesh, origin, |_| Meters(10.0)).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<TerrainTiles>()
        .insert_resource(RenderOrigin::new(threshold))
        .insert_resource(Script {
            budget: 1,
            ..Default::default()
        })
        .add_systems(Update, drive);
    let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
    let overlay = app
        .world_mut()
        .spawn((Mesh3d(handle.clone()), Visibility::Inherited))
        .id();
    app.world_mut()
        .resource_mut::<TerrainTiles>()
        .register_overlay(overlay, handle.clone(), template)
        .unwrap();
    Fixture {
        app,
        id,
        overlay,
        mesh: handle,
    }
}

fn enqueue(fixture: &mut Fixture, height: f64, replacement: bool) {
    fixture
        .app
        .world_mut()
        .resource_mut::<Script>()
        .updates
        .push_back((
            fixture.id,
            DemTile::new(
                fixture.id.bounds(),
                HeightGrid::flat(33, 33, Meters(height)),
            ),
            replacement,
        ));
}
fn positions(fixture: &Fixture) -> Vec<[f32; 3]> {
    match fixture
        .app
        .world()
        .resource::<Assets<Mesh>>()
        .get(
            &fixture
                .app
                .world()
                .get::<Mesh3d>(fixture.overlay)
                .unwrap()
                .0,
        )
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    {
        VertexAttributeValues::Float32x3(positions) => positions.clone(),
        _ => panic!("positions"),
    }
}
fn finish(fixture: &mut Fixture) {
    for _ in 0..2000 {
        fixture.app.update();
        assert!(fixture.app.world().resource::<Script>().progress.prepared <= 1);
        if !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
            && fixture.app.world().resource::<Script>().updates.is_empty()
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("overlay transaction did not finish");
}
fn visible_entities(fixture: &mut Fixture) -> Vec<Entity> {
    let mut query = fixture
        .app
        .world_mut()
        .query::<(Entity, &TerrainTile, &Visibility)>();
    query
        .iter(fixture.app.world())
        .filter(|(_, _, visibility)| **visibility != Visibility::Hidden)
        .map(|(entity, _, _)| entity)
        .collect()
}

#[test]
fn source_replacement_keeps_old_airport_and_terrain_until_atomic_commit() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let before = positions(&fixture);
    let old_terrain = visible_entities(&mut fixture);
    assert_eq!(old_terrain.len(), 1);
    enqueue(&mut fixture, 110.0, true);
    fixture.app.update();
    assert!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    assert_eq!(positions(&fixture), before);
    assert_eq!(visible_entities(&mut fixture), old_terrain);
    for _ in 0..2000 {
        fixture.app.update();
        if !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
        {
            break;
        }
        assert_eq!(
            positions(&fixture),
            before,
            "overlay moved before matching terrain visibility"
        );
        assert_eq!(visible_entities(&mut fixture), old_terrain);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    assert_ne!(positions(&fixture), before);
    assert_eq!(visible_entities(&mut fixture).len(), 1);
    assert_ne!(visible_entities(&mut fixture), old_terrain);
    assert_eq!(
        *fixture
            .app
            .world()
            .get::<Visibility>(fixture.overlay)
            .unwrap(),
        Visibility::Inherited
    );
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .rejected_transactions,
        0
    );
}

#[test]
fn zero_copy_budget_preserves_old_display_and_reset_cancels_preparation() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let before = positions(&fixture);
    enqueue(&mut fixture, 210.0, true);
    fixture.app.update();
    fixture.app.world_mut().resource_mut::<Script>().budget = 0;
    for _ in 0..4 {
        fixture.app.update();
    }
    assert_eq!(positions(&fixture), before);
    assert!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    let assets = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .drain_all();
    for (entity, mesh) in assets {
        fixture.app.world_mut().entity_mut(entity).despawn();
        fixture
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&mesh);
    }
    assert!(
        !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .pending
    );
    fixture.app.world_mut().resource_mut::<Script>().budget = 1;
    enqueue(&mut fixture, 25.0, false);
    finish(&mut fixture);
    assert_ne!(positions(&fixture), before);
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .registered,
        1
    );
}

#[test]
fn repeated_replacements_do_not_retain_old_mesh_assets() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let count = fixture.app.world().resource::<Assets<Mesh>>().len();
    for height in [20.0, 30.0, 40.0, 50.0] {
        enqueue(&mut fixture, height, true);
        finish(&mut fixture);
        assert_eq!(fixture.app.world().resource::<Assets<Mesh>>().len(), count);
    }
}

fn register_extra(fixture: &mut Fixture) -> (Entity, Handle<Mesh>) {
    let geo = fixture.id.center();
    let threshold = Geodetic::new(geo.latitude, geo.longitude, Meters(10.0));
    let (mesh, origin) =
        flightsim_render::runway::runway_mesh(threshold, Radians::ZERO, Meters(50.0), Meters(10.0));
    let source = TerrainOverlay::surface(&mesh, origin, |_| Meters(10.0)).unwrap();
    let handle = fixture
        .app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(mesh);
    let entity = fixture
        .app
        .world_mut()
        .spawn((Mesh3d(handle.clone()), Visibility::Hidden))
        .id();
    fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .register_overlay(entity, handle.clone(), source)
        .unwrap();
    (entity, handle)
}

#[test]
fn dynamic_registration_refreshes_an_unchanged_cut() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 110.0, false);
    finish(&mut fixture);
    let terrain = visible_entities(&mut fixture);
    let (entity, _) = register_extra(&mut fixture);
    assert_eq!(
        *fixture.app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Hidden
    );
    finish(&mut fixture);
    assert_eq!(
        *fixture.app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Inherited
    );
    assert_eq!(visible_entities(&mut fixture), terrain);
    assert!(
        !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .dirty
    );
}

#[test]
fn unregister_and_register_during_pending_cut_cannot_commit_a_stale_target() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    enqueue(&mut fixture, 110.0, true);
    fixture.app.update();
    assert!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    let (entity, _) = register_extra(&mut fixture);
    assert!(
        fixture
            .app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .unregister_overlay(fixture.overlay)
    );
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.overlay)
        .despawn();
    fixture
        .app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .remove(&fixture.mesh);
    finish(&mut fixture);
    assert_eq!(
        *fixture.app.world().get::<Visibility>(entity).unwrap(),
        Visibility::Inherited
    );
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .registered,
        1
    );
    assert!(
        !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .dirty
    );
}

fn optional_registration(fixture: &mut Fixture) -> flightsim_render::TerrainOverlayRegistration {
    let geo = fixture.id.center();
    let threshold = Geodetic::new(geo.latitude, geo.longitude, Meters(10.0));
    let (mesh, origin) =
        flightsim_render::runway::runway_mesh(threshold, Radians::ZERO, Meters(50.0), Meters(10.0));
    let source = TerrainOverlay::surface(&mesh, origin, |_| Meters(10.0)).unwrap();
    let handle = fixture
        .app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(mesh);
    let entity = fixture
        .app
        .world_mut()
        .spawn((Mesh3d(handle.clone()), Visibility::Hidden))
        .id();
    flightsim_render::TerrainOverlayRegistration {
        entity,
        mesh: handle,
        source,
    }
}

#[test]
fn optional_batch_swap_retains_old_generation_and_explicit_clear_returns_both() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let first = optional_registration(&mut fixture);
    let first_entity = first.entity;
    fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![first])
        .unwrap();
    finish(&mut fixture);
    let second = optional_registration(&mut fixture);
    let second_entity = second.entity;
    let swap = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![second])
        .unwrap();
    assert_eq!(swap.retired.len(), 1);
    assert_eq!(swap.retired[0].0, first_entity);
    assert_eq!(
        fixture.app.world().get::<Visibility>(first_entity),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        fixture.app.world().get::<Visibility>(second_entity),
        Some(&Visibility::Hidden)
    );
    let mut removed: Vec<_> = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .clear_optional_overlays()
        .into_iter()
        .map(|(entity, _)| entity)
        .collect();
    removed.sort();
    let mut expected = vec![first_entity, second_entity];
    expected.sort();
    assert_eq!(removed, expected);
    for entity in removed {
        fixture.app.world_mut().entity_mut(entity).despawn();
    }
    finish(&mut fixture);
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .registered,
        1
    );
    assert_eq!(
        fixture.app.world().get::<Visibility>(fixture.overlay),
        Some(&Visibility::Inherited)
    );
}

#[test]
fn optional_swap_commits_before_owner_retires_old_assets() {
    let mut fixture = fixture();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let first = optional_registration(&mut fixture);
    fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![first])
        .unwrap();
    finish(&mut fixture);
    let second = optional_registration(&mut fixture);
    let entity = second.entity;
    let swap = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(vec![second])
        .unwrap();
    finish(&mut fixture);
    assert!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .committed_revision
            >= swap.revision
    );
    assert_eq!(
        fixture.app.world().get::<Visibility>(entity),
        Some(&Visibility::Inherited)
    );
    for (retired, mesh) in swap.retired {
        fixture.app.world_mut().entity_mut(retired).despawn();
        fixture
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&mesh);
    }
    fixture.app.update();
    assert_eq!(
        fixture.app.world().get::<Visibility>(entity),
        Some(&Visibility::Inherited)
    );
}

#[test]
fn staged_uploads_share_one_attempt_budget_and_switch_all_visible_handles_together() {
    let mut fixture = fixture();
    let (second, _) = register_extra(&mut fixture);
    let (third, _) = register_extra(&mut fixture);
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let entities = [fixture.overlay, second, third];
    let before: Vec<_> = entities
        .iter()
        .map(|&entity| fixture.app.world().get::<Mesh3d>(entity).unwrap().0.clone())
        .collect();
    let terrain_before = visible_entities(&mut fixture);
    let baseline = fixture.app.world().resource::<Assets<Mesh>>().len();
    enqueue(&mut fixture, 90.0, true);
    let mut observed_partial = 0;
    let mut uploads = 0;
    for _ in 0..2000 {
        fixture.app.update();
        let progress = fixture.app.world().resource::<Script>().progress;
        assert!(progress.prepared <= 1);
        assert!(progress.overlay_work.upload_attempts + progress.overlay_work.copy_attempts <= 1);
        assert!(
            progress.overlay_work.uploaded_vertices
                <= flightsim_render::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES
        );
        uploads += progress.overlay_work.uploaded_meshes;
        if !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
        {
            break;
        }
        for (&entity, original) in entities.iter().zip(&before) {
            assert_eq!(
                &fixture.app.world().get::<Mesh3d>(entity).unwrap().0,
                original
            );
            assert_eq!(
                fixture.app.world().get::<Visibility>(entity),
                Some(&Visibility::Inherited)
            );
        }
        assert_eq!(visible_entities(&mut fixture), terrain_before);
        if progress.overlay_work.uploaded_meshes != 0 {
            observed_partial += 1;
            fixture.app.world_mut().resource_mut::<Script>().budget = 0;
            fixture.app.update();
            assert_eq!(
                fixture
                    .app
                    .world()
                    .resource::<Script>()
                    .progress
                    .overlay_work
                    .uploaded_meshes,
                0
            );
            fixture.app.world_mut().resource_mut::<Script>().budget = 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(observed_partial, 2);
    assert_eq!(uploads, 3);
    assert!(
        !fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    for (&entity, original) in entities.iter().zip(&before) {
        assert_ne!(
            &fixture.app.world().get::<Mesh3d>(entity).unwrap().0,
            original
        );
        assert!(
            fixture
                .app
                .world()
                .resource::<Assets<Mesh>>()
                .get(original)
                .is_none()
        );
    }
    assert_eq!(
        fixture.app.world().resource::<Assets<Mesh>>().len(),
        baseline
    );
}

#[test]
fn unregister_despawn_reclaims_committed_and_staged_generated_assets() {
    let mut fixture = fixture();
    let (other, _) = register_extra(&mut fixture);
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    let committed = fixture
        .app
        .world()
        .get::<Mesh3d>(fixture.overlay)
        .unwrap()
        .0
        .clone();
    enqueue(&mut fixture, 90.0, true);
    for _ in 0..2000 {
        fixture.app.update();
        if fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .uploaded_meshes
            == 1
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .is_stitching()
    );
    let pending_count = fixture.app.world().resource::<Assets<Mesh>>().len();
    assert!(
        fixture
            .app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .unregister_overlay(fixture.overlay)
    );
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.overlay)
        .despawn();
    fixture
        .app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .remove(&fixture.mesh);
    assert!(
        fixture
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&committed)
            .is_none()
    );
    assert_eq!(
        fixture.app.world().resource::<Assets<Mesh>>().len(),
        pending_count - 2,
        "root despawn releases old and unpublished generated meshes; original was retired at commit"
    );
    finish(&mut fixture);
    assert_eq!(
        fixture.app.world().get::<Visibility>(other),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        fixture.app.world().resource::<Assets<Mesh>>().len(),
        2,
        "only other generated overlay and one terrain surface remain"
    );
}

#[test]
fn optional_clear_then_terrain_reset_reclaims_mixed_pending_children_once() {
    let mut fixture = fixture();
    let optional: Vec<_> = (0..2)
        .map(|_| optional_registration(&mut fixture))
        .collect();
    fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .replace_optional_overlays(optional)
        .unwrap();
    enqueue(&mut fixture, 10.0, false);
    finish(&mut fixture);
    enqueue(&mut fixture, 90.0, true);
    for _ in 0..2000 {
        fixture.app.update();
        if fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .uploaded_meshes
            == 2
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .uploaded_meshes,
        2
    );
    let removed = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .clear_optional_overlays();
    for (entity, mesh) in removed {
        fixture.app.world_mut().entity_mut(entity).despawn();
        fixture
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&mesh);
    }
    let drained = fixture
        .app
        .world_mut()
        .resource_mut::<TerrainTiles>()
        .drain_all();
    for (entity, mesh) in drained {
        // A returned child must still exist: clear already despawned optional roots.
        fixture.app.world_mut().entity_mut(entity).despawn();
        fixture
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&mesh);
    }
    assert_eq!(fixture.app.world().resource::<Assets<Mesh>>().len(), 1);
    enqueue(&mut fixture, 20.0, false);
    finish(&mut fixture);
    assert_eq!(fixture.app.world().resource::<Assets<Mesh>>().len(), 2);
}
