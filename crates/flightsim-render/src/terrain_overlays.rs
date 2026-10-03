//! Bounded overlay preparation tied to the terrain visibility transaction.
//! CPU copies have a per-update cap; clipping runs on immutable compute-pool
//! inputs. Essential airports are admitted/processed before optional scenery.

use crate::terrain_drape::{
    DrapeError, DrapeTerrain, DrapedOverlay, OverlayPrecision, TerrainOverlay,
};
use bevy::ecs::{lifecycle::HookContext, world::DeferredWorld};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use flightsim_core::{Ecef, Meters, RenderFrame};
use flightsim_world::{TerrainSeamKey, TileId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub const MAX_TERRAIN_OVERLAYS: usize = 64;
pub const MAX_OPTIONAL_OVERLAYS: usize = 24;
pub const MAX_OPTIONAL_SOURCE_TRIANGLES: usize = 40_000;
pub const MAX_OVERLAY_TERRAIN_TILES: usize = 512;
pub const MAX_OVERLAY_TERRAIN_VERTICES: usize = 262_144;
/// Normal snapshot allowance: two maximum 65×65 surface grids. The render
/// boundary in `terrain::prepare_tile_with_global_shading` admits at most 65.
/// Skirt positions are excluded from copied surface vertices.
pub const OVERLAY_COPY_VERTEX_TARGET: usize = 2 * 65 * 65;
/// Two full 65×65 meshes: 6*(65-1)^2 surface indices plus 4*(65-1)*6 skirt
/// indices each. See world::mesh::build_mesh and append_skirt. Inspection charges
/// the complete buffer even though skirt triangles are omitted from the copy.
pub const OVERLAY_COPY_INDEX_TARGET: usize = 2 * (6 * 64 * 64 + 4 * 64 * 6);
const MAX_OVERLAY_TERRAIN_INDICES: usize = 6 * MAX_OVERLAY_TERRAIN_VERTICES;
/// Preserve the previous worst case: two sources at the unchanged per-source
/// index limit. Normally the smaller target applies; one indivisible first
/// source can exceed that target and must then be the only copy this update.
pub const OVERLAY_COPY_INDICES_PER_FRAME: usize = 2 * MAX_OVERLAY_TERRAIN_INDICES;
/// Normal aggregate upload target. One indivisible mesh may exceed this only
/// as the first upload of an update, up to MAX_OVERLAY_OUTPUT_VERTICES.
pub const OVERLAY_UPLOAD_VERTEX_TARGET: usize = 65_536;
/// Snapshot copies have their own actual-vertex cap, including bridges.
pub const OVERLAY_COPY_VERTICES_PER_FRAME: usize = MAX_OVERLAY_TERRAIN_VERTICES;
const MAX_DISCOVERY_SOURCES: usize = 5 * 8_192;

/// A staged, initially hidden ground mesh. Its owner keeps its material and
/// world-position components; the terrain transaction atomically switches its
/// Mesh3d handle and retires the original asset at its first successful commit.
/// The caller may retain/remove that original handle and despawns the root
/// normally (including its generated-asset ownership children).
#[derive(Debug)]
pub struct TerrainOverlayRegistration {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub source: TerrainOverlay,
}

/// After a successful batch replacement, keep these old assets displayed until
/// `overlay_usage().committed_revision >= revision`, then remove/despawn them.
/// Keep at most one outstanding swap; discard unpublished staged assets before
/// requesting another batch. Admission errors leave the old registry untouched.
#[derive(Debug)]
pub struct TerrainOverlaySwap {
    pub revision: u64,
    pub retired: Vec<(Entity, Handle<Mesh>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverlaySourceKey {
    Tile(TileId, u64),
    Bridge(TerrainSeamKey, [u64; 2]),
}

#[derive(Debug, Clone)]
pub(crate) struct OverlayTerrainSource {
    pub key: OverlaySourceKey,
    pub footprint: [TileId; 2],
    pub required: bool,
    pub origin: Ecef,
    pub surface_vertices: usize,
    pub mesh: Handle<Mesh>,
}
#[derive(Debug, Clone)]
struct PrecisionSource {
    footprint: [TileId; 2],
    origin: Ecef,
    extent: f64,
}
#[derive(Debug)]
struct RegisteredOverlay {
    entity: Entity,
    mesh: Handle<Mesh>,
    source: Arc<TerrainOverlay>,
    optional: bool,
}
#[derive(Debug)]
struct PreparedOverlays {
    reuse: bool,
    outputs: Vec<Option<DrapedOverlay>>,
    errors: [Option<DrapeError>; 2],
}
/// An ownership-only child, never a second rendered surface. Despawning the
/// caller-owned root also releases pending/current generated mesh assets, even
/// if the caller resets the scene before the next terrain advance.
#[derive(Component)]
#[component(on_remove = release_owned_mesh)]
struct OwnedOverlayMesh(Handle<Mesh>);

fn release_owned_mesh(mut world: DeferredWorld, context: HookContext) {
    let id = world
        .get::<OwnedOverlayMesh>(context.entity)
        .unwrap()
        .0
        .id();
    if let Some(mut meshes) = world.get_resource_mut::<Assets<Mesh>>() {
        meshes.remove(id);
    }
}

#[derive(Debug)]
struct UploadedOverlay {
    owner: Entity,
    target: Entity,
    mesh: Handle<Mesh>,
    precision: OverlayPrecision,
    vertices: usize,
}

/// Actual work submitted by one terrain update. Attempts include rejected
/// snapshot sources and upload targets; polling/atomic visibility do not count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TerrainOverlayFrameWork {
    pub copy_attempts: usize,
    pub copied_vertices: usize,
    /// Full bounded index buffers reserved for admitted copy attempts. An
    /// invalid position attribute or transaction limit may fail before scanning;
    /// such an attempt is still charged. Missing/oversized index buffers reserve
    /// zero because copy_terrain rejects them without entering its scan.
    pub copy_indices_charged: usize,
    /// Index entries actually consumed by the snapshot triangle iterator,
    /// including rejected skirt triangles. Never exceeds copy_indices_charged.
    pub copy_indices_scanned: usize,
    pub upload_attempts: usize,
    pub uploaded_meshes: usize,
    pub uploaded_vertices: usize,
}

type DrapeResult = Result<PreparedOverlays, DrapeError>;
#[derive(Debug)]
struct Preparation {
    revision: u64,
    signature: Vec<OverlaySourceKey>,
    sources: Vec<OverlayTerrainSource>,
    discovery: Option<Task<Result<Vec<OverlayTerrainSource>, DrapeError>>>,
    required_tiles: usize,
    next_source: usize,
    copied_vertices: usize,
    terrain: Vec<DrapeTerrain>,
    precision: Vec<PrecisionSource>,
    optional_disabled: bool,
    task: Option<Task<DrapeResult>>,
    result: Option<DrapeResult>,
    next_upload: usize,
    uploaded: Vec<Option<UploadedOverlay>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// CPU lifecycle/admission counts, not driver memory or a frame-rate claim.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TerrainOverlayUsage {
    pub registered: usize,
    pub optional_registered: usize,
    pub source_triangles: usize,
    pub source_vertices: usize,
    pub pending: bool,
    pub copied_tiles: usize,
    pub copied_vertices: usize,
    /// Most recent update's actual work, separate from cumulative preparation.
    pub frame_work: TerrainOverlayFrameWork,
    pub uploaded_meshes: usize,
    pub uploaded_vertices: usize,
    pub rejected_transactions: u64,
    pub dirty: bool,
    pub revision: u64,
    pub committed_revision: u64,
    pub precision_hidden: usize,
    pub optional_swap_pending: bool,
    /// Optional batches omitted for the committed cut due to preparation caps/errors.
    /// Separate from the rebase-dependent precision visibility gate.
    pub omitted_optional: usize,
    /// Largest displacement from the original authored mesh's ground surface,
    /// not a claim to have resampled physical contact at every fragment.
    pub maximum_authored_displacement: Meters,
    /// Candidate occurrences over the 60-degree ground-support limit.
    pub omitted_support_facets: usize,
}
#[derive(Debug, Default)]
pub(crate) struct TerrainOverlays {
    registered: Vec<RegisteredOverlay>,
    committed_signature: Option<Vec<OverlaySourceKey>>,
    preparation: Option<Preparation>,
    rejected_transactions: u64,
    revision: u64,
    committed_revision: u64,
    displayed: BTreeSet<Entity>,
    output_precision: BTreeMap<Entity, OverlayPrecision>,
    precision_hidden: BTreeSet<Entity>,
    committed_precision: Vec<PrecisionSource>,
    retired_optional: Vec<RegisteredOverlay>,
    optional_swap_pending: bool,
    omitted_optional: BTreeSet<Entity>,
    visible_uploads: BTreeMap<Entity, UploadedOverlay>,
    cancelled_uploads: Vec<UploadedOverlay>,
    // Owners may already have despawned these roots. Release with try_despawn
    // during advance, never return their potentially stale child IDs to drain.
    retiring_uploads: Vec<UploadedOverlay>,
    frame_work: TerrainOverlayFrameWork,
}

impl TerrainOverlays {
    pub(crate) fn register(
        &mut self,
        entity: Entity,
        mesh: Handle<Mesh>,
        source: TerrainOverlay,
    ) -> Result<(), DrapeError> {
        if source.is_empty() {
            return Ok(());
        }
        let candidate = TerrainOverlayRegistration {
            entity,
            mesh,
            source,
        };
        self.validate_admission(std::slice::from_ref(&candidate), false)?;
        self.registered.push(RegisteredOverlay {
            entity: candidate.entity,
            mesh: candidate.mesh,
            source: Arc::new(candidate.source),
            optional: false,
        });
        self.reset();
        Ok(())
    }

    fn validate_admission(
        &self,
        additions: &[TerrainOverlayRegistration],
        replace_optional: bool,
    ) -> Result<(), DrapeError> {
        let retained: Vec<_> = self
            .registered
            .iter()
            .filter(|overlay| !replace_optional || !overlay.optional)
            .collect();
        let triangles = retained
            .iter()
            .map(|overlay| overlay.source.triangle_count())
            .sum::<usize>()
            + additions
                .iter()
                .map(|overlay| overlay.source.triangle_count())
                .sum::<usize>();
        let vertices = retained
            .iter()
            .map(|overlay| overlay.source.vertex_count())
            .sum::<usize>()
            + additions
                .iter()
                .map(|overlay| overlay.source.vertex_count())
                .sum::<usize>();
        if retained.len() + additions.len() > MAX_TERRAIN_OVERLAYS
            || triangles > crate::terrain_drape::MAX_OVERLAY_SOURCE_TRIANGLES
            || vertices > crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES
        {
            return Err(DrapeError::SourceLimit);
        }
        let mut entities: BTreeSet<_> = self
            .registered
            .iter()
            .map(|overlay| overlay.entity)
            .collect();
        let mut meshes: BTreeSet<_> = self
            .registered
            .iter()
            .map(|overlay| overlay.mesh.id())
            .collect();
        meshes.extend(
            self.visible_uploads
                .values()
                .chain(&self.cancelled_uploads)
                .chain(&self.retiring_uploads)
                .chain(
                    self.preparation
                        .iter()
                        .flat_map(|prep| prep.uploaded.iter().flatten()),
                )
                .map(|upload| upload.mesh.id()),
        );
        for addition in additions {
            if !entities.insert(addition.entity)
                || !meshes.insert(addition.mesh.id())
                || addition.source.is_empty()
            {
                return Err(DrapeError::InvalidMesh);
            }
        }
        Ok(())
    }

    pub(crate) fn replace_optional(
        &mut self,
        additions: Vec<TerrainOverlayRegistration>,
    ) -> Result<TerrainOverlaySwap, DrapeError> {
        if self.optional_swap_pending {
            return Err(DrapeError::UpdatePending);
        }
        if additions.len() > MAX_OPTIONAL_OVERLAYS
            || additions
                .iter()
                .map(|overlay| overlay.source.triangle_count())
                .sum::<usize>()
                > MAX_OPTIONAL_SOURCE_TRIANGLES
        {
            return Err(DrapeError::SourceLimit);
        }
        self.validate_admission(&additions, true)?;
        let mut retired = Vec::new();
        for overlay in std::mem::take(&mut self.registered) {
            if overlay.optional {
                retired.push((overlay.entity, overlay.mesh.clone()));
                self.retired_optional.push(overlay);
            } else {
                self.registered.push(overlay);
            }
        }
        self.registered
            .extend(additions.into_iter().map(|overlay| RegisteredOverlay {
                entity: overlay.entity,
                mesh: overlay.mesh,
                source: Arc::new(overlay.source),
                optional: true,
            }));
        self.reset();
        self.optional_swap_pending = true;
        Ok(TerrainOverlaySwap {
            revision: self.revision,
            retired,
        })
    }

    pub(crate) fn unregister(&mut self, entity: Entity) -> bool {
        if let Some(index) = self
            .registered
            .iter()
            .position(|overlay| overlay.entity == entity)
        {
            self.registered.remove(index);
        } else if let Some(index) = self
            .retired_optional
            .iter()
            .position(|overlay| overlay.entity == entity)
        {
            self.retired_optional.remove(index);
        } else {
            return false;
        }
        self.retire_target_uploads(entity);
        self.omitted_optional.remove(&entity);
        self.displayed.remove(&entity);
        self.output_precision.remove(&entity);
        self.precision_hidden.remove(&entity);
        self.reset();
        true
    }

    fn retire_target_uploads(&mut self, entity: Entity) {
        if let Some(upload) = self.visible_uploads.remove(&entity) {
            self.retiring_uploads.push(upload);
        }
        let mut index = 0;
        while index < self.cancelled_uploads.len() {
            if self.cancelled_uploads[index].target == entity {
                self.retiring_uploads
                    .push(self.cancelled_uploads.swap_remove(index));
            } else {
                index += 1;
            }
        }
        if let Some(prep) = &mut self.preparation {
            for output in &mut prep.uploaded {
                if output
                    .as_ref()
                    .is_some_and(|upload| upload.target == entity)
                {
                    self.retiring_uploads.push(output.take().unwrap());
                }
            }
        }
    }

    pub(crate) fn clear_optional(&mut self) -> Vec<(Entity, Handle<Mesh>)> {
        let mut removed = std::mem::take(&mut self.retired_optional);
        let mut retained = Vec::new();
        for overlay in std::mem::take(&mut self.registered) {
            if overlay.optional {
                removed.push(overlay);
            } else {
                retained.push(overlay);
            }
        }
        self.registered = retained;
        let result = removed
            .into_iter()
            .map(|overlay| {
                self.retire_target_uploads(overlay.entity);
                self.displayed.remove(&overlay.entity);
                self.output_precision.remove(&overlay.entity);
                self.precision_hidden.remove(&overlay.entity);
                (overlay.entity, overlay.mesh)
            })
            .collect();
        self.optional_swap_pending = false;
        self.omitted_optional.clear();
        self.reset();
        result
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }
    pub(crate) fn is_dirty(&self) -> bool {
        self.revision != self.committed_revision
    }
    pub(crate) fn needs_restart(&self) -> bool {
        self.is_dirty()
            && self
                .preparation
                .as_ref()
                .is_none_or(|prep| prep.revision != self.revision)
    }
    pub(crate) fn has_preparation(&self) -> bool {
        self.preparation.is_some()
    }
    pub(crate) fn usage(&self) -> TerrainOverlayUsage {
        TerrainOverlayUsage {
            registered: self.registered.len(),
            optional_registered: self
                .registered
                .iter()
                .filter(|overlay| overlay.optional)
                .count(),
            source_triangles: self
                .registered
                .iter()
                .map(|overlay| overlay.source.triangle_count())
                .sum(),
            source_vertices: self
                .registered
                .iter()
                .map(|overlay| overlay.source.vertex_count())
                .sum(),
            pending: self.preparation.is_some(),
            copied_tiles: self.preparation.as_ref().map_or(0, |prep| prep.next_source),
            copied_vertices: self
                .preparation
                .as_ref()
                .map_or(0, |prep| prep.copied_vertices),
            frame_work: self.frame_work,
            uploaded_meshes: self
                .preparation
                .as_ref()
                .map_or(0, |prep| prep.uploaded.iter().flatten().count()),
            uploaded_vertices: self.preparation.as_ref().map_or(0, |prep| {
                prep.uploaded
                    .iter()
                    .flatten()
                    .map(|upload| upload.vertices)
                    .sum()
            }),
            rejected_transactions: self.rejected_transactions,
            dirty: self.is_dirty(),
            revision: self.revision,
            committed_revision: self.committed_revision,
            precision_hidden: self.precision_hidden.len(),
            optional_swap_pending: self.optional_swap_pending,
            omitted_optional: self.omitted_optional.len(),
            omitted_support_facets: self
                .output_precision
                .values()
                .map(|metrics| metrics.omitted_support_facets)
                .sum(),
            maximum_authored_displacement: Meters(
                self.output_precision
                    .values()
                    .map(|metrics| metrics.authored_displacement.get())
                    .fold(0.0, f64::max),
            ),
        }
    }

    pub(crate) fn begin(&mut self, sources: Vec<OverlayTerrainSource>) {
        self.cancel_preparation();
        let cancelled = Arc::new(AtomicBool::new(false));
        let templates: Vec<_> = self
            .registered
            .iter()
            .map(|overlay| (Arc::clone(&overlay.source), overlay.optional))
            .collect();
        let too_many = sources.len() > MAX_DISCOVERY_SOURCES;
        let discovery_cancelled = Arc::clone(&cancelled);
        let discovery = if too_many || templates.is_empty() {
            None
        } else {
            Some(AsyncComputeTaskPool::get().spawn(async move {
                let mut relevant = Vec::new();
                for mut source in sources {
                    if discovery_cancelled.load(Ordering::Relaxed) {
                        return Err(DrapeError::Cancelled);
                    }
                    let mut interested = false;
                    for (template, optional) in &templates {
                        if source
                            .footprint
                            .iter()
                            .any(|&id| template.intersects_tile(id))
                        {
                            interested = true;
                            source.required |= !optional;
                        }
                    }
                    if interested {
                        relevant.push(source);
                    }
                }
                Ok(relevant)
            }))
        };
        self.preparation = Some(Preparation {
            revision: self.revision,
            signature: Vec::new(),
            sources: Vec::new(),
            discovery,
            required_tiles: 0,
            next_source: 0,
            copied_vertices: 0,
            terrain: Vec::new(),
            precision: Vec::new(),
            optional_disabled: false,
            task: None,
            result: if too_many {
                Some(Err(DrapeError::TerrainLimit))
            } else if self.is_empty() {
                Some(Ok(PreparedOverlays {
                    reuse: false,
                    outputs: Vec::new(),
                    errors: [None; 2],
                }))
            } else {
                None
            },
            next_upload: 0,
            uploaded: Vec::new(),
            cancelled,
        });
    }

    /// Begin one update even when bridge work consumes all available attempts.
    pub(crate) fn begin_frame(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        self.frame_work = TerrainOverlayFrameWork::default();
        for upload in self
            .cancelled_uploads
            .drain(..)
            .chain(self.retiring_uploads.drain(..))
        {
            release_upload(commands, meshes, upload);
        }
    }

    /// Returns readiness and attempts charged against the shared mesh budget.
    pub(crate) fn advance(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        budget: usize,
    ) -> (bool, usize) {
        let Some(prep) = &mut self.preparation else {
            return (true, 0);
        };
        if prep.revision != self.revision || prep.cancelled.load(Ordering::Relaxed) {
            return (false, 0);
        }
        if prep.result.is_some() {
            return self.upload(commands, meshes, budget);
        }
        if let Some(discovery) = &mut prep.discovery {
            let Some(result) = block_on(poll_once(discovery)) else {
                return (false, 0);
            };
            prep.discovery = None;
            match result {
                Ok(mut sources) => {
                    prep.signature = sources.iter().map(|source| source.key.clone()).collect();
                    if prep.revision == self.committed_revision
                        && self.committed_signature.as_ref() == Some(&prep.signature)
                    {
                        prep.result = Some(Ok(PreparedOverlays {
                            reuse: true,
                            outputs: Vec::new(),
                            errors: [None; 2],
                        }));
                        return (true, 0);
                    }
                    sources.sort_by_key(|source| !source.required);
                    if sources.len() > MAX_OVERLAY_TERRAIN_TILES {
                        prep.optional_disabled = true;
                        sources.retain(|source| source.required);
                    }
                    if sources.len() > MAX_OVERLAY_TERRAIN_TILES {
                        prep.result = Some(Err(DrapeError::TerrainLimit));
                        return (true, 0);
                    }
                    prep.sources = sources;
                }
                Err(error) => {
                    prep.result = Some(Err(error));
                    return (true, 0);
                }
            }
            return (false, 0);
        }
        if let Some(task) = &mut prep.task {
            if let Some(result) = block_on(poll_once(task)) {
                prep.result = Some(result);
                prep.task = None;
            }
            // Upload on a subsequent update so polling never admits an entire
            // completed job outside the remaining mesh/vertex allowance.
            return (false, 0);
        }
        let mut copied = 0;
        for _ in 0..budget {
            let Some(source) = prep.sources.get(prep.next_source) else {
                break;
            };
            let index_charge = snapshot_index_charge(meshes, source);
            if !snapshot_fits_frame(&self.frame_work, source.surface_vertices, index_charge) {
                break;
            }
            // Reserve complete bounded index buffers before validation. A
            // rejected snapshot consumes its attempt/reservation, without
            // claiming that its indices were actually inspected.
            copied += 1;
            self.frame_work.copy_attempts += 1;
            self.frame_work.copy_indices_charged += index_charge;
            match copy_terrain(
                meshes,
                source,
                prep.copied_vertices,
                &mut self.frame_work.copy_indices_scanned,
            ) {
                Ok(terrain) => {
                    prep.precision.push(PrecisionSource {
                        footprint: source.footprint,
                        origin: source.origin,
                        extent: terrain
                            .positions
                            .iter()
                            .map(|point| glam::DVec3::from_array(point.map(f64::from)).length())
                            .fold(0.0, f64::max),
                    });
                    prep.required_tiles += usize::from(source.required);
                    prep.copied_vertices += terrain.positions.len();
                    self.frame_work.copied_vertices += terrain.positions.len();
                    prep.terrain.push(terrain);
                }
                Err(error) if source.required => {
                    prep.result = Some(Err(error));
                    return (true, copied);
                }
                Err(_) => {
                    prep.optional_disabled = true;
                    prep.next_source = prep.sources.len();
                    break;
                }
            }
            prep.next_source += 1;
        }
        if prep.next_source == prep.sources.len() {
            let sources: Vec<_> = self
                .registered
                .iter()
                .map(|overlay| (Arc::clone(&overlay.source), overlay.optional))
                .collect();
            let terrain = std::mem::take(&mut prep.terrain);
            let cancelled = Arc::clone(&prep.cancelled);
            let optional_disabled = prep.optional_disabled;
            let required_tiles = prep.required_tiles;
            prep.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                Ok(prepare_overlays(
                    &sources,
                    &terrain,
                    required_tiles,
                    &cancelled,
                    optional_disabled,
                ))
            }));
        }
        (false, copied)
    }

    fn upload(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        budget: usize,
    ) -> (bool, usize) {
        let prep = self.preparation.as_mut().expect("pending preparation");
        let Some(Ok(result)) = &mut prep.result else {
            return (true, 0);
        };
        if result.reuse {
            return (true, 0);
        }
        if prep.uploaded.is_empty() {
            prep.uploaded.resize_with(result.outputs.len(), || None);
        }
        let mut attempts = 0;
        while prep.next_upload < result.outputs.len() {
            let index = prep.next_upload;
            let Some(output) = result.outputs[index].as_ref() else {
                prep.next_upload += 1;
                continue;
            };
            let vertices = output.mesh.count_vertices();
            if attempts == budget
                || (self.frame_work.uploaded_meshes != 0
                    && vertices
                        > OVERLAY_UPLOAD_VERTEX_TARGET
                            .saturating_sub(self.frame_work.uploaded_vertices))
            {
                break;
            }
            attempts += 1;
            self.frame_work.upload_attempts += 1;
            assert!(vertices <= crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES);
            let output = result.outputs[index].take().unwrap();
            let mesh = meshes.add(output.mesh);
            let owner = commands
                .spawn((
                    OwnedOverlayMesh(mesh.clone()),
                    ChildOf(self.registered[index].entity),
                ))
                .id();
            prep.uploaded[index] = Some(UploadedOverlay {
                owner,
                target: self.registered[index].entity,
                mesh,
                precision: output.precision,
                vertices,
            });
            self.frame_work.uploaded_meshes += 1;
            self.frame_work.uploaded_vertices += vertices;
            prep.next_upload += 1;
        }
        (prep.next_upload == result.outputs.len(), attempts)
    }

    pub(crate) fn commit(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        let Some(mut prep) = self.preparation.take() else {
            return;
        };
        let result = prep
            .result
            .take()
            .expect("commit only after preparation is ready");
        assert_eq!(
            prep.revision, self.revision,
            "stale overlay preparation cannot commit"
        );
        self.committed_revision = self.revision;
        self.committed_signature = Some(std::mem::take(&mut prep.signature));
        if result.as_ref().is_ok_and(|result| result.reuse) {
            return;
        }
        for overlay in &self.retired_optional {
            commands.entity(overlay.entity).insert(Visibility::Hidden);
        }
        for (_, upload) in std::mem::take(&mut self.visible_uploads) {
            release_upload(commands, meshes, upload);
        }
        self.retired_optional.clear();
        self.optional_swap_pending = false;
        self.committed_precision = std::mem::take(&mut prep.precision);
        self.displayed.clear();
        self.output_precision.clear();
        self.precision_hidden.clear();
        self.omitted_optional.clear();
        match result {
            Ok(result) => {
                assert_eq!(
                    prep.next_upload,
                    result.outputs.len(),
                    "commit only after all uploads"
                );
                let omitted: usize = prep
                    .uploaded
                    .iter()
                    .flatten()
                    .map(|output| output.precision.omitted_support_facets)
                    .sum();
                if omitted > 0 {
                    info!(
                        "ground overlays omitted {omitted} steep support candidates (>60 deg); supported facets retained"
                    );
                }
                for (group, error) in result.errors.into_iter().enumerate() {
                    if let Some(error) = error {
                        self.rejected_transactions = self.rejected_transactions.saturating_add(1);
                        if group == 0 {
                            warn!(
                                "required airport overlay group rejected: {error}; hiding that group for this cut"
                            );
                        } else {
                            let total = self
                                .registered
                                .iter()
                                .filter(|overlay| overlay.optional)
                                .count();
                            let omitted = self
                                .registered
                                .iter()
                                .zip(&prep.uploaded)
                                .filter(|(overlay, output)| overlay.optional && output.is_none())
                                .count();
                            warn!(
                                "optional terrain overlays retained {}, omitted {omitted} of {total}; first rejection: {error}",
                                total - omitted
                            );
                        }
                    }
                }
                for (overlay, output) in self
                    .registered
                    .iter()
                    .zip(std::mem::take(&mut prep.uploaded))
                {
                    if let Some(output) = output {
                        // The immutable template already captured authored
                        // geometry. Keeping its source Mesh would duplicate
                        // every displayed cohort indefinitely. Owner cleanup
                        // may still remove this original handle harmlessly.
                        meshes.remove(&overlay.mesh);
                        self.output_precision
                            .insert(overlay.entity, output.precision.clone());
                        self.displayed.insert(overlay.entity);
                        commands
                            .entity(overlay.entity)
                            .insert((Mesh3d(output.mesh.clone()), Visibility::Inherited));
                        self.visible_uploads.insert(overlay.entity, output);
                    } else {
                        if overlay.optional {
                            self.omitted_optional.insert(overlay.entity);
                        }
                        commands.entity(overlay.entity).insert(Visibility::Hidden);
                    }
                }
            }
            Err(error) => {
                self.rejected_transactions = self.rejected_transactions.saturating_add(1);
                warn!(
                    "airport overlay preparation rejected: {error}; hiding overlays for this terrain cut"
                );
                for overlay in &self.registered {
                    if overlay.optional {
                        self.omitted_optional.insert(overlay.entity);
                    }
                    commands.entity(overlay.entity).insert(Visibility::Hidden);
                }
            }
        }
    }

    /// Re-evaluate conservative f32 transform headroom when the render frame
    /// moves/rebases. A coarse distant origin is not equivalent to exact f64
    /// facets. Never replace a small layer by an arbitrary large vertical lift.
    pub(crate) fn apply_precision(&mut self, commands: &mut Commands, frame: &RenderFrame) {
        for overlay in self.registered.iter().chain(&self.retired_optional) {
            if !self.displayed.contains(&overlay.entity) {
                continue;
            }
            let metrics = self.output_precision.get(&overlay.entity);
            let output_extent =
                metrics.map_or(overlay.source.radius.get(), |metrics| metrics.output_extent);
            let support_cosine = metrics.map_or(1.0, |metrics| metrics.support_cosine);
            let error = self
                .committed_precision
                .iter()
                .enumerate()
                .filter(|(index, source)| {
                    metrics.map_or_else(
                        || {
                            source
                                .footprint
                                .iter()
                                .any(|&id| overlay.source.intersects_tile(id))
                        },
                        |metrics| metrics.terrain_sources.contains(index),
                    )
                })
                .map(|(_, source)| {
                    transform_error_bound(
                        frame,
                        source.origin,
                        source.extent,
                        overlay.source.origin,
                        output_extent,
                    )
                })
                .fold(0.0, f64::max);
            let sufficient = error < overlay.source.minimum_lift() * support_cosine * 0.5;
            if sufficient {
                if self.precision_hidden.remove(&overlay.entity) {
                    commands
                        .entity(overlay.entity)
                        .insert(Visibility::Inherited);
                }
            } else if self.precision_hidden.insert(overlay.entity) {
                commands.entity(overlay.entity).insert(Visibility::Hidden);
                warn!(
                    "terrain overlay hidden: f32 bound {error:.4} m, support cosine {support_cosine:.6}, authored layer {:.4} m",
                    overlay.source.minimum_lift()
                );
            }
        }
    }

    fn cancel_preparation(&mut self) {
        if let Some(mut prep) = self.preparation.take() {
            prep.cancelled.store(true, Ordering::Relaxed);
            self.cancelled_uploads
                .extend(std::mem::take(&mut prep.uploaded).into_iter().flatten());
            drop(prep);
        }
    }

    /// The terrain reset caller can synchronously release all unpublished GPU
    /// assets without removing the still registered/visible airport roots.
    pub(crate) fn drain_cancelled_uploads(
        &mut self,
    ) -> impl Iterator<Item = (Entity, Handle<Mesh>)> + '_ {
        self.cancelled_uploads
            .drain(..)
            .map(|upload| (upload.owner, upload.mesh))
    }

    pub(crate) fn reset(&mut self) {
        self.cancel_preparation();
        self.committed_signature = None;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("overlay generation exhausted");
    }
}

fn release_upload(commands: &mut Commands, meshes: &mut Assets<Mesh>, upload: UploadedOverlay) {
    if let Some(mesh) = meshes.remove(&upload.mesh) {
        drop(mesh);
    }
    commands.entity(upload.owner).try_despawn();
}

/// Bound both actual GPU affine evaluations relative to the canonical f64
/// facets. The measured matrix discrepancy includes quaternion conversion;
/// rounded reference basis entries add a 1.5-epsilon Frobenius bound. Four
/// epsilon covers seven float multiply/add roundings (u=epsilon/2), using the
/// sqrt(3) bound for the absolute rotation matrix norm, plus translation and
/// output-vertex encoding. Keeping half the normal-direction lift is additional
/// headroom for driver arithmetic; coarse million-metre roots remain rejected.
fn transform_error_bound(
    frame: &RenderFrame,
    source_origin: Ecef,
    source_extent: f64,
    overlay_origin: Ecef,
    overlay_extent: f64,
) -> f64 {
    let rotation = glam::Mat3::from_quat(frame.rotation_to_render(glam::DQuat::IDENTITY));
    let reference = glam::Mat3::from_cols(
        frame.vector_to_render(glam::DVec3::X),
        frame.vector_to_render(glam::DVec3::Y),
        frame.vector_to_render(glam::DVec3::Z),
    );
    let difference = rotation
        .to_cols_array()
        .into_iter()
        .zip(reference.to_cols_array())
        .map(|(actual, reference)| (f64::from(actual) - f64::from(reference)).powi(2))
        .sum::<f64>()
        .sqrt();
    let epsilon = f64::from(f32::EPSILON);
    let extent = source_extent + overlay_extent;
    let translations = frame.to_render(source_origin).as_dvec3().length()
        + frame.to_render(overlay_origin).as_dvec3().length();
    (difference + 1.5 * epsilon) * extent
        + 0.5 * epsilon * (translations + overlay_extent)
        + 4.0 * epsilon * (3.0_f64.sqrt() * extent + translations)
        + 1.0e-7
}

fn prepare_overlays(
    sources: &[(Arc<TerrainOverlay>, bool)],
    terrain: &[DrapeTerrain],
    required_tiles: usize,
    cancelled: &AtomicBool,
    optional_disabled: bool,
) -> PreparedOverlays {
    prepare_overlays_with_budget(
        sources,
        terrain,
        required_tiles,
        cancelled,
        optional_disabled,
        crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
        0,
    )
}

// A smaller output/remaining-work allowance makes the aggregate failure policy
// directly testable without allocating production-sized meshes. Production uses
// the unchanged hard cap and starts all charged work at zero.
fn prepare_overlays_with_budget(
    sources: &[(Arc<TerrainOverlay>, bool)],
    terrain: &[DrapeTerrain],
    required_tiles: usize,
    cancelled: &AtomicBool,
    optional_disabled: bool,
    output_limit: usize,
    initial_work: usize,
) -> PreparedOverlays {
    let mut result = PreparedOverlays {
        reuse: false,
        outputs: (0..sources.len()).map(|_| None).collect(),
        errors: [None; 2],
    };
    let output_limit = output_limit.min(crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES);
    let mut vertices = 0;
    let mut work = initial_work;
    for optional in [false, true] {
        let group = usize::from(optional);
        if optional && optional_disabled {
            result.errors[group] = Some(DrapeError::TerrainLimit);
            continue;
        }
        let terrain = if optional {
            terrain
        } else {
            &terrain[..required_tiles]
        };
        for (index, (source, is_optional)) in sources
            .iter()
            .enumerate()
            .filter(|(_, (_, is_optional))| *is_optional == optional)
        {
            debug_assert_eq!(*is_optional, optional);
            if output_limit - vertices < 3 {
                result.errors[group].get_or_insert(DrapeError::OutputLimit);
                break; // No later complete triangle can fit either.
            }
            match source.drape_cancellable(terrain, cancelled, &mut work, output_limit - vertices) {
                Ok(output) => {
                    vertices += output.mesh.count_vertices();
                    debug_assert!(vertices <= output_limit);
                    result.outputs[index] = Some(output);
                }
                Err(error) => {
                    result.errors[group].get_or_insert(error);
                    // Airports are one atomic required group. Optional batches
                    // follow caller priority order and fail independently. All
                    // failed conversion/intersection work remains charged; a
                    // spent shared work budget cannot be reset for later items.
                    if !optional || matches!(error, DrapeError::WorkLimit | DrapeError::Cancelled) {
                        break;
                    }
                }
            }
        }
        if !optional && result.errors[group].is_some() {
            for (index, (_, is_optional)) in sources.iter().enumerate() {
                if !is_optional {
                    result.outputs[index] = None;
                }
            }
            // Discarded required output consumes no committed geometry budget.
            // Its CPU work is deliberately not refunded.
            vertices = 0;
        }
    }
    result
}

/// O(1) metadata only: do not clone/scan a candidate to decide its budget.
/// Buffers above the unchanged per-source cap are rejected before scanning and
/// reserve zero here. Other validation failures may reserve but not scan.
fn snapshot_index_charge(meshes: &Assets<Mesh>, source: &OverlayTerrainSource) -> usize {
    meshes
        .get(&source.mesh)
        .and_then(Mesh::indices)
        .map_or(0, |indices| {
            if indices.len() <= MAX_OVERLAY_TERRAIN_INDICES {
                indices.len()
            } else {
                0
            }
        })
}

fn snapshot_fits_frame(work: &TerrainOverlayFrameWork, vertices: usize, indices: usize) -> bool {
    if indices > OVERLAY_COPY_INDICES_PER_FRAME.saturating_sub(work.copy_indices_charged) {
        return false;
    }
    if work.copy_attempts == 0 {
        // One indivisible source always gets a turn. The unchanged source and
        // transaction checks reject oversized/invalid geometry before copying.
        return true;
    }
    work.copied_vertices <= OVERLAY_COPY_VERTEX_TARGET
        && work.copy_indices_charged <= OVERLAY_COPY_INDEX_TARGET
        && vertices <= OVERLAY_COPY_VERTEX_TARGET.saturating_sub(work.copied_vertices)
        && indices <= OVERLAY_COPY_INDEX_TARGET.saturating_sub(work.copy_indices_charged)
}

fn copy_terrain(
    meshes: &Assets<Mesh>,
    source: &OverlayTerrainSource,
    already_copied: usize,
    scanned_indices: &mut usize,
) -> Result<DrapeTerrain, DrapeError> {
    if source.surface_vertices > MAX_OVERLAY_TERRAIN_VERTICES.saturating_sub(already_copied) {
        return Err(DrapeError::TerrainLimit);
    }
    let mesh = meshes.get(&source.mesh).ok_or(DrapeError::InvalidMesh)?;
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err(DrapeError::InvalidMesh);
    };
    let Some(indices) = mesh.indices() else {
        return Err(DrapeError::InvalidMesh);
    };
    if source.surface_vertices > positions.len() {
        return Err(DrapeError::InvalidMesh);
    }
    if indices.len() > MAX_OVERLAY_TERRAIN_INDICES || indices.len() % 3 != 0 {
        return Err(DrapeError::TerrainLimit);
    }
    let mut surface_indices = Vec::new();
    let mut indices = indices.iter();
    while let (Some(a), Some(b), Some(c)) = (indices.next(), indices.next(), indices.next()) {
        *scanned_indices += 3;
        let triangle = [a, b, c];
        if triangle
            .iter()
            .all(|&index| index < source.surface_vertices)
        {
            for index in triangle {
                surface_indices.push(u32::try_from(index).map_err(|_| DrapeError::InvalidMesh)?);
            }
        }
    }
    Ok(DrapeTerrain {
        origin: source.origin,
        footprint: Some(source.footprint),
        positions: positions[..source.surface_vertices].to_vec(),
        indices: surface_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Geodetic, Meters, Radians};

    fn registration(world: &mut World, meshes: &mut Assets<Mesh>) -> TerrainOverlayRegistration {
        let geo = Geodetic::from_degrees(35.55, 139.78, 40.0);
        let (mesh, origin) =
            crate::runway::runway_mesh(geo, Radians::ZERO, Meters(100.0), Meters(20.0));
        let source = TerrainOverlay::surface(&mesh, origin, |_| Meters(40.0)).unwrap();
        TerrainOverlayRegistration {
            entity: world.spawn(Visibility::Hidden).id(),
            mesh: meshes.add(mesh),
            source,
        }
    }

    #[test]
    fn optional_admission_failure_does_not_mutate_airport_registry() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut registry = TerrainOverlays::default();
        let airport = registration(&mut world, &mut meshes);
        registry
            .register(airport.entity, airport.mesh, airport.source)
            .unwrap();
        let before = registry.usage();
        let too_many = (0..25)
            .map(|_| registration(&mut world, &mut meshes))
            .collect();
        assert!(matches!(
            registry.replace_optional(too_many),
            Err(DrapeError::SourceLimit)
        ));
        assert_eq!(registry.usage(), before);
    }

    #[test]
    fn first_optional_swap_is_busy_even_without_a_retired_generation_and_clear_cancels_it() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut registry = TerrainOverlays::default();
        let first = registration(&mut world, &mut meshes);
        let entity = first.entity;
        assert!(
            registry
                .replace_optional(vec![first])
                .unwrap()
                .retired
                .is_empty()
        );
        assert!(matches!(
            registry.replace_optional(Vec::new()),
            Err(DrapeError::UpdatePending)
        ));
        let removed = registry.clear_optional();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0, entity);
        assert!(!registry.usage().optional_swap_pending);
        assert!(registry.replace_optional(Vec::new()).is_ok());
    }

    #[test]
    fn optional_candidate_failure_cannot_consume_airport_work_or_hide_its_output() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let airport = registration(&mut world, &mut meshes);
        let optional = registration(&mut world, &mut meshes);
        let id = TileId::containing(13, airport.source.origin.to_geodetic());
        let dem = flightsim_world::DemTile::new(
            id.bounds(),
            flightsim_world::HeightGrid::flat(2, 2, Meters(40.0)),
        );
        let source = flightsim_world::build_mesh(
            id,
            &dem,
            &flightsim_world::MeshOptions {
                resolution: 2,
                skirt_depth: Some(Meters::ZERO),
            },
        );
        let terrain = DrapeTerrain {
            origin: source.origin,
            footprint: None,
            positions: source.positions,
            indices: source.indices,
        };
        let mut dense = terrain.clone();
        dense.indices = dense.indices.repeat(16_385);
        let sources = [
            (Arc::new(airport.source), false),
            (Arc::new(optional.source), true),
        ];
        let result = prepare_overlays(
            &sources,
            &[terrain, dense],
            1,
            &AtomicBool::new(false),
            false,
        );
        assert!(
            result.outputs[0]
                .as_ref()
                .is_some_and(|output| output.mesh.count_vertices() > 0)
        );
        assert!(result.outputs[1].is_none());
        assert_eq!(result.errors[0], None);
        assert_eq!(result.errors[1], Some(DrapeError::TerrainLimit));
    }

    fn support_for(source: &TerrainOverlay) -> DrapeTerrain {
        let id = TileId::containing(13, source.origin.to_geodetic());
        let mesh = flightsim_world::build_mesh(
            id,
            &flightsim_world::DemTile::new(
                id.bounds(),
                flightsim_world::HeightGrid::flat(2, 2, Meters(40.0)),
            ),
            &flightsim_world::MeshOptions {
                resolution: 2,
                skirt_depth: Some(Meters::ZERO),
            },
        );
        DrapeTerrain {
            origin: mesh.origin,
            footprint: None,
            positions: mesh.positions,
            indices: mesh.indices,
        }
    }

    fn queue_result(registry: &mut TerrainOverlays, result: PreparedOverlays) -> Arc<AtomicBool> {
        let cancelled = Arc::new(AtomicBool::new(false));
        registry.preparation = Some(Preparation {
            revision: registry.revision,
            signature: Vec::new(),
            sources: Vec::new(),
            discovery: None,
            required_tiles: 0,
            next_source: 0,
            copied_vertices: 0,
            terrain: Vec::new(),
            precision: Vec::new(),
            optional_disabled: false,
            task: None,
            result: Some(Ok(result)),
            next_upload: 0,
            uploaded: Vec::new(),
            cancelled: Arc::clone(&cancelled),
        });
        cancelled
    }

    fn upload_ready(registry: &mut TerrainOverlays, world: &mut World, meshes: &mut Assets<Mesh>) {
        for _ in 0..MAX_TERRAIN_OVERLAYS {
            registry.begin_frame(&mut world.commands(), meshes);
            let (ready, attempts) = registry.advance(&mut world.commands(), meshes, 1);
            assert!(attempts <= 1);
            world.flush();
            if ready {
                return;
            }
        }
        panic!("bounded uploads did not finish");
    }

    fn sized_outputs(sizes: &[usize]) -> PreparedOverlays {
        PreparedOverlays {
            reuse: false,
            outputs: sizes
                .iter()
                .map(|&vertices| {
                    Some(DrapedOverlay {
                        mesh: Mesh::new(
                            bevy::mesh::PrimitiveTopology::TriangleList,
                            bevy::asset::RenderAssetUsages::default(),
                        )
                        .with_inserted_attribute(
                            Mesh::ATTRIBUTE_POSITION,
                            vec![[0.0, 0.0, 0.0]; vertices],
                        ),
                        precision: OverlayPrecision {
                            terrain_sources: Vec::new(),
                            support_cosine: 1.0,
                            output_extent: 1.0,
                            authored_displacement: Meters::ZERO,
                            omitted_support_facets: 0,
                        },
                    })
                })
                .collect(),
            errors: [None; 2],
        }
    }

    type UploadFixture = (
        TerrainOverlays,
        World,
        Assets<Mesh>,
        Vec<(Entity, Handle<Mesh>)>,
    );

    fn upload_fixture(count: usize) -> UploadFixture {
        let mut registry = TerrainOverlays::default();
        let mut world = World::new();
        let mut meshes = Assets::default();
        let mut originals = Vec::new();
        for _ in 0..count {
            let item = registration(&mut world, &mut meshes);
            world
                .entity_mut(item.entity)
                .insert((Mesh3d(item.mesh.clone()), Visibility::Inherited));
            originals.push((item.entity, item.mesh.clone()));
            registry
                .register(item.entity, item.mesh, item.source)
                .unwrap();
        }
        (registry, world, meshes, originals)
    }

    #[test]
    fn upload_attempt_and_actual_vertex_boundaries_are_independent() {
        for (sizes, budget, expected, ready) in [
            (vec![32_768, 32_768], 2, 65_536, true),
            (vec![65_536, 1], 2, 65_536, false),
            (vec![65_537, 1], 2, 65_537, false),
            (
                vec![crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES, 1],
                2,
                crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
                false,
            ),
            (vec![3, 3], 1, 3, false),
            (vec![3, 3], 0, 0, false),
        ] {
            let (mut registry, mut world, mut meshes, originals) = upload_fixture(sizes.len());
            let baseline = meshes.len();
            queue_result(&mut registry, sized_outputs(&sizes));
            registry.begin_frame(&mut world.commands(), &mut meshes);
            let (actual_ready, attempts) =
                registry.advance(&mut world.commands(), &mut meshes, budget);
            world.flush();
            let usage = registry.usage();
            assert_eq!(actual_ready, ready);
            assert!(attempts <= budget);
            assert_eq!(attempts, usage.frame_work.upload_attempts);
            assert_eq!(usage.frame_work.uploaded_vertices, expected);
            assert_eq!(meshes.len(), baseline + usage.frame_work.uploaded_meshes);
            for (entity, original) in originals {
                assert_eq!(
                    world.get::<Mesh3d>(entity).unwrap().0,
                    original,
                    "upload cannot change the displayed handle"
                );
                assert_eq!(
                    world.get::<Visibility>(entity),
                    Some(&Visibility::Inherited)
                );
            }
            if !ready && budget != 0 {
                registry.begin_frame(&mut world.commands(), &mut meshes);
                assert!(
                    registry.advance(&mut world.commands(), &mut meshes, 2).0,
                    "an indivisible maximum output must not starve later meshes"
                );
                world.flush();
            }
        }
    }

    #[test]
    fn reset_reclaims_every_partial_upload_including_ready_to_commit() {
        for staged in 0..=3 {
            let (mut registry, mut world, mut meshes, originals) = upload_fixture(3);
            let baseline = meshes.len();
            let cancelled = queue_result(&mut registry, sized_outputs(&[3, 3, 3]));
            for _ in 0..staged {
                registry.begin_frame(&mut world.commands(), &mut meshes);
                assert_eq!(registry.advance(&mut world.commands(), &mut meshes, 1).1, 1);
                world.flush();
            }
            assert_eq!(meshes.len(), baseline + staged);
            registry.reset();
            assert!(cancelled.load(Ordering::Relaxed));
            registry.commit(&mut world.commands(), &mut meshes);
            for (entity, mesh) in registry.drain_cancelled_uploads() {
                world.entity_mut(entity).despawn();
                meshes.remove(&mesh);
            }
            world.flush();
            assert_eq!(meshes.len(), baseline);
            for (entity, original) in originals {
                assert_eq!(world.get::<Mesh3d>(entity).unwrap().0, original);
                assert_eq!(
                    world.get::<Visibility>(entity),
                    Some(&Visibility::Inherited)
                );
            }
        }
    }

    #[test]
    fn cancel_and_restart_before_commands_flush_leaves_only_new_ownership() {
        let (mut registry, mut world, mut meshes, originals) = upload_fixture(1);
        let baseline = meshes.len();
        queue_result(&mut registry, sized_outputs(&[3]));
        registry.begin_frame(&mut world.commands(), &mut meshes);
        assert!(registry.advance(&mut world.commands(), &mut meshes, 1).0);
        registry.reset();
        queue_result(&mut registry, sized_outputs(&[6]));
        registry.begin_frame(&mut world.commands(), &mut meshes);
        assert!(registry.advance(&mut world.commands(), &mut meshes, 1).0);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(meshes.len(), baseline);
        let root = originals[0].0;
        assert_eq!(world.get::<Children>(root).unwrap().len(), 1);
        assert_eq!(
            meshes
                .get(&world.get::<Mesh3d>(root).unwrap().0)
                .unwrap()
                .count_vertices(),
            6
        );
    }

    #[test]
    fn generated_mesh_handles_cannot_be_admitted_as_another_owned_source() {
        let (mut registry, mut world, mut meshes, originals) = upload_fixture(1);
        queue_result(&mut registry, sized_outputs(&[3]));
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        let generated = world.get::<Mesh3d>(originals[0].0).unwrap().0.clone();
        let item = registration(&mut world, &mut meshes);
        assert_eq!(
            registry.register(item.entity, generated, item.source),
            Err(DrapeError::InvalidMesh)
        );
        assert_eq!(registry.usage().registered, 1);
    }

    #[test]
    fn repeated_cancel_and_retained_root_replacement_reclaim_generated_assets() {
        let (mut registry, mut world, mut meshes, originals) = upload_fixture(1);
        let baseline = meshes.len();
        let (root, mut original) = originals[0].clone();
        let authored = meshes.get(&original).unwrap().clone();
        let source = (*registry.registered[0].source).clone();
        for _ in 0..16 {
            queue_result(
                &mut registry,
                sized_outputs(&[OVERLAY_UPLOAD_VERTEX_TARGET]),
            );
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert!(registry.advance(&mut world.commands(), &mut meshes, 1).0);
            world.flush();
            registry.reset();
            registry.begin_frame(&mut world.commands(), &mut meshes);
            world.flush();
            assert_eq!(meshes.len(), baseline);
            queue_result(&mut registry, sized_outputs(&[3]));
            upload_ready(&mut registry, &mut world, &mut meshes);
            registry.commit(&mut world.commands(), &mut meshes);
            world.flush();
            assert_eq!(meshes.len(), baseline);
            assert_eq!(world.get::<Children>(root).unwrap().len(), 1);
            assert!(registry.unregister(root));
            original = meshes.add(authored.clone());
            world.entity_mut(root).insert(Mesh3d(original.clone()));
            registry
                .register(root, original.clone(), source.clone())
                .unwrap();
            registry.begin_frame(&mut world.commands(), &mut meshes);
            world.flush();
            assert_eq!(
                meshes.len(),
                baseline,
                "retaining the root must not retain orphaned generated meshes"
            );
            assert!(
                world
                    .get::<Children>(root)
                    .is_none_or(|children| children.is_empty())
            );
        }
    }

    /// Keep a final impossible-to-fit sentinel so copy-only tests do not start
    /// clipping jobs. It is not reached until a later otherwise-empty update.
    fn snapshot_fixture(sizes: &[(usize, usize)]) -> UploadFixture {
        let (mut registry, world, mut meshes, originals) = upload_fixture(1);
        queue_result(&mut registry, sized_outputs(&[3]));
        let origin = registry.registered[0].source.origin;
        let id = TileId::containing(13, origin.to_geodetic());
        let prep = registry.preparation.as_mut().unwrap();
        prep.result = None;
        for (index, &(vertices, indices)) in sizes.iter().enumerate() {
            let tag = f32::from(u16::try_from(index).unwrap());
            let mesh = meshes.add(
                Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    bevy::asset::RenderAssetUsages::default(),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[tag, 0.0, 0.0]; vertices])
                .with_inserted_indices(bevy::mesh::Indices::U32(vec![0; indices])),
            );
            prep.sources.push(OverlayTerrainSource {
                key: OverlaySourceKey::Tile(id, u64::try_from(index).unwrap()),
                footprint: [id, id],
                required: true,
                origin,
                surface_vertices: vertices,
                mesh,
            });
        }
        prep.sources.push(OverlayTerrainSource {
            key: OverlaySourceKey::Tile(id, u64::MAX),
            footprint: [id, id],
            required: true,
            origin,
            surface_vertices: MAX_OVERLAY_TERRAIN_VERTICES,
            mesh: Handle::default(),
        });
        (registry, world, meshes, originals)
    }

    #[test]
    fn snapshot_targets_match_two_actual_maximum_grids_including_skirt_scan() {
        let id = TileId::containing(13, Geodetic::from_degrees(47.0, 9.0, 0.0));
        let grid = flightsim_world::build_mesh(
            id,
            &flightsim_world::DemTile::new(
                id.bounds(),
                flightsim_world::HeightGrid::flat(65, 65, Meters(40.0)),
            ),
            &flightsim_world::MeshOptions {
                resolution: 65,
                skirt_depth: Some(Meters(20.0)),
            },
        );
        assert_eq!(grid.surface_vertex_count, 4_225);
        assert_eq!(grid.positions.len(), 4_225 + 4 * 64);
        assert_eq!(grid.indices.len(), 24_576 + 1_536);
        assert_eq!(OVERLAY_COPY_VERTEX_TARGET, 2 * grid.surface_vertex_count);
        assert_eq!(OVERLAY_COPY_INDEX_TARGET, 2 * grid.indices.len());
        let mut meshes = Assets::<Mesh>::default();
        let source = OverlayTerrainSource {
            key: OverlaySourceKey::Tile(id, 1),
            footprint: [id, id],
            required: true,
            origin: grid.origin,
            surface_vertices: grid.surface_vertex_count,
            mesh: meshes.add(crate::to_bevy_mesh(&grid)),
        };
        let mut scanned = 0;
        let copy = copy_terrain(&meshes, &source, 0, &mut scanned).unwrap();
        assert_eq!(copy.positions, grid.positions[..4_225]);
        assert_eq!(copy.indices, grid.indices[..24_576]);
        assert_eq!(
            scanned, 26_112,
            "discarded skirt triangles still cost index inspection"
        );
        assert_eq!(snapshot_index_charge(&meshes, &source), scanned);
    }

    #[test]
    fn snapshot_tiny_sources_pack_to_remaining_attempt_allowance_without_visibility_change() {
        for budget in [0, 1, 3, 8] {
            let (mut registry, mut world, mut meshes, originals) =
                snapshot_fixture(&[(140, 816); 9]);
            let asset_count = meshes.len();
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, budget),
                (false, budget)
            );
            world.flush();
            let work = registry.usage().frame_work;
            assert_eq!(work.copy_attempts, budget);
            assert_eq!(work.copied_vertices, 140 * budget);
            assert_eq!(work.copy_indices_charged, 816 * budget);
            assert_eq!(work.copy_indices_scanned, 816 * budget);
            assert_eq!(registry.preparation.as_ref().unwrap().next_source, budget);
            assert_eq!(meshes.len(), asset_count);
            assert_eq!(
                world.get::<Mesh3d>(originals[0].0).unwrap().0,
                originals[0].1
            );
            assert_eq!(
                world.get::<Visibility>(originals[0].0),
                Some(&Visibility::Inherited)
            );
        }
    }

    #[test]
    fn snapshot_normal_vertex_and_index_targets_defer_next_whole_source() {
        for (sizes, vertices, indices) in [
            (vec![(4_225, 26_112); 3], 8_450, 52_224),
            (vec![(8_449, 0), (1, 0), (1, 0)], 8_450, 0),
            (vec![(3, 52_221), (3, 3), (3, 3)], 6, 52_224),
        ] {
            let (mut registry, mut world, mut meshes, _) = snapshot_fixture(&sizes);
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, 8),
                (false, 2)
            );
            assert_eq!(registry.usage().frame_work.copied_vertices, vertices);
            assert_eq!(registry.usage().frame_work.copy_indices_charged, indices);
            assert_eq!(registry.usage().frame_work.copy_indices_scanned, indices);
            assert_eq!(registry.preparation.as_ref().unwrap().next_source, 2);
        }
    }

    #[test]
    fn snapshot_indivisible_first_source_progresses_without_admitting_a_second() {
        for (vertices, indices) in [
            (OVERLAY_COPY_VERTEX_TARGET + 1, 0),
            (3, OVERLAY_COPY_INDEX_TARGET + 3),
            (3, MAX_OVERLAY_TERRAIN_INDICES),
        ] {
            let (mut registry, mut world, mut meshes, _) =
                snapshot_fixture(&[(vertices, indices), (1, 0)]);
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, 8),
                (false, 1)
            );
            let work = registry.usage().frame_work;
            assert_eq!(work.copied_vertices, vertices);
            assert_eq!(work.copy_indices_charged, indices);
            assert_eq!(work.copy_indices_scanned, indices);
            assert!(work.copied_vertices <= OVERLAY_COPY_VERTICES_PER_FRAME);
            assert!(work.copy_indices_charged <= OVERLAY_COPY_INDICES_PER_FRAME);
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, 8),
                (false, 1)
            );
            assert_eq!(registry.usage().frame_work.copied_vertices, 1);
        }
    }

    #[test]
    fn snapshot_charged_indices_are_distinct_from_scanned_entries_on_early_failure() {
        for (vertices, indices, remove_positions, expected_charge) in [
            (3, 12, true, 12),
            (
                3,
                OVERLAY_COPY_INDEX_TARGET + 1,
                false,
                OVERLAY_COPY_INDEX_TARGET + 1,
            ),
            (3, MAX_OVERLAY_TERRAIN_INDICES + 1, false, 0),
            (MAX_OVERLAY_TERRAIN_VERTICES + 1, 3, false, 3),
        ] {
            let (mut registry, mut world, mut meshes, originals) =
                snapshot_fixture(&[(vertices, indices)]);
            if remove_positions {
                let mesh = registry.preparation.as_ref().unwrap().sources[0]
                    .mesh
                    .clone();
                meshes
                    .get_mut(&mesh)
                    .unwrap()
                    .remove_attribute(Mesh::ATTRIBUTE_POSITION);
            }
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, 8),
                (true, 1)
            );
            let work = registry.usage().frame_work;
            assert_eq!(work.copy_attempts, 1);
            assert_eq!(work.copied_vertices, 0);
            assert_eq!(work.copy_indices_charged, expected_charge);
            assert_eq!(work.copy_indices_scanned, 0);
            world.flush();
            assert_eq!(
                world.get::<Mesh3d>(originals[0].0).unwrap().0,
                originals[0].1
            );
        }
    }

    #[test]
    fn snapshot_packed_prefix_preserves_required_optional_failure_policy_and_charges() {
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        for required in [true, false] {
            let (mut registry, mut world, mut meshes, originals) =
                snapshot_fixture(&[(140, 816); 5]);
            let prep = registry.preparation.as_mut().unwrap();
            if !required {
                // Production discovery sorts all required sources before the
                // optional suffix; do not put required work after this failure.
                for source in &mut prep.sources[2..] {
                    source.required = false;
                }
            }
            let rejected = prep.sources[2].mesh.clone();
            meshes
                .get_mut(&rejected)
                .unwrap()
                .remove_attribute(Mesh::ATTRIBUTE_POSITION);
            let cancelled = Arc::clone(&prep.cancelled);
            let assets = meshes.len();
            registry.begin_frame(&mut world.commands(), &mut meshes);
            assert_eq!(
                registry.advance(&mut world.commands(), &mut meshes, 8),
                (required, 3)
            );
            let work = registry.usage().frame_work;
            assert_eq!(work.copy_attempts, 3);
            assert_eq!(work.copied_vertices, 280);
            assert_eq!(work.copy_indices_charged, 3 * 816);
            assert_eq!(work.copy_indices_scanned, 2 * 816);
            let prep = registry.preparation.as_ref().unwrap();
            assert_eq!(prep.copied_vertices, 280);
            assert_eq!(prep.required_tiles, 2);
            if required {
                assert!(matches!(prep.result, Some(Err(DrapeError::InvalidMesh))));
                assert_eq!(prep.next_source, 2);
            } else {
                assert!(prep.optional_disabled);
                assert_eq!(prep.next_source, prep.sources.len());
                assert!(prep.task.is_some());
            }
            registry.reset();
            assert!(cancelled.load(Ordering::Relaxed));
            world.flush();
            assert_eq!(meshes.len(), assets);
            assert_eq!(
                world.get::<Mesh3d>(originals[0].0).unwrap().0,
                originals[0].1
            );
            assert_eq!(
                world.get::<Visibility>(originals[0].0),
                Some(&Visibility::Inherited)
            );
        }
    }

    #[test]
    fn snapshot_packing_keeps_exact_source_order_and_geometry_across_cadences() {
        let mut expected = None;
        for budget in [1, 2, 8] {
            let (mut registry, mut world, mut meshes, originals) =
                snapshot_fixture(&[(140, 816); 9]);
            let cancelled = Arc::clone(&registry.preparation.as_ref().unwrap().cancelled);
            let mut frames = 0;
            while registry.preparation.as_ref().unwrap().next_source < 9 {
                registry.begin_frame(&mut world.commands(), &mut meshes);
                let (ready, attempts) =
                    registry.advance(&mut world.commands(), &mut meshes, budget);
                assert!(!ready);
                assert!(attempts <= budget);
                frames += 1;
                assert!(frames <= 9);
                let work = registry.usage().frame_work;
                assert!(work.copy_indices_scanned <= work.copy_indices_charged);
                assert!(work.copy_indices_charged <= OVERLAY_COPY_INDICES_PER_FRAME);
            }
            assert_eq!(frames, 9_usize.div_ceil(budget));
            let geometry: Vec<_> = registry
                .preparation
                .as_ref()
                .unwrap()
                .terrain
                .iter()
                .map(|terrain| {
                    (
                        terrain.origin,
                        terrain.footprint,
                        terrain.positions.clone(),
                        terrain.indices.clone(),
                    )
                })
                .collect();
            if let Some(expected) = &expected {
                assert_eq!(&geometry, expected);
            } else {
                expected = Some(geometry);
            }
            registry.reset();
            assert!(cancelled.load(Ordering::Relaxed));
            registry.begin_frame(&mut world.commands(), &mut meshes);
            world.flush();
            assert!(!registry.has_preparation());
            assert_eq!(
                world.get::<Mesh3d>(originals[0].0).unwrap().0,
                originals[0].1
            );
            assert_eq!(
                world.get::<Visibility>(originals[0].0),
                Some(&Visibility::Inherited)
            );
            assert_eq!(
                registry.usage().frame_work,
                TerrainOverlayFrameWork::default()
            );
        }
    }

    #[test]
    fn snapshot_actual_vertex_cap_charges_rejected_excess_on_next_frame() {
        let (mut registry, mut world, mut meshes, _) = upload_fixture(1);
        queue_result(&mut registry, sized_outputs(&[3]));
        let origin = registry.registered[0].source.origin;
        let id = TileId::containing(13, origin.to_geodetic());
        let prep = registry.preparation.as_mut().unwrap();
        prep.result = None;
        for vertices in [MAX_OVERLAY_TERRAIN_VERTICES, 1] {
            let mesh = meshes.add(
                Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    bevy::asset::RenderAssetUsages::default(),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; vertices])
                .with_inserted_indices(bevy::mesh::Indices::U32(Vec::new())),
            );
            prep.sources.push(OverlayTerrainSource {
                key: OverlaySourceKey::Tile(id, 1),
                footprint: [id, id],
                required: true,
                origin,
                surface_vertices: vertices,
                mesh,
            });
        }
        registry.begin_frame(&mut world.commands(), &mut meshes);
        assert_eq!(
            registry.advance(&mut world.commands(), &mut meshes, 2),
            (false, 1)
        );
        assert_eq!(
            registry.usage().frame_work.copied_vertices,
            MAX_OVERLAY_TERRAIN_VERTICES
        );
        registry.begin_frame(&mut world.commands(), &mut meshes);
        assert_eq!(
            registry.advance(&mut world.commands(), &mut meshes, 2),
            (true, 1)
        );
        assert_eq!(registry.usage().frame_work.copied_vertices, 0);
        assert_eq!(registry.usage().frame_work.copy_attempts, 1);
        assert!(matches!(
            registry.preparation.as_ref().unwrap().result,
            Some(Err(DrapeError::TerrainLimit))
        ));
    }

    #[test]
    fn failed_snapshot_attempts_are_charged_including_optional_sources() {
        for required in [false, true] {
            let (mut registry, mut world, mut meshes, _) = upload_fixture(1);
            queue_result(&mut registry, sized_outputs(&[3]));
            let prep = registry.preparation.as_mut().unwrap();
            prep.result = None;
            let id = TileId::containing(13, registry.registered[0].source.origin.to_geodetic());
            prep.sources.push(OverlayTerrainSource {
                key: OverlaySourceKey::Tile(id, 1),
                footprint: [id, id],
                required,
                origin: registry.registered[0].source.origin,
                surface_vertices: 3,
                mesh: Handle::default(),
            });
            registry.begin_frame(&mut world.commands(), &mut meshes);
            // Optional rejection starts a worker after exhausting source copies.
            bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
            let (_, attempts) = registry.advance(&mut world.commands(), &mut meshes, 1);
            assert_eq!(attempts, 1);
            assert_eq!(registry.usage().frame_work.copy_attempts, 1);
            assert_eq!(registry.usage().frame_work.copied_vertices, 0);
            registry.reset();
        }
    }

    #[test]
    fn optional_output_failure_preserves_prior_and_later_smaller_batches() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let small = registration(&mut world, &mut meshes);
        let terrain = [support_for(&small.source)];
        let mut work = 0;
        let expected = small
            .source
            .drape_cancellable(
                &terrain,
                &AtomicBool::new(false),
                &mut work,
                crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap()
            .mesh
            .count_vertices();
        assert!(expected > 0);
        let mut large_mesh = meshes.get(&small.mesh).unwrap().clone();
        let indices: Vec<_> = large_mesh
            .indices()
            .unwrap()
            .iter()
            .map(|i| u32::try_from(i).unwrap())
            .collect();
        large_mesh.insert_indices(bevy::mesh::Indices::U32(indices.repeat(4)));
        let large = Arc::new(
            TerrainOverlay::surface(&large_mesh, small.source.origin, |_| Meters(40.0)).unwrap(),
        );
        let small = Arc::new(small.source);
        let sources = [
            (Arc::clone(&small), false),
            (Arc::clone(&small), true),
            (large, true),
            (Arc::clone(&small), true),
        ];
        let result = prepare_overlays_with_budget(
            &sources,
            &terrain,
            1,
            &AtomicBool::new(false),
            false,
            expected * 3,
            0,
        );
        assert_eq!(result.errors, [None, Some(DrapeError::OutputLimit)]);
        assert_eq!(
            result
                .outputs
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            [true, true, false, true]
        );
        assert_eq!(
            result
                .outputs
                .iter()
                .flatten()
                .map(|o| o.mesh.count_vertices())
                .sum::<usize>(),
            expected * 3
        );
    }

    #[test]
    fn exhausted_optional_work_keeps_completed_outputs_and_never_refunds_failed_work() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let small = Arc::new(registration(&mut world, &mut meshes).source);
        let terrain = [support_for(&small)];
        let mut used = 0;
        small
            .drape_cancellable(
                &terrain,
                &AtomicBool::new(false),
                &mut used,
                crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap();
        assert!(used > 1);
        let sources = [
            (Arc::clone(&small), false),
            (Arc::clone(&small), true),
            (Arc::clone(&small), true),
            (Arc::clone(&small), true),
        ];
        let result = prepare_overlays_with_budget(
            &sources,
            &terrain,
            1,
            &AtomicBool::new(false),
            false,
            crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
            crate::terrain_drape::MAX_OVERLAY_INTERSECTIONS - 2 * used,
        );
        assert_eq!(result.errors, [None, Some(DrapeError::WorkLimit)]);
        assert_eq!(
            result
                .outputs
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            [true, true, false, false]
        );
    }

    #[test]
    fn required_output_failure_remains_atomic_but_releases_discarded_vertex_capacity() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let small = Arc::new(registration(&mut world, &mut meshes).source);
        let terrain = [support_for(&small)];
        let expected = small
            .drape_cancellable(
                &terrain,
                &AtomicBool::new(false),
                &mut 0,
                crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap()
            .mesh
            .count_vertices();
        let sources = [
            (Arc::clone(&small), false),
            (Arc::clone(&small), false),
            (small, true),
        ];
        let result = prepare_overlays_with_budget(
            &sources,
            &terrain,
            1,
            &AtomicBool::new(false),
            false,
            expected,
            0,
        );
        assert_eq!(result.errors, [Some(DrapeError::OutputLimit), None]);
        assert_eq!(
            result
                .outputs
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn partial_optional_commit_recovery_and_cancel_clear_omission_state() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut registry = TerrainOverlays::default();
        let required = registration(&mut world, &mut meshes);
        let terrain = [support_for(&required.source)];
        let required_entity = required.entity;
        registry
            .register(required.entity, required.mesh, required.source)
            .unwrap();
        let optional = (0..2)
            .map(|_| registration(&mut world, &mut meshes))
            .collect::<Vec<_>>();
        let optional_entities: Vec<_> = optional.iter().map(|r| r.entity).collect();
        registry.replace_optional(optional).unwrap();
        let sources: Vec<_> = registry
            .registered
            .iter()
            .map(|r| (Arc::clone(&r.source), r.optional))
            .collect();
        let size = sources[0]
            .0
            .drape_cancellable(
                &terrain,
                &AtomicBool::new(false),
                &mut 0,
                crate::terrain_drape::MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap()
            .mesh
            .count_vertices();
        let partial = prepare_overlays_with_budget(
            &sources,
            &terrain,
            1,
            &AtomicBool::new(false),
            false,
            2 * size,
            0,
        );
        queue_result(&mut registry, partial);
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(registry.usage().omitted_optional, 1);
        assert_eq!(
            world.get::<Visibility>(optional_entities[0]),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            world.get::<Visibility>(optional_entities[1]),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            world.get::<Visibility>(required_entity),
            Some(&Visibility::Inherited)
        );
        queue_result(
            &mut registry,
            PreparedOverlays {
                reuse: true,
                outputs: Vec::new(),
                errors: [None; 2],
            },
        );
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(
            registry.usage().omitted_optional,
            1,
            "unchanged-cut reuse preserves omission state"
        );
        assert_eq!(
            world.get::<Visibility>(optional_entities[0]),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            world.get::<Visibility>(optional_entities[1]),
            Some(&Visibility::Hidden)
        );
        let complete = prepare_overlays(&sources, &terrain, 1, &AtomicBool::new(false), false);
        queue_result(&mut registry, complete);
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(registry.usage().omitted_optional, 0);
        assert!(
            optional_entities
                .iter()
                .all(|entity| world.get::<Visibility>(*entity) == Some(&Visibility::Inherited))
        );
        let next = prepare_overlays_with_budget(
            &sources,
            &terrain,
            1,
            &AtomicBool::new(false),
            false,
            size,
            0,
        );
        queue_result(&mut registry, next);
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(registry.usage().omitted_optional, 2);
        let recovery = prepare_overlays(&sources, &terrain, 1, &AtomicBool::new(false), false);
        let cancelled = queue_result(&mut registry, recovery);
        let removed = registry.clear_optional();
        assert_eq!(removed.len(), 2);
        assert!(cancelled.load(Ordering::Relaxed));
        upload_ready(&mut registry, &mut world, &mut meshes);
        registry.commit(&mut world.commands(), &mut meshes);
        world.flush();
        assert_eq!(registry.usage().omitted_optional, 0);
        assert_eq!(registry.usage().registered, 1);
        assert_eq!(
            world.get::<Visibility>(required_entity),
            Some(&Visibility::Inherited)
        );
    }

    #[test]
    fn coarse_transform_precision_is_rechecked_after_frame_changes() {
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut registry = TerrainOverlays::default();
        let item = registration(&mut world, &mut meshes);
        let entity = item.entity;
        let origin = item.source.origin;
        let id = TileId::containing(13, origin.to_geodetic());
        registry.register(entity, item.mesh, item.source).unwrap();
        registry.displayed.insert(entity);
        registry.committed_precision.push(PrecisionSource {
            footprint: [id, id],
            origin,
            extent: 2_000_000.0,
        });
        let nearby = RenderFrame::new(origin.to_geodetic());
        registry.apply_precision(&mut world.commands(), &nearby);
        world.flush();
        assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
        registry.committed_precision[0].extent = 1.0;
        registry.apply_precision(&mut world.commands(), &nearby);
        world.flush();
        assert_eq!(
            world.get::<Visibility>(entity),
            Some(&Visibility::Inherited)
        );
        let far = RenderFrame::new(
            origin
                .to_geodetic()
                .offset_by(Meters(50_000.0), Meters::ZERO),
        );
        registry.apply_precision(&mut world.commands(), &far);
        world.flush();
        assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
        registry.apply_precision(&mut world.commands(), &nearby);
        world.flush();
        assert_eq!(
            world.get::<Visibility>(entity),
            Some(&Visibility::Inherited)
        );
    }
    #[test]
    fn affine_rounding_bound_covers_real_global_facets_and_rebases() {
        use flightsim_core::{LocalFrame, Ned};
        use glam::{Affine3A, DQuat, DVec3, Vec3};
        let atlas = flightsim_world::global::GlobalTerrain::bundled().unwrap();
        let transform = |frame: &RenderFrame, origin| {
            Affine3A::from_scale_rotation_translation(
                Vec3::ONE,
                frame.rotation_to_render(DQuat::IDENTITY),
                frame.to_render(origin),
            )
        };
        let local = |frame: &LocalFrame, world| {
            let point = frame.ecef_to_ned_position(world);
            DVec3::new(point.north(), point.east(), point.up())
        };
        let cross = |a: DVec3, b: DVec3| a.x * b.y - a.y * b.x;
        let mut checked = 0;
        let mut accepted = 0;
        let mut coarse_rejected = 0;
        for (lat, lon) in [
            (35.55, 139.78),
            (47.139, 9.518),
            (-0.13, -78.36),
            (65.0, 179.99),
            (89.999, 65.0),
            (-89.999, -95.0),
        ] {
            let point = Geodetic::from_degrees(lat, lon, 0.0);
            let height = atlas.sample(point).unwrap().surface_height;
            let anchor = Geodetic::new(point.latitude, point.longitude, height);
            let local_frame = LocalFrame::new(anchor);
            let up = local_frame.up_ecef();
            let frames = [
                RenderFrame::new(anchor),
                RenderFrame::new(
                    local_frame
                        .ned_to_ecef_position(Ned::new(6000.0, -3000.0, 0.0))
                        .to_geodetic(),
                ),
            ];
            for level in [0, 4, 8, 11, 13] {
                for north in [-1500.0, 0.0, 1500.0] {
                    for east in [-1500.0, 0.0, 1500.0] {
                        let probe = local_frame.ned_to_ecef_position(Ned::new(north, east, 0.0));
                        let id = TileId::containing(level, probe.to_geodetic());
                        let dem = atlas.tile(id).unwrap();
                        let mesh = flightsim_world::build_mesh(
                            id,
                            &dem,
                            &flightsim_world::MeshOptions {
                                resolution: 33,
                                skirt_depth: Some(Meters::ZERO),
                            },
                        );
                        let projected = local(&local_frame, probe);
                        let mut hit = None;
                        for tri in mesh.indices.chunks_exact(3) {
                            let vertices = [tri[0], tri[1], tri[2]].map(|index| {
                                local(
                                    &local_frame,
                                    Ecef(
                                        mesh.origin.as_vec()
                                            + Vec3::from_array(mesh.positions[index as usize])
                                                .as_dvec3(),
                                    ),
                                )
                            });
                            let area = cross(vertices[1] - vertices[0], vertices[2] - vertices[0]);
                            if area >= -1e-10 {
                                continue;
                            }
                            let b =
                                cross(projected - vertices[0], vertices[2] - vertices[0]) / area;
                            let c =
                                cross(vertices[1] - vertices[0], projected - vertices[0]) / area;
                            if b < -1e-9 || c < -1e-9 || b + c > 1.0 + 1e-9 {
                                continue;
                            }
                            let on_facet =
                                vertices[0] * (1.0 - b - c) + vertices[1] * b + vertices[2] * c;
                            let face = (vertices[1] - vertices[0]).cross(vertices[2] - vertices[0]);
                            hit =
                                Some(([tri[0], tri[1], tri[2]], on_facet, -face.z / face.length()));
                            break;
                        }
                        let Some((tri, on_facet, cosine)) = hit else {
                            continue;
                        };
                        let world = local_frame.ned_to_ecef_position(Ned::new(
                            on_facet.x,
                            on_facet.y,
                            -on_facet.z,
                        ));
                        let source_extent = mesh
                            .positions
                            .iter()
                            .map(|p| Vec3::from_array(*p).as_dvec3().length())
                            .fold(0.0, f64::max);
                        for frame in &frames {
                            let terrain_transform = transform(frame, mesh.origin);
                            let overlay_transform = transform(frame, anchor.to_ecef());
                            let triangle = tri.map(|index| {
                                terrain_transform
                                    .transform_point3(Vec3::from_array(
                                        mesh.positions[index as usize],
                                    ))
                                    .as_dvec3()
                            });
                            let normal = (triangle[1] - triangle[0])
                                .cross(triangle[2] - triangle[0])
                                .normalize();
                            for lift in [0.0, 0.03, 0.055, 0.08] {
                                let relative =
                                    world.as_vec() + up * lift - anchor.to_ecef().as_vec();
                                let rendered = overlay_transform
                                    .transform_point3(relative.as_vec3())
                                    .as_dvec3();
                                let separation = (rendered - triangle[0]).dot(normal);
                                let bound = transform_error_bound(
                                    frame,
                                    mesh.origin,
                                    source_extent,
                                    anchor.to_ecef(),
                                    relative.length(),
                                );
                                if lift.abs() < f64::EPSILON {
                                    assert!(
                                        separation.abs() <= bound,
                                        "{lat},{lon} L{level}: error={} bound={bound}",
                                        separation.abs()
                                    );
                                } else if bound < lift * cosine * 0.5 {
                                    assert!(
                                        separation > 0.0,
                                        "accepted buried layer at {lat},{lon} L{level}"
                                    );
                                    accepted += 1;
                                } else if level == 0 {
                                    coarse_rejected += 1;
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            checked > 1500 && accepted > 500 && coarse_rejected > 100,
            "checked={checked} accepted={accepted} coarse={coarse_rejected}"
        );
    }
}
