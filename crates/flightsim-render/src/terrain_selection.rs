//! Availability-aware render-only LOD selection.
//!
//! Read DEMs and prepare hidden meshes under separate per-frame budgets. Only
//! activate a non-overlapping cut once every mesh in a replacement is prepared.
//! None of this changes the ground sampler or writes to simulation state.

use flightsim_core::Ecef;
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
    frame: u64,
    resident_limit: usize,
}

impl Default for TerrainSelectionState {
    fn default() -> Self {
        Self {
            live: BTreeSet::new(),
            resident: BTreeSet::new(),
            attempts: HashMap::new(),
            frame: 0,
            resident_limit: RESIDENT_TILE_LIMIT,
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

    fn available(&self, id: TileId, cache: &TileCache) -> bool {
        self.resident.contains(&id) || cache.contains(id)
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
            while let Some(id) = ancestor {
                if self.resident.contains(&id) {
                    break;
                }
                if cache.contains(id) {
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

    fn remove(&mut self, id: TileId, update: &mut TerrainUpdate) {
        self.live.remove(&id);
        self.resident.remove(&id);
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
    /// Includes missing and failed reads; at most the frame budget.
    pub load_attempts: usize,
    pub missing: usize,
    pub failed: usize,
    /// The resident ceiling evicted retained finer coverage or deferred work.
    pub capacity_limited: bool,
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
/// This does not discover unseen finer descendants of an unavailable coarse tile.
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
    update_selected_tiles(
        selector.select(camera).tiles.into_iter().collect(),
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
    let mut active = BTreeSet::new();
    for &leaf in &wanted {
        insert_ancestors(leaf, &mut active);
    }
    assert!(
        active.len() <= state.resident_limit,
        "the desired terrain tree exceeds the resident mesh limit"
    );
    state.attempts.retain(|id, _| active.contains(id));
    state.frame = state.frame.wrapping_add(1);
    if state.frame == 0 {
        state.attempts.clear();
    }
    let previous_live = state.live.clone();
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
    while update.load_attempts < frame_budget {
        let Some(id) = state.next_request(&wanted, cache, camera) else {
            break;
        };
        update.load_attempts += 1;
        let outcome = match source.load(id) {
            Ok(Some(tile)) => {
                cache.insert(id, tile);
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
        state.attempts.insert(
            id,
            LoadAttempt {
                frame: state.frame,
                outcome,
            },
        );
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
        resolve_cut(root, false, &wanted, &tree, state, &mut cut);
    }
    let keep: BTreeSet<_> = cut.into_iter().collect();
    update.spawned.extend(keep.difference(&previous_live));

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
    state.live = keep;
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
    mesh_sink: &mut dyn FnMut(TileId, &DemTile),
    update: &mut TerrainUpdate,
) {
    while update.prepared.len() < frame_budget {
        let Some(id) = state.next_preparation(wanted, cache, camera) else {
            break;
        };
        if !state.reserve(id, active, camera, update) {
            break;
        }
        let tile = cache.get(id).expect("prepared tile has a cached DEM");
        mesh_sink(id, tile);
        state.resident.insert(id);
        update.prepared.push(id);
    }
}

/// Roll back every descendant if a ready ancestor must cover an incomplete cut.
/// Below an unavailable desired coarse tile, consider only already visible finer
/// meshes carried into `tree`; no new descendant data is searched or requested.
fn resolve_cut(
    id: TileId,
    below_wanted: bool,
    wanted: &BTreeSet<TileId>,
    tree: &BTreeSet<TileId>,
    state: &TerrainSelectionState,
    cut: &mut Vec<TileId>,
) -> bool {
    if !tree.contains(&id) {
        return false;
    }
    let below_wanted = below_wanted || wanted.contains(&id);
    if below_wanted && state.resident.contains(&id) {
        cut.push(id);
        return true;
    }
    let start = cut.len();
    let mut covered = true;
    if let Some(children) = id.children() {
        for child in children {
            covered &= resolve_cut(child, below_wanted, wanted, tree, state, cut);
        }
    } else {
        covered = false;
    }
    if covered {
        return true;
    }
    if state.resident.contains(&id) {
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
    }

    impl Source {
        fn add(&mut self, id: TileId) {
            self.tiles.insert(id, dem(id));
        }
    }

    impl TileSource for Source {
        fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
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
        let ids: Vec<_> = state.ids().collect();
        for (index, &a) in ids.iter().enumerate() {
            for &b in &ids[index + 1..] {
                assert!(!contains(a, b) && !contains(b, a), "overlap: {a:?}, {b:?}");
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
        let before = source.calls.borrow().len();
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
        assert_eq!(update.load_attempts, source.calls.borrow().len() - before);
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
}
