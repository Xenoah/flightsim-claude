//! Public-API lifecycle regressions for bounded, atomic terrain-overlay uploads.
//! Covers source-asset retirement, cancellation, caller-owned entity reuse and
//! complete asset cleanup. These ECS tests do not measure GPU completion or FPS.
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use flightsim_core::{Geodetic, Meters, Radians};
use flightsim_render::terrain::{TerrainTile, prepare_tile};
use flightsim_render::terrain_drape::TerrainOverlay;
use flightsim_render::terrain_stitching::{
    StitchProgress, advance_stitched_update, apply_stitched_update,
};
use flightsim_render::{RenderOrigin, TerrainOverlayRegistration, TerrainTiles, TerrainUpdate};
use flightsim_world::{DemTile, HeightGrid, MemoryTileSource, Terrain, TileId};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

// Asynchronous clipping must be given a chance to run. This limits fixture
// retries rather than asserting a wall-clock performance budget.
const MAX_UPDATES: usize = 5_000;

fn fixture_number(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("fixture index fits in u32"))
}

#[derive(Resource)]
struct Driver {
    queue: VecDeque<(TileId, Arc<DemTile>, bool)>,
    budget: usize,
    progress: StitchProgress,
}
impl Default for Driver {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            budget: 1,
            progress: StitchProgress::default(),
        }
    }
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "Bevy systems take owned parameter wrappers"
)]
fn drive(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: ResMut<TerrainTiles>,
    origin: Res<RenderOrigin>,
    mut driver: ResMut<Driver>,
) {
    if tiles.is_stitching() {
        driver.progress = advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            driver.budget,
            None,
        );
    } else if !driver.queue.is_empty() && driver.budget != 0 {
        let (id, dem, replacement) = driver.queue.pop_front().unwrap();
        let prepared = prepare_tile(
            &mut commands,
            &mut meshes,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            id,
            &dem,
            None,
        );
        tiles.insert_prepared(prepared);
        driver.progress = apply_stitched_update(
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
            driver.budget,
            None,
        );
    } else if tiles.overlay_usage().dirty && driver.queue.is_empty() {
        driver.progress = apply_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            TerrainUpdate::default(),
            driver.budget,
            None,
        );
    } else {
        driver.progress = advance_stitched_update(
            &mut commands,
            &mut meshes,
            &mut tiles,
            Handle::<StandardMaterial>::default(),
            &origin.0,
            driver.budget,
            None,
        );
    }
}
struct Root {
    entity: Entity,
    original: Handle<Mesh>,
    authored: Mesh,
    template: TerrainOverlay,
}
struct Rig {
    app: App,
    id: TileId,
}
impl Rig {
    fn new() -> Self {
        let id = TileId::containing(13, Geodetic::from_degrees(35.55, 139.78, 0.0));
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<TerrainTiles>()
            .init_resource::<Driver>()
            .insert_resource(RenderOrigin::new(id.center()))
            .add_systems(Update, drive);
        Self { app, id }
    }
    fn registration(&mut self, n: usize) -> (Root, TerrainOverlayRegistration) {
        let center = self.id.center();
        let threshold = Geodetic::new(center.latitude, center.longitude, Meters(10.0));
        let (mesh, origin) = flightsim_render::runway::runway_mesh(
            threshold,
            Radians::ZERO,
            Meters(50.0 + fixture_number(n) * 10.0),
            Meters(20.0),
        );
        let source = TerrainOverlay::surface(&mesh, origin, |_| Meters(10.0)).unwrap();
        let handle = self
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(mesh.clone());
        let entity = self
            .app
            .world_mut()
            .spawn((Mesh3d(handle.clone()), Visibility::Hidden))
            .id();
        (
            Root {
                entity,
                original: handle.clone(),
                authored: mesh,
                template: source.clone(),
            },
            TerrainOverlayRegistration {
                entity,
                mesh: handle,
                source,
            },
        )
    }
    fn required(&mut self, count: usize) -> Vec<Root> {
        (0..count)
            .map(|n| {
                let (root, registration) = self.registration(n);
                self.app
                    .world_mut()
                    .resource_mut::<TerrainTiles>()
                    .register_overlay(registration.entity, registration.mesh, registration.source)
                    .unwrap();
                root
            })
            .collect()
    }
    fn optional(&mut self, count: usize) -> (Vec<Root>, flightsim_render::TerrainOverlaySwap) {
        let (roots, items): (Vec<_>, Vec<_>) =
            (0..count).map(|n| self.registration(n + 10)).unzip();
        let swap = self
            .app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .replace_optional_overlays(items)
            .unwrap();
        (roots, swap)
    }
    fn enqueue(&mut self, height: f64, replacement: bool) -> Arc<DemTile> {
        let dem = Arc::new(DemTile::new(
            self.id.bounds(),
            HeightGrid::flat(33, 33, Meters(height)),
        ));
        self.app
            .world_mut()
            .resource_mut::<Driver>()
            .queue
            .push_back((self.id, dem.clone(), replacement));
        dem
    }
    fn budget(&mut self, budget: usize) {
        self.app.world_mut().resource_mut::<Driver>().budget = budget;
    }
    fn tick(&mut self) {
        self.app.update();
        let driver = self.app.world().resource::<Driver>();
        let w = driver.progress.overlay_work;
        assert!(driver.progress.prepared <= driver.budget);
        assert!(w.copy_attempts + w.upload_attempts <= driver.budget);
        assert!(w.copied_vertices <= flightsim_render::OVERLAY_COPY_VERTICES_PER_FRAME);
        assert!(
            w.uploaded_vertices <= flightsim_render::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES
        );
        assert!(
            w.uploaded_vertices <= flightsim_render::OVERLAY_UPLOAD_VERTEX_TARGET
                || w.uploaded_meshes == 1
        );
        assert_eq!(
            w,
            self.app
                .world()
                .resource::<TerrainTiles>()
                .overlay_usage()
                .frame_work
        );
    }
    fn pending(&self) -> bool {
        self.app.world().resource::<TerrainTiles>().is_stitching()
    }
    fn finish(&mut self) {
        for _ in 0..MAX_UPDATES {
            self.tick();
            if !self.pending()
                && !self
                    .app
                    .world()
                    .resource::<TerrainTiles>()
                    .overlay_usage()
                    .dirty
                && self.app.world().resource::<Driver>().queue.is_empty()
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("public terrain transaction failed to settle");
    }
    fn reach_partial(&mut self, count: usize) {
        for _ in 0..MAX_UPDATES {
            self.tick();
            let u = self.app.world().resource::<TerrainTiles>().overlay_usage();
            if u.uploaded_meshes == count && self.pending() {
                return;
            }
            assert!(
                u.uploaded_meshes <= count,
                "missed requested partial upload"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("requested partial upload never appeared");
    }
    fn handle(&self, entity: Entity) -> Handle<Mesh> {
        self.app.world().get::<Mesh3d>(entity).unwrap().0.clone()
    }
    fn exists(&self, handle: &Handle<Mesh>) -> bool {
        self.app
            .world()
            .resource::<Assets<Mesh>>()
            .get(handle)
            .is_some()
    }
    fn assets(&self) -> usize {
        self.app.world().resource::<Assets<Mesh>>().len()
    }
    fn children(&self, entity: Entity) -> usize {
        self.app
            .world()
            .get::<Children>(entity)
            .map_or(0, |children| children.len())
    }
    fn visible_terrain(&mut self) -> BTreeSet<Entity> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &TerrainTile, &Visibility)>();
        q.iter(self.app.world())
            .filter(|(_, _, visibility)| **visibility != Visibility::Hidden)
            .map(|(e, _, _)| e)
            .collect()
    }
    fn remove_assets(&mut self, entries: Vec<(Entity, Handle<Mesh>)>) {
        let mut entities = BTreeSet::new();
        let mut handles = BTreeSet::new();
        for (entity, handle) in entries {
            assert!(entities.insert(entity), "duplicate cleanup entity");
            assert!(handles.insert(handle.id()), "duplicate cleanup asset");
            assert!(
                self.app.world().get_entity(entity).is_ok(),
                "cleanup returned already-dead entity"
            );
            self.app.world_mut().entity_mut(entity).despawn();
            self.app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .remove(&handle);
        }
    }
    fn drain(&mut self) {
        let entries = self
            .app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .drain_all();
        self.remove_assets(entries);
        assert!(
            self.app
                .world_mut()
                .resource_mut::<TerrainTiles>()
                .drain_all()
                .is_empty(),
            "drain must be idempotent"
        );
    }
    fn clear_optional(&mut self) {
        let entries = self
            .app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .clear_optional_overlays();
        self.remove_assets(entries);
        assert!(
            self.app
                .world_mut()
                .resource_mut::<TerrainTiles>()
                .clear_optional_overlays()
                .is_empty(),
            "optional clear must be idempotent"
        );
    }
    fn unregister_despawn(&mut self, root: &Root) {
        assert!(
            self.app
                .world_mut()
                .resource_mut::<TerrainTiles>()
                .unregister_overlay(root.entity)
        );
        assert!(
            !self
                .app
                .world_mut()
                .resource_mut::<TerrainTiles>()
                .unregister_overlay(root.entity)
        );
        self.app.world_mut().entity_mut(root.entity).despawn();
        self.app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&root.original);
    }
    fn close(&mut self, roots: &[Root]) {
        for root in roots {
            self.unregister_despawn(root);
        }
        self.drain();
        self.budget(0);
        self.finish();
        assert_eq!(
            self.assets(),
            0,
            "all managed terrain and overlay assets must be reclaimed"
        );
        assert_eq!(
            self.app
                .world()
                .resource::<TerrainTiles>()
                .overlay_usage()
                .registered,
            0
        );
        let mut query = self.app.world_mut().query::<&Mesh3d>();
        assert_eq!(query.iter(self.app.world()).count(), 0);
    }
}
fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
        VertexAttributeValues::Float32x3(p) => p.clone(),
        _ => panic!("wrong mesh positions"),
    }
}

#[test]
fn initial_assets_retire_only_at_full_atomic_commit_and_dem_stays_readonly() {
    let mut rig = Rig::new();
    let mut roots = rig.required(3);
    let (optional, _) = rig.optional(1);
    roots.extend(optional);
    let source = rig.enqueue(10.0, false);
    let original_dem = (*source).clone();
    let mut physical_source = MemoryTileSource::new();
    physical_source.insert(rig.id, (*source).clone());
    let mut sampler = Terrain::new(physical_source, 1_000_000, 13..=13);
    let physical_height = sampler.elevation_at(rig.id.center());
    assert_eq!(physical_height, Some(Meters(10.0)));
    let mut copied_terrain = None;
    let mut staged_counts = BTreeSet::new();
    for _ in 0..MAX_UPDATES {
        rig.tick();
        let terrain_entity = rig
            .app
            .world()
            .resource::<TerrainTiles>()
            .entity(rig.id)
            .unwrap();
        let terrain_handle = rig.handle(terrain_entity);
        let current_positions = positions(
            rig.app
                .world()
                .resource::<Assets<Mesh>>()
                .get(&terrain_handle)
                .unwrap(),
        );
        if let Some(ref expected) = copied_terrain {
            assert_eq!(&current_positions, expected);
        } else {
            copied_terrain = Some(current_positions);
        }
        assert_eq!(*source, original_dem);
        assert_eq!(sampler.elevation_at(rig.id.center()), physical_height);
        if !rig.pending() {
            break;
        }
        let staged = rig
            .app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .uploaded_meshes;
        staged_counts.insert(staged);
        assert!(rig.visible_terrain().is_empty());
        for root in &roots {
            assert_eq!(rig.handle(root.entity), root.original);
            assert!(rig.exists(&root.original));
            assert_eq!(
                positions(
                    rig.app
                        .world()
                        .resource::<Assets<Mesh>>()
                        .get(&root.original)
                        .unwrap()
                ),
                positions(&root.authored)
            );
            assert_eq!(
                rig.app.world().get::<Visibility>(root.entity),
                Some(&Visibility::Hidden)
            );
        }
        if staged > 0 {
            rig.budget(0);
            for _ in 0..3 {
                rig.tick();
                assert!(rig.pending());
                assert_eq!(
                    rig.app
                        .world()
                        .resource::<TerrainTiles>()
                        .overlay_usage()
                        .uploaded_meshes,
                    staged
                );
            }
            rig.budget(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!rig.pending());
    assert!(staged_counts.is_superset(&BTreeSet::from([1, 2, 3])));
    assert_eq!(rig.assets(), 5);
    for root in &roots {
        assert!(!rig.exists(&root.original));
        assert_ne!(rig.handle(root.entity), root.original);
        assert_eq!(rig.children(root.entity), 1);
    }
    rig.enqueue(80.0, true);
    rig.finish();
    assert_eq!(*source, original_dem);
    assert_eq!(sampler.elevation_at(rig.id.center()), physical_height);
    assert_eq!(rig.assets(), 5);
    rig.close(&roots);
}

#[test]
fn repeated_partial_reset_preserves_current_outputs_and_closes_every_staged_asset() {
    let mut rig = Rig::new();
    let roots = rig.required(4);
    rig.enqueue(10.0, false);
    rig.finish();
    for cycle in 0..12 {
        let current: Vec<_> = roots.iter().map(|r| rig.handle(r.entity)).collect();
        rig.enqueue(30.0 + fixture_number(cycle), true);
        rig.reach_partial(1 + cycle % 3);
        rig.drain();
        assert_eq!(
            rig.assets(),
            4,
            "reset retains only four committed overlays"
        );
        assert!(!rig.pending());
        assert!(
            !rig.app
                .world()
                .resource::<TerrainTiles>()
                .overlay_usage()
                .pending
        );
        for (root, handle) in roots.iter().zip(&current) {
            assert_eq!(rig.handle(root.entity), *handle);
            assert!(rig.exists(handle));
            assert_eq!(rig.children(root.entity), 1);
        }
        rig.enqueue(20.0, false);
        rig.finish();
        assert_eq!(rig.assets(), 5);
        for handle in current {
            assert!(!rig.exists(&handle));
        }
    }
    rig.close(&roots);
}

#[test]
fn repeated_optional_swap_clear_reset_cleans_retired_and_unpublished_generations() {
    let mut rig = Rig::new();
    let required = rig.required(2);
    rig.enqueue(10.0, false);
    rig.finish();
    for cycle in 0..8 {
        let (old, _) = rig.optional(3);
        rig.finish();
        let (new, swap) = rig.optional(3);
        assert_eq!(swap.retired.len(), 3);
        rig.reach_partial(1 + cycle % 4);
        rig.clear_optional();
        for root in old.iter().chain(&new) {
            assert!(rig.app.world().get_entity(root.entity).is_err());
            assert!(!rig.exists(&root.original));
        }
        rig.drain();
        assert_eq!(
            rig.assets(),
            2,
            "only committed required overlays may survive"
        );
        for root in &required {
            assert_eq!(rig.children(root.entity), 1);
            assert!(rig.exists(&rig.handle(root.entity)));
        }
        rig.enqueue(10.0, false);
        rig.finish();
        assert_eq!(rig.assets(), 3);
        let usage = rig.app.world().resource::<TerrainTiles>().overlay_usage();
        assert_eq!(usage.optional_registered, 0);
        assert!(!usage.optional_swap_pending);
    }
    rig.close(&required);
}

#[test]
fn partial_unregister_and_reregister_retained_root_has_one_current_ownership_child() {
    let mut rig = Rig::new();
    let mut roots = rig.required(3);
    rig.enqueue(10.0, false);
    rig.finish();
    for cycle in 0..9 {
        rig.enqueue(40.0 + fixture_number(cycle), true);
        rig.reach_partial(1 + cycle % 2);
        let root = &mut roots[cycle % 3];
        let old_output = rig.handle(root.entity);
        assert!(
            rig.app
                .world_mut()
                .resource_mut::<TerrainTiles>()
                .unregister_overlay(root.entity)
        );
        let fresh = rig
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(root.authored.clone());
        rig.app
            .world_mut()
            .entity_mut(root.entity)
            .insert((Mesh3d(fresh.clone()), Visibility::Hidden));
        rig.app
            .world_mut()
            .resource_mut::<TerrainTiles>()
            .register_overlay(root.entity, fresh.clone(), root.template.clone())
            .unwrap();
        root.original = fresh;
        rig.finish();
        assert!(!rig.exists(&old_output));
        assert_eq!(rig.assets(), 4);
        for item in &roots {
            assert_eq!(rig.children(item.entity), 1);
            assert!(!rig.exists(&item.original));
        }
    }
    rig.close(&roots);
}

#[test]
fn despawn_during_staging_then_new_entity_registration_does_not_resurrect_owner() {
    let mut rig = Rig::new();
    let mut roots = rig.required(3);
    rig.enqueue(10.0, false);
    rig.finish();
    for cycle in 0..9 {
        rig.enqueue(50.0 + fixture_number(cycle), true);
        rig.reach_partial(1 + cycle % 2);
        let old = roots.remove(0);
        let output = rig.handle(old.entity);
        rig.unregister_despawn(&old);
        assert!(!rig.exists(&output));
        let mut added = rig.required(1);
        let new = added.remove(0);
        assert_ne!(old.entity, new.entity);
        roots.push(new);
        rig.finish();
        assert!(rig.app.world().get_entity(old.entity).is_err());
        assert_eq!(rig.assets(), 4);
        for item in &roots {
            assert_eq!(rig.children(item.entity), 1);
        }
    }
    rig.close(&roots);
}

#[test]
fn zero_budget_empty_optional_swap_settles_without_uploads() {
    let mut rig = Rig::new();
    let (roots, _) = rig.optional(3);
    rig.enqueue(10.0, false);
    rig.finish();
    let (_, swap) = rig.optional(0);
    rig.budget(0);
    rig.finish();
    assert_eq!(
        rig.app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .registered,
        0
    );
    assert_eq!(
        rig.assets(),
        1,
        "empty replacement retires all generated optional outputs"
    );
    assert_eq!(
        rig.app
            .world()
            .resource::<Driver>()
            .progress
            .overlay_work
            .uploaded_meshes,
        0
    );
    for root in &roots {
        assert_eq!(
            rig.app.world().get::<Visibility>(root.entity),
            Some(&Visibility::Hidden)
        );
        assert_eq!(rig.children(root.entity), 0);
    }
    rig.remove_assets(swap.retired);
    rig.drain();
    rig.finish();
    assert_eq!(rig.assets(), 0);
}

#[test]
fn missing_snapshot_asset_after_original_retirement_recovers_from_immutable_template() {
    let mut rig = Rig::new();
    let mut roots = rig.required(1);
    let (optional, _) = rig.optional(1);
    roots.extend(optional);
    rig.enqueue(10.0, false);
    rig.finish();
    let generated: Vec<_> = roots.iter().map(|root| rig.handle(root.entity)).collect();
    assert!(roots.iter().all(|root| !rig.exists(&root.original)));
    // Explicit asset-loss fault injection after staging, before the first copy.
    rig.enqueue(30.0, true);
    rig.tick();
    assert_eq!(
        rig.app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .copied_tiles,
        0
    );
    let staged_entity = rig
        .app
        .world()
        .resource::<TerrainTiles>()
        .entity(rig.id)
        .unwrap();
    let staged_mesh = rig.handle(staged_entity);
    assert!(
        rig.app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(&staged_mesh)
            .is_some()
    );
    rig.finish();
    let usage = rig.app.world().resource::<TerrainTiles>().overlay_usage();
    assert_eq!(usage.rejected_transactions, 1);
    assert_eq!(usage.omitted_optional, 1);
    assert_eq!(
        rig.assets(),
        0,
        "faulted terrain and rejected overlays leave no retained assets"
    );
    for (root, previous) in roots.iter().zip(&generated) {
        assert_eq!(
            rig.app.world().get::<Visibility>(root.entity),
            Some(&Visibility::Hidden)
        );
        assert!(!rig.exists(previous));
        assert_eq!(rig.children(root.entity), 0);
    }
    rig.enqueue(20.0, true);
    rig.finish();
    assert_eq!(rig.assets(), 3);
    assert_eq!(
        rig.app
            .world()
            .resource::<TerrainTiles>()
            .overlay_usage()
            .omitted_optional,
        0
    );
    for root in &roots {
        assert_eq!(
            rig.app.world().get::<Visibility>(root.entity),
            Some(&Visibility::Inherited)
        );
        assert!(rig.exists(&rig.handle(root.entity)));
        assert!(!rig.exists(&root.original));
        assert_eq!(rig.children(root.entity), 1);
    }
    rig.close(&roots);
}
