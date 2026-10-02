//! Bounded actual-edge stitching and atomic terrain visibility transactions.
//!
//! The selector may prepare an arbitrary mixed-LOD/source cut. Keep the old cut
//! and its bridges visible until all new bridges are ready, spending only the
//! surface generator's remaining frame budget. No DEM is read here. Boundary
//! data survive DEM cache eviction; source replacements have new generations.

use crate::TerrainUpdate;
use crate::terrain::{TerrainTiles, apply_terrain_update};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, Meters, Radians, RenderFrame};
use flightsim_world::TileId;
use flightsim_world::seams::{
    TerrainBoundary, TerrainSeam, TerrainSeamKey, TerrainSeamPlanner, TerrainSeamPlanningUsage,
};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum deterministic topology work per call, independent of the mesh budget.
/// Planning uses no wall clock and performs no whole-boundary snapshot. This is
/// a work bound, not a wall-time/60 fps guarantee; cut assembly and atomic commit
/// remain separate, linear operations.
pub const SEAM_PLANNING_WORK_PER_FRAME: usize = 1_024;

/// A bridge is render-only and participates in floating-origin updates.
#[derive(Component, Debug, Clone, Copy)]
pub struct TerrainBridge(pub TerrainSeamKey);

/// Counts for the most recent stitched update. Surface preparations and this
/// frame's bridge preparations together never exceed the supplied mesh budget.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StitchProgress {
    /// Topology work charged this frame, at most `SEAM_PLANNING_WORK_PER_FRAME`.
    pub planning_work: usize,
    /// True while topology or its scratch cleanup is still pending.
    pub planning_pending: bool,
    pub prepared: usize,
    pub remaining: usize,
    pub visible: usize,
    pub resident: usize,
}

/// Logical geometry owned by the stitched path, including pending/retired
/// assets. Byte counts include mesh positions, normals, UVs, colours and indices;
/// driver copies, Bevy bookkeeping and allocation capacity are not included.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TerrainResourceUsage {
    pub planning_pending: bool,
    pub planning: TerrainSeamPlanningUsage,
    pub planned_bridges: usize,
    pub surface_meshes: usize,
    pub retired_surface_meshes: usize,
    pub bridge_meshes: usize,
    pub visible_bridges: usize,
    pub queued_bridges: usize,
    pub surface_vertices: usize,
    pub bridge_vertices: usize,
    pub indices: usize,
    pub boundary_vertices: usize,
    pub boundary_bytes: usize,
    pub geometry_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
struct MeshFootprint {
    vertices: usize,
    indices: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedSeam {
    descriptor: TerrainSeam,
    generations: [u64; 2],
}

#[derive(Debug)]
struct RenderedSeam {
    plan: PlannedSeam,
    entity: Entity,
    mesh: Handle<Mesh>,
    footprint: MeshFootprint,
}

#[derive(Debug)]
struct PendingCut {
    update: TerrainUpdate,
    target: BTreeSet<TileId>,
    desired: BTreeMap<TerrainSeamKey, PlannedSeam>,
    queue: BTreeSet<TerrainSeamKey>,
    planner: Option<TerrainSeamPlanner>,
    prepared: BTreeMap<TerrainSeamKey, RenderedSeam>,
}

/// Internal ownership of compact boundaries and one pending cut. With at most
/// N=8192 resident tiles, each cut has at most 4N bridges and a pending cut keeps
/// at most two bridge sets. At most one frame's replaced tile assets are retired
/// here because the selector must pause until commit. No history accumulates.
#[derive(Debug, Default)]
pub(crate) struct TerrainStitching {
    boundaries: BTreeMap<TileId, TerrainBoundary>,
    generations: BTreeMap<TileId, u64>,
    next_generation: u64,
    displayed: BTreeSet<TileId>,
    visible: BTreeMap<TerrainSeamKey, RenderedSeam>,
    retired: Vec<(Entity, Handle<Mesh>)>,
    surfaces: BTreeMap<Entity, MeshFootprint>,
    pending: Option<PendingCut>,
}

impl TerrainStitching {
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn displayed_ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.displayed.iter().copied()
    }

    pub(crate) fn record_surface(&mut self, entity: Entity, vertices: usize, indices: usize) {
        self.surfaces
            .insert(entity, MeshFootprint { vertices, indices });
    }

    pub(crate) fn resource_usage(&self) -> TerrainResourceUsage {
        let mut usage = TerrainResourceUsage {
            planning_pending: self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.planner.is_some()),
            planning: self
                .pending
                .as_ref()
                .and_then(|pending| pending.planner.as_ref())
                .map_or_else(
                    TerrainSeamPlanningUsage::default,
                    TerrainSeamPlanner::resource_usage,
                ),
            planned_bridges: self
                .pending
                .as_ref()
                .map_or(0, |pending| pending.desired.len()),
            surface_meshes: self.surfaces.len(),
            retired_surface_meshes: self.retired.len(),
            visible_bridges: self.visible.len(),
            queued_bridges: self
                .pending
                .as_ref()
                .map_or(0, |pending| pending.queue.len()),
            ..Default::default()
        };
        for surface in self.surfaces.values() {
            usage.surface_vertices += surface.vertices;
            usage.indices += surface.indices;
        }
        for bridge in self.visible.values().chain(
            self.pending
                .iter()
                .flat_map(|pending| pending.prepared.values()),
        ) {
            usage.bridge_meshes += 1;
            usage.bridge_vertices += bridge.footprint.vertices;
            usage.indices += bridge.footprint.indices;
        }
        for boundary in self.boundaries.values() {
            usage.boundary_vertices += boundary.vertex_count();
            usage.boundary_bytes += boundary.memory_footprint();
        }
        usage.geometry_bytes =
            48 * (usage.surface_vertices + usage.bridge_vertices) + 4 * usage.indices;
        usage
    }

    pub(crate) fn retire(&mut self, asset: (Entity, Handle<Mesh>)) {
        assert!(
            !self.is_pending(),
            "pause terrain selection while stitching"
        );
        self.retired.push(asset);
    }

    pub(crate) fn insert_boundary(&mut self, id: TileId, boundary: TerrainBoundary) {
        assert!(
            !self.is_pending(),
            "pause terrain selection while stitching"
        );
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("terrain generation exhausted");
        self.boundaries.insert(id, boundary);
        self.generations.insert(id, self.next_generation);
    }

    pub(crate) fn remove_boundary(&mut self, id: TileId) {
        self.boundaries.remove(&id);
        self.generations.remove(&id);
    }

    pub(crate) fn drain_all(&mut self) -> Vec<(Entity, Handle<Mesh>)> {
        let mut result = std::mem::take(&mut self.retired);
        result.extend(
            std::mem::take(&mut self.visible)
                .into_values()
                .map(|seam| (seam.entity, seam.mesh)),
        );
        if let Some(pending) = self.pending.take() {
            result.extend(
                pending
                    .prepared
                    .into_values()
                    .map(|seam| (seam.entity, seam.mesh)),
            );
        }
        *self = Self::default();
        result
    }

    fn progress(&self, prepared: usize, planning_work: usize) -> StitchProgress {
        StitchProgress {
            planning_work,
            planning_pending: self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.planner.is_some()),
            prepared,
            remaining: self
                .pending
                .as_ref()
                .map_or(0, |pending| pending.queue.len()),
            visible: self.visible.len(),
            resident: self.visible.len()
                + self
                    .pending
                    .as_ref()
                    .map_or(0, |pending| pending.prepared.len()),
        }
    }

    fn prune_boundaries(&mut self, tiles: &TerrainTiles) {
        self.boundaries.retain(|id, _| tiles.contains(*id));
        self.generations.retain(|id, _| tiles.contains(*id));
        let entities: BTreeSet<_> = tiles.ids().filter_map(|id| tiles.entity(id)).collect();
        self.surfaces.retain(|entity, _| entities.contains(entity));
    }
}

/// Stage a selector update and spend the mesh budget left after this frame's
/// surface preparations on actual-edge bridges. Call only after registering all
/// `prepare_tile` results with `TerrainTiles::insert_prepared`.
///
/// While `tiles.is_stitching()` is true, call [`advance_stitched_update`] instead
/// of running the selector. This prevents starvation and holds source identity,
/// geometry, and old visible coverage stable until one atomic commit.
#[allow(
    clippy::too_many_arguments,
    reason = "transaction uses the existing terrain render context"
)]
pub fn apply_stitched_update(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tiles: &mut TerrainTiles,
    material: Handle<StandardMaterial>,
    frame: &RenderFrame,
    update: TerrainUpdate,
    mesh_budget: usize,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> StitchProgress {
    assert!(
        !tiles.is_stitching(),
        "advance the pending terrain cut before selecting another"
    );
    assert!(
        update.prepared.len() <= mesh_budget,
        "surface preparation exceeded the shared mesh budget"
    );
    let remaining_budget = mesh_budget - update.prepared.len();
    let mut state = std::mem::take(&mut tiles.stitching);
    let mut target = state.displayed.clone();
    for id in update.hidden.iter().chain(&update.despawned) {
        target.remove(id);
    }
    target.extend(&update.spawned);
    let changed = target != state.displayed || update.replaced.iter().any(|id| target.contains(id));
    if changed {
        assert!(
            target.iter().all(|id| state.boundaries.contains_key(id)),
            "stitched tiles must retain their actual mesh boundaries"
        );
        state.pending = Some(PendingCut {
            update,
            target,
            desired: BTreeMap::new(),
            queue: BTreeSet::new(),
            planner: Some(TerrainSeamPlanner::default()),
            prepared: BTreeMap::new(),
        });
    } else {
        apply_terrain_update(commands, meshes, tiles, update);
        release_assets(commands, meshes, std::mem::take(&mut state.retired));
        state.prune_boundaries(tiles);
    }
    tiles.stitching = state;
    advance_stitched_update(
        commands,
        meshes,
        tiles,
        material,
        frame,
        remaining_budget,
        color,
    )
}

/// Advance bounded topology work and at most `mesh_budget` new bridge meshes.
/// Newly spawned bridges use this frame's origin even after a rebase while the
/// transaction was pending. Already resident entities follow normal transforms.
#[allow(
    clippy::too_many_arguments,
    reason = "bounded preparation uses the existing terrain render context"
)]
pub fn advance_stitched_update(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tiles: &mut TerrainTiles,
    material: Handle<StandardMaterial>,
    frame: &RenderFrame,
    mesh_budget: usize,
    color: Option<&dyn Fn(Geodetic, Radians) -> [f32; 4]>,
) -> StitchProgress {
    let mut state = std::mem::take(&mut tiles.stitching);
    let mut prepared_count = 0;
    let mut planning_work = 0;
    if let Some(pending) = &mut state.pending {
        if let Some(planner) = &mut pending.planner {
            planning_work = planner.advance(
                &state.boundaries,
                &pending.target,
                SEAM_PLANNING_WORK_PER_FRAME,
                |descriptor| {
                    let key = descriptor.key();
                    let plan = PlannedSeam {
                        generations: descriptor.tiles().map(|id| state.generations[&id]),
                        descriptor,
                    };
                    if state.visible.get(&key).is_none_or(|seam| seam.plan != plan) {
                        pending.queue.insert(key);
                    }
                    pending.desired.insert(key, plan);
                    assert!(
                        pending.desired.len() <= 4 * pending.target.len(),
                        "terrain edge topology exceeded the linear bridge bound"
                    );
                },
            );
            if planner.is_finished() {
                // Completed planners have already drained their scratch within
                // the work budget; this drop does not defer a whole-plan free.
                pending.planner = None;
            }
        }
        let build_budget = if pending.planner.is_none() {
            mesh_budget
        } else {
            0
        };
        for _ in 0..build_budget {
            let Some(key) = pending.queue.pop_first() else {
                break;
            };
            let plan = pending.desired[&key].clone();
            let (vertex_bound, index_bound) = plan.descriptor.mesh_size_bound(&state.boundaries);
            let source = plan.descriptor.build_mesh(&state.boundaries);
            assert!(
                source.positions.len() <= vertex_bound && source.indices.len() <= index_bound,
                "bridge generation exceeded its planned geometry bound"
            );
            let mut mesh = crate::to_bevy_mesh(&source);
            if let Some(color) = color {
                let colors: Vec<_> = source
                    .positions
                    .iter()
                    .zip(&source.elevations)
                    .zip(&source.slopes)
                    .map(|((position, elevation), slope)| {
                        let position = Ecef(
                            source.origin.as_vec()
                                + glam::DVec3::new(
                                    f64::from(position[0]),
                                    f64::from(position[1]),
                                    f64::from(position[2]),
                                ),
                        )
                        .to_geodetic();
                        color(
                            Geodetic::new(
                                position.latitude,
                                position.longitude,
                                Meters(f64::from(*elevation)),
                            ),
                            Radians(f64::from(*slope)),
                        )
                    })
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
            }
            let handle = meshes.add(mesh);
            let transform = Transform {
                translation: frame.to_render(source.origin),
                rotation: frame.rotation_to_render(glam::DQuat::IDENTITY),
                ..default()
            };
            let entity = commands
                .spawn((
                    crate::terrain_mesh_bundle(handle.clone(), material.clone(), source.origin),
                    TerrainBridge(key),
                    Name::new(format!("terrain bridge {:?}", key)),
                ))
                .insert((transform, Visibility::Hidden))
                .id();
            pending.prepared.insert(
                key,
                RenderedSeam {
                    plan,
                    entity,
                    mesh: handle,
                    footprint: MeshFootprint {
                        vertices: source.positions.len(),
                        indices: source.indices.len(),
                    },
                },
            );
            prepared_count += 1;
        }
    }
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| pending.planner.is_none() && pending.queue.is_empty())
    {
        let pending = state.pending.take().expect("ready transaction");
        let mut old = std::mem::take(&mut state.visible);
        let mut prepared = pending.prepared;
        for key in pending.desired.keys() {
            let seam = if let Some(seam) = prepared.remove(key) {
                commands.entity(seam.entity).insert(Visibility::Inherited);
                seam
            } else {
                old.remove(key).expect("unchanged bridge remains resident")
            };
            state.visible.insert(*key, seam);
        }
        release_assets(
            commands,
            meshes,
            old.into_values().map(|seam| (seam.entity, seam.mesh)),
        );
        apply_terrain_update(commands, meshes, tiles, pending.update);
        release_assets(commands, meshes, std::mem::take(&mut state.retired));
        state.displayed = pending.target;
        state.prune_boundaries(tiles);
    }
    let progress = state.progress(prepared_count, planning_work);
    tiles.stitching = state;
    progress
}

fn release_assets(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: impl IntoIterator<Item = (Entity, Handle<Mesh>)>,
) {
    for (entity, mesh) in assets {
        meshes.remove(&mesh);
        commands.entity(entity).despawn();
    }
}
