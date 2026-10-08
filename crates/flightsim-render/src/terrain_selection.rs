//! Availability-aware render-only LOD selection.
//!
//! Read DEMs and prepare hidden meshes under separate per-frame budgets. Only
//! activate a non-overlapping cut once every mesh in a replacement is prepared.
//! None of this changes the ground sampler or writes to simulation state.

use flightsim_core::{Ecef, Meters};
use flightsim_world::lod::distance_to_bounds;
use flightsim_world::{DemTile, LodSelector, TileCache, TileId, TileSource};
use std::collections::{BTreeSet, HashMap};

const RETRY_FRAMES: u64 = 120;
/// A full four-way tree has fewer than twice as many nodes as leaves. The extra
/// space also covers the selector's truncated terminal path (depth <= 24).
const RESIDENT_TILE_LIMIT: usize = 2 * flightsim_world::lod::DEFAULT_MAX_TILES;

#[derive(Debug, Clone, Copy)]
enum LoadOutcome {
    Loaded,
    Missing,
    Failed,
}

#[derive(Debug, Clone, Copy)]
struct LoadAttempt {
    frame: u64,
    outcome: LoadOutcome,
}

/// Allocated only while readiness is observed. Earlier unavailable fallback
/// outcomes are unknown until an ordinary retry runs; enabling observation
/// never advances that retry. Entries follow the existing active-tree bound.
#[derive(Debug, Default)]
struct ReadinessObservation {
    ready: bool,
    fallback_outcomes: HashMap<TileId, LoadOutcome>,
}

/// Persistent render-only streaming state.
///
/// `ids()` reports visible tiles; `resident_len()` also counts prepared hidden
/// meshes. Both survive DEM cache eviction. There are at most 8,192 resident
/// meshes, including retained finer coverage during unavailable coarsening.
/// Old visible descendants outside the desired tree can be evicted in favor of
/// nearer work when this ceiling is reached; hidden stale work is discarded.
/// Load history is pruned to the current desired tree and its ancestors.
#[derive(Debug)]
pub struct TerrainSelectionState {
    live: BTreeSet<TileId>,
    resident: BTreeSet<TileId>,
    attempts: HashMap<TileId, LoadAttempt>,
    // Fallback DEMs must never stop the search for a real ancestor. Keep cache
    // and resident provenance separately because their lifetimes differ.
    fallback_cache: BTreeSet<TileId>,
    fallback_resident: BTreeSet<TileId>,
    fallback_attempts: HashMap<TileId, u64>,
    frame: u64,
    // Retain arbitration across updates, including a one-read frame budget.
    last_load_was_fallback: bool,
    resident_limit: usize,
    lod_truncated: bool,
    desired_len: usize,
    matches_desired: bool,
    // Optional read-only work observation, used by one-shot scene capture.
    // Ordinary streaming does not perform the extra dependency traversal.
    readiness_observation: Option<Box<ReadinessObservation>>,
}

impl Default for TerrainSelectionState {
    fn default() -> Self {
        Self {
            live: BTreeSet::new(),
            resident: BTreeSet::new(),
            attempts: HashMap::new(),
            fallback_cache: BTreeSet::new(),
            fallback_resident: BTreeSet::new(),
            fallback_attempts: HashMap::new(),
            frame: 0,
            last_load_was_fallback: false,
            resident_limit: RESIDENT_TILE_LIMIT,
            lod_truncated: false,
            desired_len: 0,
            matches_desired: true,
            readiness_observation: None,
        }
    }
}

impl TerrainSelectionState {
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    #[must_use]
    pub fn contains(&self, id: TileId) -> bool {
        self.live.contains(&id)
    }

    #[must_use]
    pub fn resident_len(&self) -> usize {
        self.resident.len()
    }

    /// Visible IDs, in deterministic tile order.
    pub fn ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.live.iter().copied()
    }

    /// The selector retained coarser complete leaves because its detail budget
    /// was exhausted. Coverage is retained, but requested SSE is not satisfied.
    #[must_use]
    pub const fn selection_truncated(&self) -> bool {
        self.lod_truncated
    }

    /// Number of raw leaves requested by the latest selector update, before
    /// availability fallback or retaining already visible finer coverage.
    #[must_use]
    pub const fn desired_len(&self) -> usize {
        self.desired_len
    }

    /// Whether visible IDs exactly equal the latest requested leaf IDs. Equal
    /// counts alone do not imply convergence. This describes the render selector
    /// cut, not a renderer's possibly still-pending atomic bridge transaction.
    #[must_use]
    pub const fn matches_desired(&self) -> bool {
        self.matches_desired
    }

    /// Enable an additional read-only dependency observation after selection.
    /// No source reads, scheduling priorities or budgets change. Disabling
    /// removes the observation and its extra bounded traversal entirely.
    /// Outcomes of earlier unavailable fallback reads are unknown until a
    /// normal retry is observed; cached/resident data can be verified directly.
    pub fn observe_readiness(&mut self, enabled: bool) {
        if !enabled {
            self.readiness_observation = None;
        } else if self.readiness_observation.is_none() {
            self.readiness_observation = Some(Box::default());
        }
    }

    /// Whether the last observed selection examined all current dependency
    /// paths and has no cached mesh preparation left. Known missing/failed
    /// reads can still retry later. This permits valid coarse/empty availability
    /// cuts without treating an arbitrary quiet frame as convergence. Renderer
    /// bridge/overlay commits and displayed IDs must be checked separately.
    #[must_use]
    pub fn observed_readiness(&self) -> Option<bool> {
        self.readiness_observation
            .as_ref()
            .map(|observation| observation.ready)
    }

    fn dependencies_examined(
        &self,
        wanted: &BTreeSet<TileId>,
        cache: &TileCache,
        primary_reads_possible: bool,
        has_fallback: bool,
    ) -> bool {
        for &leaf in wanted {
            let mut ancestor = Some(leaf);
            let mut primary_available = false;
            while let Some(id) = ancestor {
                if self.available(id, cache) {
                    primary_available = true;
                    break;
                }
                if primary_reads_possible
                    && !self.attempts.get(&id).is_some_and(|attempt| {
                        matches!(attempt.outcome, LoadOutcome::Missing | LoadOutcome::Failed)
                    })
                {
                    return false;
                }
                ancestor = id.parent();
            }
            // Primary ancestors deliberately outrank fine global fallback.
            // Otherwise every path down to an available fallback ancestor must
            // have been tried, regardless of distance-based retry arbitration.
            if has_fallback && !primary_available {
                let mut ancestor = Some(leaf);
                while let Some(id) = ancestor {
                    if self.resident.contains(&id) || cache.contains(id) {
                        break;
                    }
                    if !self
                        .readiness_observation
                        .as_ref()
                        .and_then(|observation| observation.fallback_outcomes.get(&id))
                        .is_some_and(|outcome| {
                            matches!(outcome, LoadOutcome::Missing | LoadOutcome::Failed)
                        })
                    {
                        return false;
                    }
                    ancestor = id.parent();
                }
            }
        }
        true
    }

    fn available(&self, id: TileId, cache: &TileCache) -> bool {
        self.resident.contains(&id) && !self.fallback_resident.contains(&id)
            || cache.contains(id) && !self.fallback_cache.contains(&id)
    }

    /// Count of visible tiles drawn from the explicitly identified global fallback.
    #[must_use]
    pub fn fallback_len(&self) -> usize {
        self.live.intersection(&self.fallback_resident).count()
    }

    fn next_fallback(
        &self,
        primary_reads_possible: bool,
        wanted: &BTreeSet<TileId>,
        cache: &TileCache,
        camera: Ecef,
    ) -> Option<TileId> {
        let mut candidates = BTreeSet::new();
        for &leaf in wanted {
            // A fine generated tile never outranks any available primary
            // ancestor. Unless primary data are explicitly impossible, unknown
            // ancestors must first be searched under budget.
            let mut ancestor = Some(leaf);
            let mut all_missing = true;
            while let Some(id) = ancestor {
                if self.available(id, cache)
                    || primary_reads_possible
                        && !self.attempts.get(&id).is_some_and(|attempt| {
                            matches!(attempt.outcome, LoadOutcome::Missing | LoadOutcome::Failed)
                        })
                {
                    all_missing = false;
                    break;
                }
                ancestor = id.parent();
            }
            if !all_missing {
                continue;
            }
            // The fallback may have a lower maximum level than the requested
            // primary DEM. Try its ancestors too, rather than leaving holes at
            // an otherwise valid --max-level above the atlas tessellation cap.
            let mut ancestor = Some(leaf);
            while let Some(id) = ancestor {
                if self.resident.contains(&id) || cache.contains(id) {
                    break;
                }
                if self
                    .fallback_attempts
                    .get(&id)
                    .is_none_or(|frame| self.frame.wrapping_sub(*frame) >= RETRY_FRAMES)
                {
                    candidates.insert(id);
                    break;
                }
                ancestor = id.parent();
            }
        }
        candidates
            .into_iter()
            .min_by_key(|id| tile_priority(*id, camera))
    }

    /// Fresh dependencies precede retries, then oldest attempt, distance and ID.
    /// Repeated near misses cannot starve ancestors or other regions. Evicted
    /// successful reads also receive a fair turn, rather than restarting first.
    fn next_request(
        &self,
        wanted: &BTreeSet<TileId>,
        cache: &TileCache,
        camera: Ecef,
    ) -> Option<TileId> {
        let mut candidates = BTreeSet::new();
        for &leaf in wanted {
            let mut ancestor = Some(leaf);
            while let Some(id) = ancestor {
                if self.available(id, cache) {
                    break;
                }
                let Some(attempt) = self.attempts.get(&id) else {
                    candidates.insert(id);
                    break;
                };
                match attempt.outcome {
                    LoadOutcome::Loaded => {
                        if attempt.frame != self.frame {
                            candidates.insert(id);
                        }
                        break;
                    }
                    LoadOutcome::Missing | LoadOutcome::Failed => {
                        if self.frame - attempt.frame >= RETRY_FRAMES {
                            candidates.insert(id);
                        }
                        ancestor = id.parent();
                    }
                }
            }
        }
        candidates.into_iter().min_by(|a, b| {
            self.attempts
                .get(a)
                .map_or(0, |attempt| attempt.frame)
                .cmp(&self.attempts.get(b).map_or(0, |attempt| attempt.frame))
                .then_with(|| tile_priority(*a, camera).cmp(&tile_priority(*b, camera)))
        })
    }

    fn next_preparation(
        &self,
        wanted: &BTreeSet<TileId>,
        cache: &TileCache,
        camera: Ecef,
    ) -> Option<TileId> {
        let mut candidates = BTreeSet::new();
        for &leaf in wanted {
            let mut ancestor = Some(leaf);
            let mut fallback = None;
            let mut real_available = false;
            while let Some(id) = ancestor {
                if self.resident.contains(&id) && !self.fallback_resident.contains(&id) {
                    real_available = true;
                    break;
                }
                if cache.contains(id) && !self.fallback_cache.contains(&id) {
                    candidates.insert(id);
                    real_available = true;
                    break;
                }
                if fallback.is_none() && (self.resident.contains(&id) || cache.contains(id)) {
                    fallback = Some(id);
                }
                ancestor = id.parent();
            }
            if !real_available
                && let Some(id) = fallback
                && !self.resident.contains(&id)
                && cache.contains(id)
            {
                candidates.insert(id);
            }
        }
        candidates
            .into_iter()
            .min_by_key(|id| tile_priority(*id, camera))
    }

    fn remove(&mut self, id: TileId, update: &mut TerrainUpdate) {
        self.live.remove(&id);
        self.resident.remove(&id);
        self.fallback_resident.remove(&id);
        update.despawned.push(id);
    }

    /// Only previously visible finer tiles outside the current desired tree are
    /// capacity victims. Active parents and prepared replacements are protected.
    /// Active-tree <= capacity prevents exhaustion by protected active nodes.
    /// Farther work can still be deferred intentionally in favor of nearer
    /// retained coverage; moving the camera reprioritizes that choice.
    fn reserve(
        &mut self,
        id: TileId,
        active: &BTreeSet<TileId>,
        camera: Ecef,
        update: &mut TerrainUpdate,
    ) -> bool {
        if self.resident.len() < self.resident_limit {
            return true;
        }
        update.capacity_limited = true;
        let victim = self
            .resident
            .iter()
            .copied()
            .filter(|id| !active.contains(id))
            .max_by_key(|id| tile_priority(*id, camera));
        if let Some(victim) = victim
            && tile_priority(id, camera) < tile_priority(victim, camera)
        {
            self.remove(victim, update);
            return true;
        }
        false
    }
}

// distance_to_bounds returns a nonnegative distance; IEEE bits preserve that
// order. total_cmp semantics for exceptional values are unnecessary for valid
// camera coordinates (the app validates its render observation point).
fn tile_priority(id: TileId, camera: Ecef) -> (u64, TileId) {
    let distance = distance_to_bounds(camera, camera.to_geodetic(), id.bounds()).get();
    (distance.to_bits(), id)
}

fn has_ancestor(id: TileId, set: &BTreeSet<TileId>) -> bool {
    let mut current = id.parent();
    while let Some(ancestor) = current {
        if set.contains(&ancestor) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// Exact powers-of-four coverage in the geographic quadtree. Complete root
/// seeds are useful during cold global startup, but a partial regional request
/// must never unexpectedly draw an entire hemisphere.
fn covers_region(region: TileId, leaves: &BTreeSet<TileId>) -> bool {
    let mut fraction = 0.0;
    for &id in leaves {
        if id.level < region.level {
            let delta = region.level - id.level;
            if region.x >> delta == id.x && region.y >> delta == id.y {
                return true;
            }
        } else {
            let delta = id.level - region.level;
            if id.x >> delta == region.x && id.y >> delta == region.y {
                fraction += 0.25_f64.powi(i32::from(delta));
            }
        }
    }
    fraction >= 1.0
}

fn insert_ancestors(id: TileId, set: &mut BTreeSet<TileId>) {
    let mut ancestor = Some(id);
    while let Some(id) = ancestor {
        if !set.insert(id) {
            break;
        }
        ancestor = id.parent();
    }
}

/// One atomic visibility update, plus bounded hidden-mesh preparation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerrainUpdate {
    /// New hidden meshes delivered to `mesh_sink`, at most the frame budget.
    pub prepared: Vec<TileId>,
    /// Resident tiles becoming visible; no mesh generation is needed here.
    pub spawned: Vec<TileId>,
    /// Visible tiles kept resident but hidden beneath a fallback ancestor.
    pub hidden: Vec<TileId>,
    /// Resident tiles to destroy, whether previously visible or hidden.
    pub despawned: Vec<TileId>,
    pub loaded: usize,
    /// Successful, bounded global-fallback tile generations (included in loaded).
    pub fallback_loaded: usize,
    /// Source changes at an existing ID. The sink replaces its mesh atomically.
    pub replaced: Vec<TileId>,
    /// Includes missing and failed reads; at most the frame budget.
    pub load_attempts: usize,
    pub missing: usize,
    pub failed: usize,
    /// The resident ceiling evicted retained finer coverage or deferred work.
    pub capacity_limited: bool,
}

/// Provenance of the exact cached DEM handed to mesh preparation.
/// Never infer this from a missing directory or from a tile's resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainMeshProvenance {
    Primary,
    Fallback,
}

/// Prepare and select render tiles without changing terrain sampling.
///
/// `frame_budget` independently caps source reads (including failures/misses) and
/// mesh preparations (including cache hits). The sink must create **hidden**,
/// correctly positioned meshes. Apply visibility and removal changes together
/// after preparation, before rendering. Existing parents remain visible until
/// every mesh of a replacing cut is ready, even with a one-tile DEM cache.
///
/// Missing/error reads are retried after 120 updates. Missing desired leaves can
/// use ancestors; existing visible descendants survive unavailable coarsening.
/// Indexed primary coverage can request finer regional roots before streaming;
/// sources without that optional hint retain the existing ancestor-only search.
///
/// # Panics
///
/// A custom selector whose desired ancestor closure exceeds the 8,192 resident
/// ceiling is an invalid configuration. The default 4,096-leaf selector fits.
pub fn update_terrain_selection<S: TileSource>(
    selector: &LodSelector,
    source: &S,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile),
) -> TerrainUpdate {
    let selection = selector.select_with_coverage(camera, Meters::ZERO, source.primary_coverage());
    state.lod_truncated = selection.truncated;
    update_selected_tiles(
        selection.tiles.into_iter().collect(),
        source,
        cache,
        state,
        camera,
        frame_budget,
        mesh_sink,
    )
}

/// Availability-aware selection using the camera's local ground height. Read
/// and mesh-generation budgets have exactly the same contract as the legacy
/// zero-height wrapper. This prevents coarse terrain at elevated airports.
#[allow(
    clippy::too_many_arguments,
    reason = "the existing streaming context plus a unit-typed ground reference"
)]
pub fn update_terrain_selection_with_surface<S: TileSource>(
    selector: &LodSelector,
    source: &S,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    ground_elevation: Meters,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile),
) -> TerrainUpdate {
    let selection =
        selector.select_with_coverage(camera, ground_elevation, source.primary_coverage());
    state.lod_truncated = selection.truncated;
    update_selected_tiles(
        selection.tiles.into_iter().collect(),
        source,
        cache,
        state,
        camera,
        frame_budget,
        mesh_sink,
    )
}

/// Preparation callback preserving the successful read's provenance.
/// The existing wrappers intentionally retain their two-argument callback API.
#[allow(
    clippy::too_many_arguments,
    reason = "streaming context and explicit ground reference"
)]
pub fn update_terrain_selection_with_surface_and_provenance<S: TileSource>(
    selector: &LodSelector,
    source: &S,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    ground_elevation: Meters,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile, TerrainMeshProvenance),
) -> TerrainUpdate {
    let selection =
        selector.select_with_coverage(camera, ground_elevation, source.primary_coverage());
    state.lod_truncated = selection.truncated;
    update_selected_tiles_with_provenance(
        selection.tiles.into_iter().collect(),
        source,
        cache,
        state,
        camera,
        frame_budget,
        mesh_sink,
    )
}

fn update_selected_tiles<S: TileSource>(
    wanted: BTreeSet<TileId>,
    source: &S,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile),
) -> TerrainUpdate {
    update_selected_tiles_with_provenance(
        wanted,
        source,
        cache,
        state,
        camera,
        frame_budget,
        &mut |id, dem, _provenance| mesh_sink(id, dem),
    )
}

fn update_selected_tiles_with_provenance<S: TileSource>(
    wanted: BTreeSet<TileId>,
    source: &S,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile, TerrainMeshProvenance),
) -> TerrainUpdate {
    let mut active = BTreeSet::new();
    for &leaf in &wanted {
        insert_ancestors(leaf, &mut active);
    }
    assert!(
        active.len() <= state.resident_limit,
        "the desired terrain tree exceeds the resident mesh limit"
    );
    state.attempts.retain(|id, _| active.contains(id));
    state.fallback_attempts.retain(|id, _| active.contains(id));
    if let Some(observation) = state.readiness_observation.as_mut() {
        observation
            .fallback_outcomes
            .retain(|id, _| active.contains(id));
    }
    state.fallback_cache.retain(|id| cache.contains(*id));
    state.frame = state.frame.wrapping_add(1);
    if state.frame == 0 {
        state.attempts.clear();
        state.fallback_attempts.clear();
        if let Some(observation) = state.readiness_observation.as_mut() {
            observation.fallback_outcomes.clear();
        }
    }
    let previous_live = state.live.clone();
    // At most two coarse seeds close the whole globe quickly while finer
    // primary discovery continues. Seeds are provisional, never primary data.
    let seed_roots: Vec<_> = if source.has_fallback() {
        TileId::roots()
            .into_iter()
            .filter(|id| covers_region(*id, &wanted) && !covers_region(*id, &state.live))
            .collect()
    } else {
        Vec::new()
    };
    let mut update = TerrainUpdate::default();

    // Retain finer geometry only if it was actually visible. Never accumulate
    // canceled hidden work while travelling between regions.
    let stale: Vec<_> = state
        .resident
        .iter()
        .copied()
        .filter(|id| {
            !(active.contains(id) || state.live.contains(id) && has_ancestor(*id, &wanted))
        })
        .collect();
    for id in stale {
        state.remove(id, &mut update);
    }

    prepare_cached(
        &wanted,
        &active,
        cache,
        state,
        camera,
        frame_budget,
        mesh_sink,
        &mut update,
    );
    let primary_reads_possible = source.primary_reads_possible();
    while update.load_attempts < frame_budget {
        let primary = primary_reads_possible
            .then(|| {
                seed_roots
                    .iter()
                    .copied()
                    .filter(|id| !state.available(*id, cache) && !state.attempts.contains_key(id))
                    .min_by_key(|id| tile_priority(*id, camera))
                    .or_else(|| state.next_request(&wanted, cache, camera))
            })
            .flatten();
        let fallback = if source.has_fallback() {
            seed_roots
                .iter()
                .copied()
                .filter(|id| {
                    !state.resident.contains(id)
                        && !cache.contains(*id)
                        && (!primary_reads_possible
                            || state.attempts.get(id).is_some_and(|attempt| {
                                matches!(
                                    attempt.outcome,
                                    LoadOutcome::Missing | LoadOutcome::Failed
                                )
                            }))
                        && state
                            .fallback_attempts
                            .get(id)
                            .is_none_or(|frame| state.frame.wrapping_sub(*frame) >= RETRY_FRAMES)
                })
                .min_by_key(|id| tile_priority(*id, camera))
                .or_else(|| state.next_fallback(primary_reads_possible, &wanted, cache, camera))
        } else {
            None
        };
        // Both lanes have already established eligibility and their own priority
        // order. Comparing distances here lets near primary retries repeatedly
        // delay a farther ready fallback, or vice versa. Alternate read attempts
        // across frames so ready terrain and newly arriving real DEMs both make
        // progress, even with a one-read budget. Cold globe seeds still go first.
        let use_fallback = fallback.is_some_and(|id| seed_roots.contains(&id))
            || match (primary, fallback) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(_), Some(_)) => !state.last_load_was_fallback,
            };
        let Some(id) = (if use_fallback { fallback } else { primary }) else {
            break;
        };
        update.load_attempts += 1;
        state.last_load_was_fallback = use_fallback;
        let loaded = if use_fallback {
            source.load_fallback(id)
        } else {
            source.load(id)
        };
        let outcome = match loaded {
            Ok(Some(tile)) => {
                cache.insert(id, tile);
                if use_fallback {
                    state.fallback_cache.insert(id);
                    update.fallback_loaded += 1;
                } else {
                    state.fallback_cache.remove(&id);
                }
                update.loaded += 1;
                LoadOutcome::Loaded
            }
            Ok(None) => {
                update.missing += 1;
                LoadOutcome::Missing
            }
            Err(_) => {
                update.failed += 1;
                LoadOutcome::Failed
            }
        };
        if use_fallback {
            state.fallback_attempts.insert(id, state.frame);
            if let Some(observation) = state.readiness_observation.as_mut() {
                observation.fallback_outcomes.insert(id, outcome);
            }
        } else {
            state.attempts.insert(
                id,
                LoadAttempt {
                    frame: state.frame,
                    outcome,
                },
            );
        }
        // Prepare before a later load can evict this DEM from a small cache.
        prepare_cached(
            &wanted,
            &active,
            cache,
            state,
            camera,
            frame_budget,
            mesh_sink,
            &mut update,
        );
    }

    let mut tree = active.clone();
    for &id in &state.live {
        insert_ancestors(id, &mut tree);
    }
    let mut cut = Vec::new();
    for root in TileId::roots() {
        resolve_cut(root, false, false, &wanted, &tree, state, &mut cut);
    }
    let keep: BTreeSet<_> = cut.into_iter().collect();
    update.spawned.extend(keep.difference(&previous_live));
    // A replacement keeps its ID, but the newly prepared entity is hidden until
    // this cut commits. Explicitly reveal it even when its predecessor was live.
    update
        .spawned
        .extend(update.replaced.iter().filter(|id| keep.contains(id)));
    update.spawned.sort_unstable();
    update.spawned.dedup();

    let unused: Vec<_> = state.resident.difference(&keep).copied().collect();
    for id in unused {
        if active.contains(&id) && has_ancestor(id, &keep) {
            if previous_live.contains(&id) {
                update.hidden.push(id);
            }
        } else {
            state.remove(id, &mut update);
        }
    }
    state.desired_len = wanted.len();
    state.matches_desired = keep == wanted;
    state.live = keep;
    let observed_ready = state.readiness_observation.as_ref().map(|_| {
        frame_budget > 0
            && !update.capacity_limited
            && state.dependencies_examined(
                &wanted,
                cache,
                primary_reads_possible,
                source.has_fallback(),
            )
            && state.next_preparation(&wanted, cache, camera).is_none()
    });
    if let Some((observation, ready)) = state.readiness_observation.as_mut().zip(observed_ready) {
        observation.ready = ready;
    }
    update.despawned.sort_unstable();
    update.despawned.dedup();
    update
}

#[allow(
    clippy::too_many_arguments,
    reason = "the private preparation phase shares one frame's streaming context"
)]
fn prepare_cached(
    wanted: &BTreeSet<TileId>,
    active: &BTreeSet<TileId>,
    cache: &mut TileCache,
    state: &mut TerrainSelectionState,
    camera: Ecef,
    frame_budget: usize,
    mesh_sink: &mut dyn FnMut(TileId, &DemTile, TerrainMeshProvenance),
    update: &mut TerrainUpdate,
) {
    while update.prepared.len() < frame_budget {
        let Some(id) = state.next_preparation(wanted, cache, camera) else {
            break;
        };
        let replacing = state.resident.contains(&id);
        if !replacing && !state.reserve(id, active, camera, update) {
            break;
        }
        let tile = cache.get(id).expect("prepared tile has a cached DEM");
        let provenance = if state.fallback_cache.contains(&id) {
            TerrainMeshProvenance::Fallback
        } else {
            TerrainMeshProvenance::Primary
        };
        mesh_sink(id, tile, provenance);
        state.resident.insert(id);
        if state.fallback_cache.contains(&id) {
            state.fallback_resident.insert(id);
        } else {
            state.fallback_resident.remove(&id);
        }
        if replacing {
            update.replaced.push(id);
        }
        update.prepared.push(id);
    }
}

/// Roll back every descendant if a ready ancestor must cover an incomplete cut.
/// Below an unavailable desired coarse tile, consider only already visible finer
/// meshes carried into `tree`; no new descendant data is searched or requested.
fn resolve_cut(
    id: TileId,
    below_wanted: bool,
    real_ancestor: bool,
    wanted: &BTreeSet<TileId>,
    tree: &BTreeSet<TileId>,
    state: &TerrainSelectionState,
    cut: &mut Vec<TileId>,
) -> bool {
    if !tree.contains(&id) {
        return false;
    }
    let below_wanted = below_wanted || wanted.contains(&id);
    let resident =
        state.resident.contains(&id) && (!real_ancestor || !state.fallback_resident.contains(&id));
    let real_here = resident && !state.fallback_resident.contains(&id);
    if below_wanted && resident {
        cut.push(id);
        return true;
    }
    let start = cut.len();
    let mut covered = true;
    if let Some(children) = id.children() {
        for child in children {
            covered &= resolve_cut(
                child,
                below_wanted,
                real_ancestor || real_here,
                wanted,
                tree,
                state,
                cut,
            );
        }
    } else {
        covered = false;
    }
    if covered {
        return true;
    }
    if resident {
        cut.truncate(start);
        cut.push(id);
        return true;
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Geodetic, Meters, Radians};
    use flightsim_world::{HeightGrid, MemoryTileSource, TerrainError};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Source {
        tiles: MemoryTileSource,
        errors: BTreeSet<TileId>,
        malformed: BTreeSet<TileId>,
        calls: RefCell<Vec<TileId>>,
        fallback_calls: RefCell<Vec<TileId>>,
        fallback: bool,
        no_primary: bool,
        fallback_max_level: Option<u8>,
        fallback_errors: BTreeSet<TileId>,
        coverage: Option<flightsim_world::PrimaryCoverage>,
    }

    impl Source {
        fn add(&mut self, id: TileId) {
            self.tiles.insert(id, dem(id));
        }
    }

    impl TileSource for Source {
        fn primary_coverage(&self) -> Option<&flightsim_world::PrimaryCoverage> {
            self.coverage.as_ref()
        }
        fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
            assert!(
                !self.no_primary,
                "explicitly absent primary must not be probed"
            );
            self.calls.borrow_mut().push(id);
            if self.malformed.contains(&id) {
                Err(TerrainError::Malformed {
                    path: "synthetic.fsdem".into(),
                    source: flightsim_world::dem::io::read_tile(&mut &b"broken"[..]).unwrap_err(),
                })
            } else if self.errors.contains(&id) {
                Err(TerrainError::Io {
                    path: "synthetic.fsdem".into(),
                    source: std::io::Error::other("synthetic read failure"),
                })
            } else {
                self.tiles.load(id)
            }
        }

        fn primary_reads_possible(&self) -> bool {
            !self.no_primary
        }

        fn has_fallback(&self) -> bool {
            self.fallback
        }

        fn load_fallback(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
            self.fallback_calls.borrow_mut().push(id);
            if self.fallback_errors.contains(&id) {
                return Err(TerrainError::Io {
                    path: "synthetic-fallback".into(),
                    source: std::io::Error::other("synthetic fallback failure"),
                });
            }
            Ok((self.fallback
                && self
                    .fallback_max_level
                    .is_none_or(|level| id.level <= level))
            .then(|| DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(5.0)))))
        }
    }

    fn dem(id: TileId) -> DemTile {
        DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(42.0)))
    }

    fn parent() -> TileId {
        TileId::new(6, 75, 20)
    }

    fn camera() -> Ecef {
        let center = parent().center();
        Geodetic::new(center.latitude, center.longitude, Meters(100.0)).to_ecef()
    }

    fn contains(ancestor: TileId, descendant: TileId) -> bool {
        descendant.level >= ancestor.level
            && (descendant.x >> (descendant.level - ancestor.level)) == ancestor.x
            && (descendant.y >> (descendant.level - ancestor.level)) == ancestor.y
    }

    fn sorted<const N: usize>(mut ids: [TileId; N]) -> [TileId; N] {
        ids.sort_unstable();
        ids
    }

    fn check_no_overlap(state: &TerrainSelectionState) {
        // The geographic quadtree can overlap only along ancestor paths.
        // Check those exact paths rather than every pair so global regressions
        // can verify every intermediate cut without quadratic test overhead.
        for id in state.ids() {
            let mut ancestor = id.parent();
            while let Some(parent) = ancestor {
                assert!(!state.contains(parent), "overlap: {id:?}, {parent:?}");
                ancestor = parent.parent();
            }
        }
    }

    fn covered_at(state: &TerrainSelectionState, position: Geodetic) -> usize {
        state
            .ids()
            .filter(|id| *id == TileId::containing(id.level, position))
            .count()
    }

    fn check_covered(state: &TerrainSelectionState, tile: TileId) {
        for y in 0..=8 {
            for x in 0..=8 {
                let bounds = tile.bounds();
                // Include both sides of every interior child edge and approach
                // the exterior edges without assigning them to the adjacent tile.
                let u = (f64::from(x) / 8.0).clamp(1.0e-9, 1.0 - 1.0e-9);
                let v = (f64::from(y) / 8.0).clamp(1.0e-9, 1.0 - 1.0e-9);
                let point = Geodetic::new(
                    Radians(bounds.north.get() - bounds.height().get() * v),
                    Radians(bounds.west.get() + bounds.width().get() * u),
                    Meters::ZERO,
                );
                assert_eq!(covered_at(state, point), 1, "hole at {point:?}");
            }
        }
    }

    fn step(
        wanted: &[TileId],
        source: &Source,
        cache: &mut TileCache,
        state: &mut TerrainSelectionState,
        budget: usize,
    ) -> TerrainUpdate {
        step_at(wanted, source, cache, state, budget, camera())
    }

    fn step_at(
        wanted: &[TileId],
        source: &Source,
        cache: &mut TileCache,
        state: &mut TerrainSelectionState,
        budget: usize,
        camera: Ecef,
    ) -> TerrainUpdate {
        let before = source.calls.borrow().len() + source.fallback_calls.borrow().len();
        let mut meshes = Vec::new();
        let update = update_selected_tiles(
            wanted.iter().copied().collect(),
            source,
            cache,
            state,
            camera,
            budget,
            &mut |id, dem| {
                assert_eq!(dem.bounds(), id.bounds());
                meshes.push(id);
            },
        );
        assert_eq!(
            update.load_attempts,
            source.calls.borrow().len() + source.fallback_calls.borrow().len() - before
        );
        assert!(update.load_attempts <= budget);
        assert_eq!(
            update.loaded + update.missing + update.failed,
            update.load_attempts
        );
        assert_eq!(update.prepared, meshes);
        assert!(meshes.len() <= budget);
        assert!(state.resident_len() <= state.resident_limit);
        assert!(state.live.is_subset(&state.resident));
        check_no_overlap(state);
        assert_eq!(state.desired_len(), wanted.len());
        assert_eq!(
            state.matches_desired(),
            state.live == wanted.iter().copied().collect()
        );
        update
    }

    #[test]
    fn missing_children_render_the_nearest_available_parent() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for _ in 0..8 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        check_covered(&state, parent);
        assert_eq!(
            source
                .calls
                .borrow()
                .iter()
                .filter(|id| **id == parent)
                .count(),
            1
        );
    }

    #[test]
    fn partial_child_availability_does_not_overlap_the_parent() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        source.add(children[0]);
        source.add(children[3]);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for _ in 0..8 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        check_covered(&state, parent);
    }

    #[test]
    fn an_existing_parent_stays_until_every_child_is_ready() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        for child in children {
            source.add(child);
        }
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        step(&[parent], &source, &mut cache, &mut state, 1);
        for _ in 0..3 {
            let update = step(&children, &source, &mut cache, &mut state, 1);
            assert!(update.spawned.is_empty());
            assert!(update.despawned.is_empty());
            assert!(state.contains(parent));
            check_covered(&state, parent);
        }
        let update = step(&children, &source, &mut cache, &mut state, 1);
        assert_eq!(update.despawned, vec![parent]);
        assert_eq!(update.spawned, sorted(children));
        assert_eq!(state.len(), 4);
        check_covered(&state, parent);
    }

    #[test]
    fn missing_children_are_retried_and_refine_when_data_arrives() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        step(&children, &source, &mut cache, &mut state, 8);
        assert!(state.contains(parent));
        for child in children {
            source.add(child);
        }
        for _ in 0..RETRY_FRAMES + 4 {
            step(&children, &source, &mut cache, &mut state, 1);
            check_covered(&state, parent);
        }
        assert!(!state.contains(parent));
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
    }

    #[test]
    fn failed_reads_are_budgeted_separately_and_recover() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        for child in children {
            source.add(child);
            source.errors.insert(child);
        }
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        let update = step(&children, &source, &mut cache, &mut state, 8);
        assert_eq!(update.failed, 4);
        assert_eq!(update.missing, 0);
        assert!(state.contains(parent));
        source.errors.clear();
        for _ in 0..RETRY_FRAMES + 4 {
            step(&children, &source, &mut cache, &mut state, 1);
            check_covered(&state, parent);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
    }

    #[test]
    fn corrupt_source_data_falls_back_without_becoming_a_missing_tile() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        source.malformed.extend(children);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        let update = step(&children, &source, &mut cache, &mut state, 8);
        assert_eq!(update.failed, 4);
        assert_eq!(update.missing, 0);
        assert_eq!(update.loaded, 1);
        assert_eq!(update.load_attempts, 5);
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        check_covered(&state, parent);
    }

    #[test]
    fn all_child_availability_patterns_have_an_overlap_free_cut() {
        let parent = parent();
        let children = parent.children().unwrap();
        for pattern in 0_u32..32 {
            let mut source = Source::default();
            if pattern & 16 != 0 {
                source.add(parent);
            }
            for (index, child) in children.into_iter().enumerate() {
                if pattern & (1 << index) != 0 {
                    source.add(child);
                }
            }
            let mut cache = TileCache::new(1_000_000);
            let mut state = TerrainSelectionState::default();
            for _ in 0..16 {
                step(&children, &source, &mut cache, &mut state, 1);
            }
            if pattern & 16 != 0 && pattern & 15 != 15 {
                assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
                check_covered(&state, parent);
            } else {
                let expected: BTreeSet<_> = children
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, id)| (pattern & (1 << index) != 0).then_some(id))
                    .collect();
                assert_eq!(state.live, expected);
            }
        }
    }

    #[test]
    fn ancestor_fallback_removes_all_descendants_at_mixed_depths() {
        let parent = parent();
        let children = parent.children().unwrap();
        let grandchildren = children[0].children().unwrap();
        let wanted: Vec<_> = grandchildren
            .into_iter()
            .chain(children[1..].iter().copied())
            .collect();
        let mut source = Source::default();
        source.add(parent);
        source.add(grandchildren[0]);
        source.add(grandchildren[1]);
        source.add(children[2]);
        source.add(children[3]);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for _ in 0..16 {
            step(&wanted, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        check_covered(&state, parent);
        for child in children[1..].iter().copied() {
            source.add(child);
        }
        for grandchild in grandchildren {
            source.add(grandchild);
        }
        for _ in 0..RETRY_FRAMES + 16 {
            step(&wanted, &source, &mut cache, &mut state, 2);
            check_covered(&state, parent);
        }
        assert_eq!(
            state.ids().collect::<BTreeSet<_>>(),
            wanted.into_iter().collect()
        );
    }

    #[test]
    fn cache_eviction_does_not_drop_a_parent_or_prepared_children() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        for child in children {
            source.add(child);
        }
        let mut cache = TileCache::new(dem(parent).memory_footprint());
        let mut state = TerrainSelectionState::default();
        step(&[parent], &source, &mut cache, &mut state, 1);
        for frame in 0..4 {
            step(&children, &source, &mut cache, &mut state, 1);
            assert!(!cache.contains(parent));
            assert!(cache.used_bytes() <= cache.capacity_bytes());
            if frame < 3 {
                assert!(state.contains(parent));
            }
            check_covered(&state, parent);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
        assert_eq!(
            source
                .calls
                .borrow()
                .iter()
                .filter(|id| **id == parent)
                .count(),
            1
        );
    }

    #[test]
    fn tiny_cache_does_not_starve_available_tiles_without_a_parent() {
        let children = parent().children().unwrap();
        let mut source = Source::default();
        for child in children {
            source.add(child);
        }
        let mut cache = TileCache::new(dem(parent()).memory_footprint());
        let mut state = TerrainSelectionState::default();
        for _ in 0..16 {
            step(&children, &source, &mut cache, &mut state, 2);
            assert!(cache.used_bytes() <= cache.capacity_bytes());
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
    }

    #[test]
    fn zero_budget_preserves_meshes_but_does_not_prepare_cache_hits() {
        let parent = parent();
        let children = parent.children().unwrap();
        let source = Source::default();
        let mut cache = TileCache::new(1_000_000);
        cache.insert(parent, dem(parent));
        let mut state = TerrainSelectionState::default();
        let update = step(&children, &source, &mut cache, &mut state, 0);
        assert!(update.prepared.is_empty());
        assert!(state.is_empty());
        step(&children, &source, &mut cache, &mut state, 1);
        assert!(state.contains(parent));
        cache.clear();
        step(&children, &source, &mut cache, &mut state, 0);
        assert!(state.contains(parent));
        for child in children {
            cache.insert(child, dem(child));
        }
        let update = step(&children, &source, &mut cache, &mut state, 0);
        assert!(update.prepared.is_empty());
        assert!(state.contains(parent));
        let update = step(&children, &source, &mut cache, &mut state, 4);
        assert_eq!(update.prepared.len(), 4);
        assert_eq!(update.load_attempts, 0);
        assert_eq!(update.spawned, sorted(children));
        assert_eq!(update.despawned, vec![parent]);
    }

    #[test]
    fn retry_history_is_pruned_when_travelling_between_regions() {
        let source = Source::default();
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for x in 0..500 {
            let leaf = TileId::new(10, x * 3, x);
            step(&[leaf], &source, &mut cache, &mut state, 16);
            assert!(state.attempts.len() <= usize::from(leaf.level) + 1);
            for &id in state.attempts.keys() {
                assert!(contains(id, leaf));
            }
        }
        step(&[], &source, &mut cache, &mut state, 16);
        assert!(state.attempts.is_empty());
        assert!(state.is_empty());
    }

    #[test]
    fn load_and_surface_order_are_deterministic() {
        let children = parent().children().unwrap();
        let mut source = Source::default();
        source.add(parent());
        source.add(children[2]);
        let mut states = [
            TerrainSelectionState::default(),
            TerrainSelectionState::default(),
        ];
        let mut caches = [TileCache::new(1_000_000), TileCache::new(1_000_000)];
        let mut reverse = children;
        reverse.reverse();
        for _ in 0..RETRY_FRAMES + 8 {
            let a = step(&children, &source, &mut caches[0], &mut states[0], 1);
            let b = step(&reverse, &source, &mut caches[1], &mut states[1], 1);
            assert_eq!(a, b);
            assert_eq!(states[0].live, states[1].live);
        }
    }

    #[test]
    fn parent_coverage_includes_dateline_and_polar_edges() {
        let parents = [
            TileId::new(3, 0, 0),
            TileId::new(3, 15, 0),
            TileId::new(3, 0, 7),
            TileId::new(3, 15, 7),
        ];
        let wanted: Vec<_> = parents
            .into_iter()
            .flat_map(|p| p.children().unwrap())
            .collect();
        let mut source = Source::default();
        for parent in parents {
            source.add(parent);
        }
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for _ in 0..32 {
            step(&wanted, &source, &mut cache, &mut state, 1);
        }
        for parent in parents {
            check_covered(&state, parent);
        }
        for latitude in [-90.0, -89.999, 89.999, 90.0] {
            for longitude in [-180.0, -179.999, 179.999, 180.0] {
                assert_eq!(
                    covered_at(&state, Geodetic::from_degrees(latitude, longitude, 0.0)),
                    1
                );
            }
        }
    }

    #[test]
    fn a_completely_absent_tree_is_bounded_and_remains_empty() {
        let source = Source::default();
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        let leaf = TileId::new(flightsim_world::tile::MAX_LEVEL, 0, 0);
        for _ in 0..64 {
            step(&[leaf], &source, &mut cache, &mut state, 1);
            assert!(state.is_empty());
        }
        assert_eq!(source.calls.borrow().len(), usize::from(leaf.level) + 1);
    }

    #[test]
    fn unavailable_coarsening_keeps_live_children_even_after_cache_clear() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        for child in children {
            source.add(child);
        }
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        step(&children, &source, &mut cache, &mut state, 4);
        cache.clear();
        let update = step(&[parent], &source, &mut cache, &mut state, 0);
        assert!(update.despawned.is_empty());
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
        for _ in 0..16 {
            step(&[parent], &source, &mut cache, &mut state, 1);
            check_covered(&state, parent);
        }
        source.add(parent);
        for _ in 0..RETRY_FRAMES + 8 {
            step(&[parent], &source, &mut cache, &mut state, 1);
            check_covered(&state, parent);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.resident_len(), 1);
    }

    #[test]
    fn unavailable_coarsening_keeps_partial_live_coverage_and_never_discovers_new_children() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(children[0]);
        source.add(children[2]);
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        for _ in 0..16 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        cache.clear();
        source.calls.borrow_mut().clear();
        for child in children {
            source.add(child);
        }
        for _ in 0..16 {
            step(&[parent], &source, &mut cache, &mut state, 1);
        }
        assert_eq!(
            state.ids().collect::<Vec<_>>(),
            sorted([children[0], children[2]])
        );
        assert!(
            source
                .calls
                .borrow()
                .iter()
                .all(|id| id.level <= parent.level)
        );
        source.add(parent);
        for _ in 0..RETRY_FRAMES + 8 {
            step(&[parent], &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
    }

    #[test]
    fn deep_replacement_prepares_one_mesh_per_frame_before_atomic_activation() {
        let root = TileId::roots()[0];
        let mut leaves = vec![root];
        for _ in 0..4 {
            leaves = leaves
                .into_iter()
                .flat_map(|id| id.children().unwrap())
                .collect();
        }
        assert_eq!(leaves.len(), 256);
        let mut source = Source::default();
        source.add(root);
        for &id in &leaves {
            source.add(id);
        }
        let mut cache = TileCache::new(dem(root).memory_footprint());
        let mut state = TerrainSelectionState::default();
        step(&[root], &source, &mut cache, &mut state, 1);
        for frame in 1..=256 {
            let update = step(&leaves, &source, &mut cache, &mut state, 1);
            assert_eq!(update.prepared.len(), 1);
            if frame < 256 {
                assert_eq!(state.ids().collect::<Vec<_>>(), vec![root]);
                assert!(update.spawned.is_empty());
                assert!(update.despawned.is_empty());
            } else {
                assert_eq!(update.spawned.len(), 256);
                assert_eq!(update.despawned, vec![root]);
            }
            check_covered(&state, root);
        }
    }

    #[test]
    fn stale_hidden_work_is_removed_when_desired_resolution_changes() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        for child in children {
            source.add(child);
        }
        let mut cache = TileCache::new(1_000_000);
        let mut state = TerrainSelectionState::default();
        step(&[parent], &source, &mut cache, &mut state, 1);
        let prepared = step(&children, &source, &mut cache, &mut state, 1).prepared[0];
        assert_eq!(state.resident_len(), 2);
        assert!(!state.contains(prepared));
        let update = step(&[parent], &source, &mut cache, &mut state, 0);
        assert_eq!(update.despawned, vec![prepared]);
        assert_eq!(state.resident_len(), 1);
        assert!(state.contains(parent));
    }

    #[test]
    fn minimum_sufficient_resident_capacity_finishes_a_replacement() {
        let parent = TileId::roots()[0];
        let children = parent.children().unwrap();
        let mut source = Source::default();
        source.add(parent);
        for child in children {
            source.add(child);
        }
        let mut state = TerrainSelectionState {
            resident_limit: 5,
            ..Default::default()
        };
        let mut cache = TileCache::new(dem(parent).memory_footprint());
        step(&[parent], &source, &mut cache, &mut state, 1);
        for _ in 0..4 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
        assert_eq!(state.resident_len(), 4);
    }

    #[test]
    fn invalid_capacity_is_rejected_before_mutating_any_streaming_state() {
        let parent = TileId::roots()[0];
        let mut source = Source::default();
        source.add(parent);
        let mut state = TerrainSelectionState {
            resident_limit: 4,
            ..Default::default()
        };
        let mut cache = TileCache::new(1_000_000);
        step(&[parent], &source, &mut cache, &mut state, 1);
        let calls = source.calls.borrow().len();
        let frame = state.frame;
        let bytes = cache.used_bytes();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            step(
                &parent.children().unwrap(),
                &source,
                &mut cache,
                &mut state,
                1,
            );
        }));
        assert!(result.is_err());
        assert_eq!(source.calls.borrow().len(), calls);
        assert_eq!(state.frame, frame);
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.resident_len(), 1);
        assert_eq!(cache.used_bytes(), bytes);
    }

    #[test]
    fn retained_fine_history_is_bounded_and_nearby_new_coverage_wins() {
        let root = TileId::roots()[0];
        let mut coarse = vec![root];
        for _ in 0..3 {
            coarse = coarse
                .into_iter()
                .flat_map(|id| id.children().unwrap())
                .collect();
        }
        let mut source = Source::default();
        for &id in &coarse {
            for child in id.children().unwrap() {
                source.add(child);
            }
        }
        let mut cache = TileCache::new(dem(root).memory_footprint());
        let mut state = TerrainSelectionState {
            resident_limit: 96,
            ..Default::default()
        };
        let mut hit_limit = false;
        for &region in &coarse {
            let children = region.children().unwrap();
            let wanted: Vec<_> = coarse
                .iter()
                .copied()
                .filter(|id| *id != region)
                .chain(children)
                .collect();
            let center = region.center();
            let camera = Geodetic::new(center.latitude, center.longitude, Meters(100.0)).to_ecef();
            for _ in 0..32 {
                let update = step_at(&wanted, &source, &mut cache, &mut state, 8, camera);
                hit_limit |= update.capacity_limited;
                if children.iter().all(|id| state.contains(*id)) {
                    break;
                }
            }
            assert!(
                children.iter().all(|id| state.contains(*id)),
                "new nearby region was starved: {region:?}"
            );
            check_covered(&state, region);
            assert!(state.resident_len() <= 96);
        }
        assert!(hit_limit, "fixture never exercised resident eviction");
        assert_eq!(state.resident_len(), 96);
    }

    #[test]
    fn the_default_selector_and_its_truncated_tree_fit_the_resident_limit() {
        use flightsim_core::Degrees;
        for error in [16.0, 1.0e-9] {
            let selector = LodSelector::new(
                error,
                1_080.0,
                Degrees(60.0).to_radians(),
                24,
                Meters(20_000.0),
            );
            for (lat, lon, altitude) in [
                (35.55, 139.78, 40.0),
                (90.0, 180.0, 0.0),
                (-90.0, -180.0, 1.0),
                (0.0, 0.0, 30_000.0),
            ] {
                let selection =
                    selector.select(Geodetic::from_degrees(lat, lon, altitude).to_ecef());
                if error < 1.0 {
                    assert!(selection.truncated);
                }
                let mut active = BTreeSet::new();
                for id in selection.tiles {
                    insert_ancestors(id, &mut active);
                }
                assert!(active.len() <= RESIDENT_TILE_LIMIT);
            }
        }
    }

    #[test]
    fn explicit_global_only_seeds_and_refines_without_primary_reads_or_history() {
        let wanted: Vec<_> = TileId::roots()
            .into_iter()
            .flat_map(|id| id.children().unwrap())
            .flat_map(|id| id.children().unwrap())
            .collect();
        for budget in [1, 8] {
            let source = Source {
                fallback: true,
                no_primary: true,
                ..Default::default()
            };
            let mut state = TerrainSelectionState::default();
            let mut cache = TileCache::new(dem(parent()).memory_footprint());
            let zero = step(&wanted, &source, &mut cache, &mut state, 0);
            assert_eq!(zero.load_attempts, 0);
            assert!(state.is_empty());
            let mut calls = 0;
            let mut reads = 0;
            while !state.matches_desired() && calls < 34 {
                let update = step(&wanted, &source, &mut cache, &mut state, budget);
                calls += 1;
                reads += update.load_attempts;
                assert_eq!(update.load_attempts, update.fallback_loaded);
                assert_eq!(update.missing, 0);
                if calls * budget >= 2 {
                    for root in TileId::roots() {
                        assert!(covers_region(root, &state.live));
                    }
                }
                assert!(state.attempts.is_empty());
                assert!(cache.used_bytes() <= cache.capacity_bytes());
            }
            assert!(state.matches_desired());
            assert_eq!(reads, 34); // Two root seeds plus the 32 exact leaves.
            assert_eq!(calls, 34_usize.div_ceil(budget));
            assert_eq!(state.fallback_len(), wanted.len());
            assert!(source.calls.borrow().is_empty());
        }
    }

    #[test]
    fn explicit_no_primary_keeps_cached_and_resident_real_ancestors() {
        let parent = parent();
        let children = parent.children().unwrap();
        for resident in [false, true] {
            let mut state = TerrainSelectionState::default();
            let mut cache = TileCache::new(1_000_000);
            cache.insert(parent, dem(parent));
            if resident {
                let mut original = Source::default();
                original.add(parent);
                step(&children, &original, &mut cache, &mut state, 8);
                cache.clear();
                assert!(state.contains(parent));
            }
            let source = Source {
                fallback: true,
                no_primary: true,
                ..Default::default()
            };
            for _ in 0..RETRY_FRAMES + 1 {
                step(&children, &source, &mut cache, &mut state, 8);
                assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
                assert_eq!(state.fallback_len(), 0);
                check_covered(&state, parent);
            }
            assert!(source.calls.borrow().is_empty());
            assert!(source.fallback_calls.borrow().is_empty());
        }
    }

    #[test]
    fn a_configured_absent_directory_still_discovers_late_real_primary() {
        use flightsim_world::{
            DiskTileSource,
            global::{GlobalTerrain, GlobalTileSource},
        };
        let directory = std::env::temp_dir().join(format!(
            "flightsim-late-primary-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let parent = parent();
        let children = parent.children().unwrap();
        let source = GlobalTileSource::new(
            DiskTileSource::new(&directory),
            GlobalTerrain::bundled().unwrap(),
        );
        assert!(source.primary_reads_possible());
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        let advance = |state: &mut TerrainSelectionState, cache: &mut TileCache| {
            update_selected_tiles(
                children.into_iter().collect(),
                &source,
                cache,
                state,
                camera(),
                8,
                &mut |_, _| {},
            )
        };
        for _ in 0..32 {
            advance(&mut state, &mut cache);
        }
        assert_eq!(state.fallback_len(), 4);
        let path = directory.join(flightsim_world::dem::io::tile_relative_path(parent));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        flightsim_world::dem::io::write_tile(&mut bytes, parent, dem(parent).grid()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        for _ in 0..RETRY_FRAMES + 16 {
            advance(&mut state, &mut cache);
            check_covered(&state, parent);
            if state.fallback_len() == 0 {
                break;
            }
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.fallback_len(), 0);
        assert!(source.primary_reads_possible());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn global_fallback_never_masks_a_real_ancestor() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source {
            fallback: true,
            ..Default::default()
        };
        source.add(parent);
        let mut state = TerrainSelectionState::default();
        state.observe_readiness(true);
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..32 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.fallback_len(), 0);
        assert!(source.fallback_calls.borrow().is_empty());
        assert!(!state.matches_desired());
        assert_eq!(state.observed_readiness(), Some(true));
    }

    #[test]
    fn global_generation_and_meshes_share_the_existing_frame_budgets() {
        let children = parent().children().unwrap();
        let source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..80 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
        assert_eq!(state.fallback_len(), 4);
        assert_eq!(source.fallback_calls.borrow().len(), 4);
        check_covered(&state, parent());
    }

    #[test]
    fn real_tile_arriving_after_global_fallback_replaces_same_id_atomically() {
        let id = parent();
        let mut source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..32 {
            step(&[id], &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.fallback_len(), 1);
        source.add(id);
        let mut replaced = false;
        for _ in 0..160 {
            let update = step(&[id], &source, &mut cache, &mut state, 1);
            assert!(state.contains(id), "replacement must not leave a hole");
            if update.replaced.contains(&id) {
                replaced = true;
                assert!(update.prepared.contains(&id));
                assert!(update.spawned.contains(&id));
                assert!(!update.despawned.contains(&id));
                assert_eq!(state.fallback_len(), 0);
                break;
            }
        }
        assert!(replaced, "real DEM retry never replaced global fallback");
    }

    #[test]
    fn provenance_callback_tracks_same_id_primary_superseding_fallback() {
        let id = parent();
        let mut source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        let mut seen = Vec::new();
        for _ in 0..32 {
            update_selected_tiles_with_provenance(
                BTreeSet::from([id]),
                &source,
                &mut cache,
                &mut state,
                camera(),
                1,
                &mut |prepared, _dem, provenance| {
                    if prepared == id {
                        seen.push(provenance);
                    }
                },
            );
        }
        assert_eq!(seen, vec![TerrainMeshProvenance::Fallback]);
        assert!(state.fallback_cache.contains(&id));
        // Source returns the same numerical DEM fixture through its primary
        // lane. Height matching must never be mistaken for fallback provenance.
        source.tiles.insert(
            id,
            DemTile::new(id.bounds(), HeightGrid::flat(9, 9, Meters(5.0))),
        );
        let mut replaced = false;
        for _ in 0..160 {
            let update = update_selected_tiles_with_provenance(
                BTreeSet::from([id]),
                &source,
                &mut cache,
                &mut state,
                camera(),
                1,
                &mut |prepared, _dem, provenance| {
                    if prepared == id {
                        seen.push(provenance);
                    }
                },
            );
            if update.replaced.contains(&id) {
                replaced = true;
                break;
            }
        }
        assert!(replaced);
        assert_eq!(
            seen,
            vec![
                TerrainMeshProvenance::Fallback,
                TerrainMeshProvenance::Primary
            ]
        );
        assert!(!state.fallback_cache.contains(&id));
        assert!(!state.fallback_resident.contains(&id));
    }

    #[test]
    fn late_real_parent_overrides_already_visible_global_children() {
        let parent = parent();
        let children = parent.children().unwrap();
        let mut source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..32 {
            step(&children, &source, &mut cache, &mut state, 2);
        }
        assert_eq!(state.fallback_len(), 4);
        source.add(parent);
        for _ in 0..160 {
            step(&children, &source, &mut cache, &mut state, 2);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.fallback_len(), 0);
        check_covered(&state, parent);
    }

    #[test]
    fn global_fallback_remains_complete_with_a_one_tile_cache() {
        let children = parent().children().unwrap();
        let source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(dem(parent()).memory_footprint());
        for _ in 0..80 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), sorted(children));
        assert_eq!(state.fallback_len(), 4);
        check_covered(&state, parent());
        for _ in 0..160 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.fallback_len(), 4);
        assert_eq!(
            source.fallback_calls.borrow().len(),
            4,
            "resident meshes avoid regeneration after DEM eviction"
        );
    }

    #[test]
    fn global_fallback_uses_supported_ancestor_above_its_tessellation_cap() {
        let parent = parent();
        let children = parent.children().unwrap();
        let source = Source {
            fallback: true,
            fallback_max_level: Some(parent.level),
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        state.observe_readiness(true);
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..80 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(state.fallback_len(), 1);
        check_covered(&state, parent);
        assert!(!state.matches_desired());
        assert_eq!(state.observed_readiness(), Some(true));
    }

    #[test]
    fn readiness_observation_is_opt_in_and_rejects_zero_budget_or_cached_work() {
        let children = parent().children().unwrap();
        let source = Source {
            no_primary: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for id in children {
            cache.insert(id, dem(id));
        }
        step(&children, &source, &mut cache, &mut state, 1);
        assert_eq!(state.observed_readiness(), None);
        state.observe_readiness(true);
        step(&children, &source, &mut cache, &mut state, 0);
        assert_eq!(state.observed_readiness(), Some(false));
        step(&children, &source, &mut cache, &mut state, 1);
        assert_eq!(state.observed_readiness(), Some(false));
        for _ in 0..2 {
            step(&children, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.observed_readiness(), Some(true));
        state.observe_readiness(false);
        step(&children, &source, &mut cache, &mut state, 1);
        assert_eq!(state.observed_readiness(), None);
    }

    #[test]
    fn readiness_does_not_mistake_evicted_success_for_a_missing_dependency() {
        let id = parent();
        let wanted = BTreeSet::from([id]);
        let cache = TileCache::new(1_000_000);
        for fallback in [false, true] {
            let mut state = TerrainSelectionState::default();
            state.observe_readiness(true);
            let mut ancestor = Some(id);
            while let Some(tile) = ancestor {
                state.attempts.insert(
                    tile,
                    LoadAttempt {
                        frame: 1,
                        outcome: LoadOutcome::Missing,
                    },
                );
                state
                    .readiness_observation
                    .as_mut()
                    .unwrap()
                    .fallback_outcomes
                    .insert(tile, LoadOutcome::Missing);
                ancestor = tile.parent();
            }
            state.attempts.insert(
                id,
                LoadAttempt {
                    frame: 1,
                    outcome: LoadOutcome::Loaded,
                },
            );
            state
                .readiness_observation
                .as_mut()
                .unwrap()
                .fallback_outcomes
                .insert(id, LoadOutcome::Loaded);
            assert!(!state.dependencies_examined(&wanted, &cache, !fallback, fallback));
        }
    }

    #[test]
    fn readiness_observation_preserves_source_calls_budgets_and_selection_updates() {
        let wanted = parent().children().unwrap();
        let mut ordinary_source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut observed_source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut ordinary = TerrainSelectionState::default();
        let mut observed = TerrainSelectionState::default();
        observed.observe_readiness(true);
        let mut ordinary_cache = TileCache::new(1_000_000);
        let mut observed_cache = TileCache::new(1_000_000);
        for frame in 0..180 {
            if frame == 40 {
                ordinary_source.add(parent());
                observed_source.add(parent());
            }
            let a = step(
                &wanted,
                &ordinary_source,
                &mut ordinary_cache,
                &mut ordinary,
                1,
            );
            let b = step(
                &wanted,
                &observed_source,
                &mut observed_cache,
                &mut observed,
                1,
            );
            assert_eq!(a, b);
            assert_eq!(ordinary.live, observed.live);
            assert_eq!(
                *ordinary_source.calls.borrow(),
                *observed_source.calls.borrow()
            );
            assert_eq!(
                *ordinary_source.fallback_calls.borrow(),
                *observed_source.fallback_calls.borrow()
            );
            assert_eq!(ordinary.observed_readiness(), None);
        }
    }

    #[test]
    fn late_readiness_observation_waits_for_normal_retries_and_prunes_history() {
        let id = parent();
        let source = Source {
            no_primary: true,
            fallback: true,
            fallback_max_level: Some(0),
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..16 {
            step(&[id], &source, &mut cache, &mut state, 1);
        }
        assert!(state.readiness_observation.is_none());
        let calls = source.fallback_calls.borrow().len();
        state.observe_readiness(true);
        step(&[id], &source, &mut cache, &mut state, 1);
        assert_eq!(
            source.fallback_calls.borrow().len(),
            calls,
            "observer must not accelerate retries"
        );
        assert_eq!(
            state.observed_readiness(),
            Some(false),
            "unobserved missing history is unknown"
        );
        for _ in 0..RETRY_FRAMES + 16 {
            step(&[id], &source, &mut cache, &mut state, 1);
            if state.observed_readiness() == Some(true) {
                break;
            }
        }
        assert_eq!(state.observed_readiness(), Some(true));
        let next = TileId::roots()[0];
        step(&[next], &source, &mut cache, &mut state, 1);
        assert!(
            state
                .readiness_observation
                .as_ref()
                .unwrap()
                .fallback_outcomes
                .keys()
                .all(|&tile| tile == next)
        );
        state.frame = u64::MAX;
        step(&[next], &source, &mut cache, &mut state, 0);
        assert!(
            state
                .readiness_observation
                .as_ref()
                .unwrap()
                .fallback_outcomes
                .is_empty()
        );
        assert_eq!(state.observed_readiness(), Some(false));
        state.observe_readiness(false);
        assert!(state.readiness_observation.is_none());
        assert_eq!(
            std::mem::size_of::<Option<Box<ReadinessObservation>>>(),
            std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn polar_coarse_primary_readiness_finishes_despite_continuous_missing_retries() {
        let selector = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        );
        let camera = Geodetic::from_degrees(90.0, 0.0, 1215.0).to_ecef();
        let wanted = selector.select_with_surface(camera, Meters(14.90934)).tiles;
        assert_eq!(wanted.len(), 4_094);
        let mut source = Source {
            fallback: true,
            ..Default::default()
        };
        for root in TileId::roots() {
            source.add(root);
        }
        let mut state = TerrainSelectionState::default();
        state.observe_readiness(true);
        let mut cache = TileCache::new(512 * 1024 * 1024);
        let mut ready_at = None;
        for frame in 0..1_200 {
            let update = step_at(&wanted, &source, &mut cache, &mut state, 8, camera);
            assert!(
                update.load_attempts > 0,
                "fixture must expose the old idle-update deadlock"
            );
            if state.observed_readiness() == Some(true) {
                ready_at = Some(frame);
                break;
            }
        }
        assert!(ready_at.is_some_and(|frame| frame >= RETRY_FRAMES));
        assert!(!state.matches_desired());
        assert_eq!(state.ids().collect::<Vec<_>>(), TileId::roots());
        assert_eq!(state.fallback_len(), 0);
        // No full cut, hidden model or renderer readiness is claimed here.
        // This proves only that continued known-missing retries are not fresh
        // discovery and cannot starve an otherwise valid availability capture.
    }

    #[test]
    fn global_cold_start_seeds_complete_globe_before_fine_discovery() {
        let wanted: Vec<_> = TileId::roots()
            .into_iter()
            .flat_map(|id| id.children().unwrap())
            .flat_map(|id| id.children().unwrap())
            .collect();
        let source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        for _ in 0..4 {
            step(&wanted, &source, &mut cache, &mut state, 1);
        }
        assert_eq!(state.ids().collect::<Vec<_>>(), TileId::roots());
        assert_eq!(state.fallback_len(), 2);
        for root in TileId::roots() {
            assert!(covers_region(root, &state.live));
        }
        for _ in 0..100 {
            step(&wanted, &source, &mut cache, &mut state, 2);
        }
        assert_eq!(
            state.ids().collect::<BTreeSet<_>>(),
            wanted.into_iter().collect()
        );
        for root in TileId::roots() {
            assert!(covers_region(root, &state.live));
        }
    }

    #[test]
    fn eligible_fallback_and_primary_work_both_progress_under_continuous_demand() {
        let near = TileId::new(4, 2, 3);
        let far = TileId::new(4, 24, 11);
        let mut near_leaves = vec![near];
        for _ in 0..4 {
            near_leaves = near_leaves
                .into_iter()
                .flat_map(|id| id.children().unwrap())
                .collect();
        }
        let wanted: Vec<_> = near_leaves
            .iter()
            .copied()
            .chain(far.children().unwrap())
            .collect();
        let mut active = BTreeSet::new();
        for &id in &wanted {
            insert_ancestors(id, &mut active);
        }
        for camera_region in [near, far] {
            let center = camera_region.center();
            let camera = Geodetic::new(center.latitude, center.longitude, Meters(100.0)).to_ecef();
            for (fail_fallback, fresh_primary) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                for budget in [1, 8] {
                    let mut source = Source {
                        fallback: true,
                        ..Default::default()
                    };
                    // Exercise both distance orders, and unsuccessful fallback reads.
                    // A real coarse tile is newly discovered or has arrived after
                    // a failed read. Its first read/retry and the fallback lane
                    // must each get a turn.
                    source.add(far);
                    if fail_fallback {
                        source.fallback_errors.extend(active.iter().copied());
                    }
                    let mut state = TerrainSelectionState {
                        frame: RETRY_FRAMES + 1,
                        ..Default::default()
                    };
                    state.attempts.extend(active.iter().map(|&id| {
                        (
                            id,
                            LoadAttempt {
                                frame: 1,
                                outcome: LoadOutcome::Missing,
                            },
                        )
                    }));
                    state.attempts.insert(
                        far,
                        LoadAttempt {
                            frame: 0,
                            outcome: LoadOutcome::Failed,
                        },
                    );
                    if fresh_primary {
                        state.attempts.remove(&far);
                    }
                    state.live.extend([near]);
                    state.live.extend(far.children().unwrap());
                    state.resident.clone_from(&state.live);
                    state.fallback_resident.clone_from(&state.live);
                    let mut cache = TileCache::new(dem(near).memory_footprint());
                    let frames = 32 / budget;
                    let mut primary_reads = 0;
                    let mut fallback_reads = 0;
                    for frame in 0..frames {
                        assert!(
                            state
                                .next_request(&wanted.iter().copied().collect(), &cache, camera)
                                .is_some()
                        );
                        assert!(
                            state
                                .next_fallback(
                                    true,
                                    &wanted.iter().copied().collect(),
                                    &cache,
                                    camera
                                )
                                .is_some()
                        );
                        let before_primary = source.calls.borrow().len();
                        let before_fallback = source.fallback_calls.borrow().len();
                        step_at(&wanted, &source, &mut cache, &mut state, budget, camera);
                        primary_reads += source.calls.borrow().len() - before_primary;
                        fallback_reads += source.fallback_calls.borrow().len() - before_fallback;
                        assert!(primary_reads.abs_diff(fallback_reads) <= 1);
                        if (frame + 1) * budget >= 2 {
                            assert!(
                                state.contains(far),
                                "new real ancestor must get a fair read"
                            );
                            assert!(!state.fallback_resident.contains(&far));
                        }
                        check_covered(&state, near);
                        check_covered(&state, far);
                        assert!(cache.used_bytes() <= cache.capacity_bytes());
                    }
                    assert_eq!(primary_reads, 16);
                    assert_eq!(fallback_reads, 16);
                }
            }
        }
    }

    #[test]
    fn missing_and_failed_fallback_reads_take_a_turn_and_retry_after_data_arrives() {
        for no_primary in [false, true] {
            for budget in [1, 8] {
                for fail in [false, true] {
                    // A complete sibling cut can replace its covering fallback
                    // parent; one child alone correctly leaves that parent visible.
                    let wanted = parent().children().unwrap();
                    let id = *wanted
                        .iter()
                        .min_by_key(|&&id| tile_priority(id, camera()))
                        .unwrap();
                    let mut source = Source {
                        fallback: true,
                        no_primary,
                        fallback_max_level: (!fail).then_some(id.level - 1),
                        ..Default::default()
                    };
                    if fail {
                        source.fallback_errors.extend(wanted);
                    }
                    let mut state = TerrainSelectionState::default();
                    let mut cache = TileCache::new(dem(id).memory_footprint());
                    let mut last_attempt = None;
                    for _ in 0..32 {
                        let update = step(&wanted, &source, &mut cache, &mut state, budget);
                        if state.fallback_attempts.contains_key(&id) {
                            assert!(if fail {
                                update.failed > 0
                            } else {
                                update.missing > 0
                            });
                            last_attempt = state.fallback_attempts.get(&id).copied();
                            break;
                        }
                    }
                    let failed_at =
                        last_attempt.expect("fixture must try its unavailable fallback");
                    source.fallback_errors.clear();
                    source.fallback_max_level = None;
                    let calls_before = source
                        .fallback_calls
                        .borrow()
                        .iter()
                        .filter(|&&tile| tile == id)
                        .count();
                    let mut recovered = false;
                    for _ in 0..RETRY_FRAMES + 16 {
                        step(&wanted, &source, &mut cache, &mut state, budget);
                        let calls_now = source
                            .fallback_calls
                            .borrow()
                            .iter()
                            .filter(|&&tile| tile == id)
                            .count();
                        if state.frame - failed_at < RETRY_FRAMES {
                            assert_eq!(
                                calls_now, calls_before,
                                "fallback retry ignored its cooldown"
                            );
                        }
                        if state.matches_desired() {
                            assert!(state.frame - failed_at >= RETRY_FRAMES);
                            assert_eq!(calls_now, calls_before + 1);
                            recovered = true;
                            break;
                        }
                    }
                    assert!(
                        recovered,
                        "available fallback did not recover after its cooldown: budget={budget}, fail={fail}"
                    );
                    assert_eq!(state.ids().collect::<Vec<_>>(), sorted(wanted));
                    check_covered(&state, parent());
                }
            }
        }
    }

    #[test]
    fn large_global_cold_start_and_moved_camera_converge_to_exact_desired_ids() {
        let selector = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        );
        let source = Source {
            fallback: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(512 * 1024 * 1024);
        // This default-size exact-pole cut exposed a 684-call tail: all but one
        // fallback mesh were ready, but nearby primary retries won every read.
        // The moved cut also has 4,094 leaves, so counts cannot prove convergence.
        for (lat, lon, alt, max_calls) in [(90.0, 0.0, 1215.0, 1_200), (89.9914, 90.0, 1282.0, 450)]
        {
            let camera = Geodetic::from_degrees(lat, lon, alt).to_ecef();
            let wanted = selector.select_with_surface(camera, Meters(14.90934)).tiles;
            let wanted_set: BTreeSet<_> = wanted.iter().copied().collect();
            assert_eq!(wanted_set.len(), 4_094);
            assert_ne!(state.live, wanted_set);
            let mut converged = false;
            for _ in 0..max_calls {
                step_at(&wanted, &source, &mut cache, &mut state, 8, camera);
                for root in TileId::roots() {
                    assert!(covers_region(root, &state.live), "global coverage was lost");
                }
                assert!(cache.used_bytes() <= cache.capacity_bytes());
                if state.matches_desired() {
                    assert_eq!(state.live, wanted_set);
                    converged = true;
                    break;
                }
            }
            assert!(
                converged,
                "bounded streaming failed to reach the exact desired cut"
            );
        }
    }

    #[test]
    fn explicit_global_only_large_cold_and_moved_cuts_need_no_primary_probes() {
        let selector = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        );
        let source = Source {
            fallback: true,
            no_primary: true,
            ..Default::default()
        };
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(512 * 1024 * 1024);
        for (lat, lon, alt, expected_calls, expected_reads, expected_prepared) in [
            (90.0, 0.0, 1215.0, 512, 4096, 4096),
            (89.9914, 90.0, 1282.0, 221, 1710, 1761),
        ] {
            let camera = Geodetic::from_degrees(lat, lon, alt).to_ecef();
            let wanted = selector.select_with_surface(camera, Meters(14.90934)).tiles;
            assert_eq!(wanted.len(), 4094);
            let mut reads = 0;
            let mut prepared = 0;
            for call in 1..=expected_calls {
                let update = step_at(&wanted, &source, &mut cache, &mut state, 8, camera);
                reads += update.load_attempts;
                prepared += update.prepared.len();
                assert_eq!(update.load_attempts, update.fallback_loaded);
                assert!(state.attempts.is_empty());
                assert!(cache.used_bytes() <= cache.capacity_bytes());
                for root in TileId::roots() {
                    assert!(covers_region(root, &state.live));
                }
                assert_eq!(state.matches_desired(), call == expected_calls);
            }
            assert_eq!(reads, expected_reads);
            assert_eq!(prepared, expected_prepared);
            assert_eq!(state.live, wanted.into_iter().collect());
        }
        assert!(source.calls.borrow().is_empty());
    }

    #[test]
    fn desired_diagnostics_compare_ids_instead_of_only_counts() {
        let old = parent();
        let new = old.parent().unwrap();
        let mut source = Source::default();
        source.add(old);
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        step(&[old], &source, &mut cache, &mut state, 1);
        assert!(state.matches_desired());
        step(&[new], &source, &mut cache, &mut state, 1);
        assert_eq!(state.desired_len(), state.len());
        assert!(
            !state.matches_desired(),
            "retained finer coverage is not the desired ID"
        );
        source.add(new);
        for _ in 0..RETRY_FRAMES + 1 {
            step(&[new], &source, &mut cache, &mut state, 1);
            if state.matches_desired() {
                break;
            }
        }
        assert!(state.matches_desired());
        assert_eq!(state.ids().collect::<Vec<_>>(), vec![new]);
        step(&[], &source, &mut cache, &mut state, 1);
        assert_eq!(state.desired_len(), 0);
        assert!(state.matches_desired());
    }

    #[test]
    fn streaming_exposes_when_lod_detail_was_limited() {
        let source = Source::default();
        let mut state = TerrainSelectionState::default();
        let mut cache = TileCache::new(1_000_000);
        let selector = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        )
        .with_max_tiles(2);
        update_terrain_selection_with_surface(
            &selector,
            &source,
            &mut cache,
            &mut state,
            camera(),
            Meters::ZERO,
            0,
            &mut |_, _| {},
        );
        assert!(state.selection_truncated());
        let root_only = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            0,
            Meters(20_000.0),
        );
        update_terrain_selection(
            &root_only,
            &source,
            &mut cache,
            &mut state,
            camera(),
            0,
            &mut |_, _| {},
        );
        assert!(!state.selection_truncated());
    }
    #[test]
    fn indexed_sparse_region_survives_climb_move_and_missing_payloads_under_budget() {
        let region = TileId::new(10, 1078, 244);
        let selector = LodSelector::new(
            16.0,
            1080.0,
            flightsim_core::Degrees(60.0).to_radians(),
            13,
            Meters(20_000.0),
        );
        for outcome in [
            LoadOutcome::Loaded,
            LoadOutcome::Missing,
            LoadOutcome::Failed,
        ] {
            let missing = !matches!(outcome, LoadOutcome::Loaded);
            let mut source = Source {
                fallback: true,
                coverage: Some(flightsim_world::PrimaryCoverage::from_tiles([region]).unwrap()),
                ..Default::default()
            };
            if !missing {
                source.add(region);
            }
            if matches!(outcome, LoadOutcome::Failed) {
                source.malformed.insert(region);
            }
            let mut cache = TileCache::new(1_000_000);
            let mut state = TerrainSelectionState::default();
            let mut globe_seeded = false;
            for (lat, lon, agl) in [
                (47.068, 9.501, 3000.0),
                (47.068, 9.501, 100.0),
                (47.068, 9.501, 3000.0),
                (47.068, 9.520, 3000.0),
                (48.0, 10.0, 3000.0),
                (47.068, 9.501, 3000.0),
            ] {
                let camera = Geodetic::from_degrees(lat, lon, agl).to_ecef();
                state.observe_readiness(true);
                let mut ready = false;
                for _ in 0..500 {
                    let update = update_terrain_selection_with_surface_and_provenance(
                        &selector,
                        &source,
                        &mut cache,
                        &mut state,
                        camera,
                        Meters::ZERO,
                        1,
                        &mut |_, _, _| {},
                    );
                    assert!(update.load_attempts <= 1 && update.prepared.len() <= 1);
                    assert!(cache.used_bytes() <= cache.capacity_bytes());
                    check_no_overlap(&state);
                    let complete = TileId::roots()
                        .into_iter()
                        .all(|root| covers_region(root, &state.live));
                    if globe_seeded {
                        assert!(complete, "previous complete coverage was lost");
                    }
                    globe_seeded |= complete;
                    if state.observed_readiness() == Some(true) {
                        ready = true;
                        break;
                    }
                }
                assert!(ready, "streaming failed to settle at {lat},{lon},{agl}");
                if lat < 47.1 && agl >= 3000.0 && !missing {
                    assert!(state.contains(region));
                    assert!(!state.fallback_resident.contains(&region));
                }
                if missing {
                    assert_eq!(state.fallback_len(), state.len());
                }
            }
        }
    }
}
