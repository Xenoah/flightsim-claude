//! Incremental topology planning. A step never scans or clones the whole cut.

use std::ops::Bound::{Excluded, Unbounded};

use super::{
    CornerKey, CornerPatch, EdgeExtent, PoleCap, SeamVertex, TerrainBoundary, TerrainEdge,
    TerrainSeam, TerrainSeamKey, WORLD_HEIGHT, mean_vertex,
};
use crate::TileId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Default)]
struct IndexedLine {
    before: BTreeMap<(u32, u32), EdgeExtent>,
    after: BTreeMap<(u32, u32), EdgeExtent>,
}

#[derive(Debug, Default)]
struct CornerSamples {
    samples: BTreeMap<TileId, SeamVertex>,
    links: BTreeSet<(TileId, TileId)>,
}

#[derive(Debug)]
struct PlannedCorner {
    anchor: SeamVertex,
    patch: Option<Arc<CornerPatch>>,
}

#[derive(Debug, Clone, Copy)]
struct Pair {
    first: EdgeExtent,
    second: EdgeExtent,
    start: u32,
    end: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    Index,
    Pairs,
    Samples,
    Corners,
    Poles,
    Emit,
    CleanupCorners,
    CleanupCaps,
    Finished,
}

/// Scratch record counts, excluding caller-owned boundaries and emitted seams.
/// Each field has a linear bound for a non-overlapping N-tile cut: at most 4N
/// edges, 4N pairs, 8N corners, 2N polar edges and 2N assigned caps. Ordinary
/// corners retain at most four samples and four links. There are two polar means.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TerrainSeamPlanningUsage {
    pub indexed_edges: usize,
    pub adjacent_pairs: usize,
    pub corners: usize,
    pub polar_edges: usize,
    pub assigned_caps: usize,
}

/// Deterministic, incremental equivalent of [`super::plan_seams`].
///
/// Construction is constant work and does not clone boundaries or the cut.
/// [`Self::advance`] charges every indexing/sweep/corner/polar/emission/cleanup
/// step to its explicit work budget. One unit handles at most four edges, one
/// interval comparison, four ordinary corner samples/links, one polar vertex,
/// one emitted seam, or one scratch record. Ordered-map operations are O(log N).
/// No bulk sorting, final collection, or completed-plan scratch drop is hidden
/// outside the budget. Total work is O((N + P) log N), where P is the number of
/// polar boundary vertices (their source lookup is also ordered). Storage is O(N); see [`Self::resource_usage`].
///
/// The caller must keep the same non-overlapping cut and source boundaries until
/// completion or cancellation. Cancellation is simply dropping this planner;
/// unlike normal completion, cancellation may synchronously free O(N) scratch.
/// No worker or stale result survives cancellation. The caller owns emitted
/// descriptors and must discard them too when cancelling.
#[derive(Debug, Default)]
pub struct TerrainSeamPlanner {
    phase: Phase,
    tile_cursor: Option<TileId>,
    lines: BTreeMap<super::LineKey, IndexedLine>,
    active_line: Option<IndexedLine>,
    indexed_edges: usize,
    pairs: BTreeMap<TerrainSeamKey, Pair>,
    pair_cursor: Option<TerrainSeamKey>,
    corners: BTreeMap<CornerKey, CornerSamples>,
    final_corners: BTreeMap<CornerKey, PlannedCorner>,
    polar_edges: BTreeSet<(CornerKey, TileId, TerrainEdge)>,
    active_pole: Option<(CornerKey, TileId, TerrainEdge, usize)>,
    polar_means: BTreeMap<CornerKey, (SeamVertex, u32)>,
    assigned_caps: BTreeSet<(TileId, TerrainEdge)>,
}

impl TerrainSeamPlanner {
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.phase == Phase::Finished
    }

    /// Constant-work inspection; does not traverse scratch maps.
    #[must_use]
    pub fn resource_usage(&self) -> TerrainSeamPlanningUsage {
        TerrainSeamPlanningUsage {
            indexed_edges: self.indexed_edges,
            adjacent_pairs: self.pairs.len(),
            corners: self.corners.len() + self.final_corners.len(),
            polar_edges: self.polar_edges.len() + usize::from(self.active_pole.is_some()),
            assigned_caps: self.assigned_caps.len(),
        }
    }

    /// Perform at most `work_budget` bounded units, returning the actual count.
    /// Descriptors arrive in the same key order as [`super::plan_seams`]. The
    /// callback must itself do bounded work, such as inserting one descriptor.
    /// Zero budget does nothing; positive budgets always advance toward completion.
    ///
    /// # Panics
    /// Panics if the caller removes a needed source during planning, or supplies
    /// an overlapping cut with more than four incidences at an ordinary corner.
    pub fn advance(
        &mut self,
        boundaries: &BTreeMap<TileId, TerrainBoundary>,
        visible: &BTreeSet<TileId>,
        work_budget: usize,
        mut emit: impl FnMut(TerrainSeam),
    ) -> usize {
        let mut worked = 0;
        while worked < work_budget && !self.is_finished() {
            self.step(boundaries, visible, &mut emit);
            worked += 1;
        }
        worked
    }

    fn step(
        &mut self,
        boundaries: &BTreeMap<TileId, TerrainBoundary>,
        visible: &BTreeSet<TileId>,
        emit: &mut impl FnMut(TerrainSeam),
    ) {
        match self.phase {
            Phase::Index => {
                let tile = self.tile_cursor.map_or_else(
                    || visible.first().copied(),
                    |last| visible.range((Excluded(last), Unbounded)).next().copied(),
                );
                let Some(tile) = tile else {
                    self.phase = Phase::Pairs;
                    return;
                };
                self.tile_cursor = Some(tile);
                if !boundaries.contains_key(&tile) {
                    return;
                }
                for edge in TerrainEdge::ALL {
                    let extent = EdgeExtent::new(tile, edge);
                    let line = self.lines.entry(extent.line).or_default();
                    let side = if matches!(edge, TerrainEdge::East | TerrainEdge::South) {
                        &mut line.before
                    } else {
                        &mut line.after
                    };
                    side.insert((extent.start, extent.end), extent);
                    self.indexed_edges += 1;
                    if !extent.line.vertical && matches!(extent.line.coordinate, 0 | WORLD_HEIGHT) {
                        self.polar_edges
                            .insert((extent.corner(extent.start), tile, edge));
                    }
                }
            }
            Phase::Pairs => self.sweep_step(),
            Phase::Samples => {
                let pair = self.pair_cursor.map_or_else(
                    || self.pairs.first_key_value(),
                    |last| self.pairs.range((Excluded(last), Unbounded)).next(),
                );
                let Some((&key, &pair)) = pair else {
                    self.phase = Phase::Corners;
                    return;
                };
                self.pair_cursor = Some(key);
                for coordinate in [pair.start, pair.end] {
                    let corner = pair.first.corner(coordinate);
                    // Polar anchors are recomputed from all edge vertices. Do
                    // not retain an unbounded polar "ordinary corner" map.
                    if matches!(corner.y, 0 | WORLD_HEIGHT) {
                        continue;
                    }
                    let samples = self.corners.entry(corner).or_default();
                    samples.links.insert((pair.first.tile, pair.second.tile));
                    for edge in [pair.first, pair.second] {
                        samples.samples.insert(
                            edge.tile,
                            boundaries[&edge.tile].sample(edge.edge, f64::from(coordinate)),
                        );
                    }
                    assert!(
                        samples.samples.len() <= 4 && samples.links.len() <= 4,
                        "visible tiles must be a non-overlapping cut"
                    );
                }
            }
            Phase::Corners => {
                let Some((key, corner)) = self.corners.pop_first() else {
                    self.phase = Phase::Poles;
                    return;
                };
                let anchor = mean_vertex(corner.samples.values().copied());
                let indices: BTreeMap<_, _> = corner
                    .samples
                    .keys()
                    .enumerate()
                    .map(|(index, tile)| (*tile, index))
                    .collect();
                let patch = Arc::new(CornerPatch {
                    vertices: corner.samples.into_values().collect(),
                    links: corner
                        .links
                        .into_iter()
                        .map(|(first, second)| [indices[&first], indices[&second]])
                        .collect(),
                });
                self.final_corners.insert(
                    key,
                    PlannedCorner {
                        anchor,
                        patch: Some(patch),
                    },
                );
            }
            Phase::Poles => self.polar_step(boundaries),
            Phase::Emit => {
                let Some((key, pair)) = self.pairs.pop_first() else {
                    self.phase = Phase::CleanupCorners;
                    return;
                };
                let corner_keys = [pair.first.corner(pair.start), pair.first.corner(pair.end)];
                let mut pole_caps = Vec::new();
                for corner in corner_keys {
                    if matches!(corner.y, 0 | WORLD_HEIGHT) {
                        let edge = if corner.y == 0 {
                            TerrainEdge::North
                        } else {
                            TerrainEdge::South
                        };
                        for tile in [pair.first.tile, pair.second.tile] {
                            if self.assigned_caps.insert((tile, edge)) {
                                pole_caps.push(PoleCap {
                                    tile,
                                    edge,
                                    anchor: self.final_corners[&corner].anchor,
                                });
                            }
                        }
                    }
                }
                emit(TerrainSeam {
                    key,
                    start: pair.start,
                    end: pair.end,
                    anchors: corner_keys.map(|key| self.final_corners[&key].anchor),
                    fingerprints: [
                        boundaries[&pair.first.tile].fingerprint,
                        boundaries[&pair.second.tile].fingerprint,
                    ],
                    pole_caps,
                    corner_patches: corner_keys.map(|key| self.final_corners[&key].patch.clone()),
                });
            }
            Phase::CleanupCorners => {
                if self.final_corners.pop_first().is_none() {
                    self.phase = Phase::CleanupCaps;
                }
            }
            Phase::CleanupCaps => {
                if self.assigned_caps.pop_first().is_none() {
                    self.phase = Phase::Finished;
                }
            }
            Phase::Finished => {}
        }
    }

    fn sweep_step(&mut self) {
        let Some(line) = &mut self.active_line else {
            self.active_line = self.lines.pop_first().map(|(_, line)| line);
            if self.active_line.is_none() {
                self.phase = Phase::Samples;
            }
            return;
        };
        match (line.before.first_key_value(), line.after.first_key_value()) {
            (Some((&left, &a)), Some((&right, &b))) => {
                let start = a.start.max(b.start);
                let end = a.end.min(b.end);
                if start < end && a.tile != b.tile {
                    let (first, second) = if a.tile < b.tile { (a, b) } else { (b, a) };
                    self.pairs.insert(
                        TerrainSeamKey {
                            first: first.tile,
                            second: second.tile,
                            edge: first.edge,
                        },
                        Pair {
                            first,
                            second,
                            start,
                            end,
                        },
                    );
                }
                if a.end <= b.end {
                    line.before.remove(&left);
                    self.indexed_edges -= 1;
                }
                if b.end <= a.end {
                    line.after.remove(&right);
                    self.indexed_edges -= 1;
                }
            }
            // Drain unmatched edges one at a time rather than dropping a large
            // remaining side at the end of a line (e.g. the polar boundary).
            (Some(_), None) => {
                line.before.pop_first();
                self.indexed_edges -= 1;
            }
            (None, Some(_)) => {
                line.after.pop_first();
                self.indexed_edges -= 1;
            }
            (None, None) => self.active_line = None,
        }
    }

    fn polar_step(&mut self, boundaries: &BTreeMap<TileId, TerrainBoundary>) {
        let Some((corner, tile, edge, index)) = self.active_pole else {
            if let Some((corner, tile, edge)) = self.polar_edges.pop_first() {
                self.active_pole = Some((corner, tile, edge, 0));
            } else {
                // There are at most two poles; transfer their means in constant
                // work. Reduction order matches the synchronous planner exactly.
                for (key, (anchor, _)) in std::mem::take(&mut self.polar_means) {
                    self.final_corners.insert(
                        key,
                        PlannedCorner {
                            anchor,
                            patch: None,
                        },
                    );
                }
                self.phase = Phase::Emit;
            }
            return;
        };
        let boundary = &boundaries[&tile];
        let vertex = boundary.vertex(edge, index);
        match self.polar_means.entry(corner) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((vertex, 1));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (mean, count) = entry.get_mut();
                *count = count.checked_add(1).expect("polar vertex count fits u32");
                *mean = mean.lerp(vertex, 1.0 / f64::from(*count));
            }
        }
        self.active_pole = if index + 1 < boundary.resolution() {
            Some((corner, tile, edge, index + 1))
        } else {
            None
        };
    }
}
