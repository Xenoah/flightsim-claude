//! Render-only bridges between the **actual, already quantized** edges of a tile cut.
//!
//! Skirts cannot close arbitrary LOD differences: a fine curved edge can be above
//! both a coarse chord and the end of its own skirt. These ribbons join the two
//! piecewise-linear mesh edges instead of estimating an elevation difference.
//! No DEM, geographic sample, or physical terrain query is modified.
//!
//! Adjacency uses integer quadtree coordinates, a periodic longitude, and sorted
//! interval sweeps. Planning a non-overlapping cut takes O(n log n) work and O(n)
//! space, with at most 4N positive-length neighbour pairs across an N-tile cut.
//! A single coarse tile can have many finer neighbours.
//! [`TerrainSeamPlanner`] splits topology work into explicit bounded units;
//! [`plan_seams`] is the synchronous reference. Mesh construction is separate
//! so callers can charge it to their frame budget.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use flightsim_core::{Ecef, Meters};
use glam::DVec3;

use crate::{TerrainMesh, TileId, tile::MAX_LEVEL};

mod planner;
pub use planner::{TerrainSeamPlanner, TerrainSeamPlanningUsage};

const WORLD_WIDTH: u32 = 2 << MAX_LEVEL;
const WORLD_HEIGHT: u32 = 1 << MAX_LEVEL;

/// An edge in increasing geographic-grid order: west to east for horizontal
/// edges, north to south for vertical edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerrainEdge {
    North,
    East,
    South,
    West,
}

impl TerrainEdge {
    const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    const fn index(self) -> usize {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }
}

/// One retained mesh-edge vertex. Positions have exactly the same f32 bits as
/// the source mesh, relative to [`TerrainBoundary::origin`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainBoundaryVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub elevation: f32,
    pub slope: f32,
}

/// Compact source for future stitches, retaining only four mesh edges.
///
/// A default 33-by-33 mesh retains 132 vertices, not its surface or skirt arrays.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainBoundary {
    pub id: TileId,
    pub origin: Ecef,
    edges: [Vec<TerrainBoundaryVertex>; 4],
    fingerprint: u64,
}

impl TerrainBoundary {
    /// Extract the surface boundary after the source mesh's f32 quantization.
    ///
    /// # Panics
    /// Panics if the surface is not a square grid of at least two vertices per
    /// side, or if a required vertex attribute is missing or non-finite.
    #[must_use]
    pub fn from_mesh(id: TileId, mesh: &TerrainMesh) -> Self {
        let resolution = mesh.surface_vertex_count.isqrt();
        assert!(resolution >= 2 && resolution * resolution == mesh.surface_vertex_count);
        let count = mesh.surface_vertex_count;
        assert!(
            mesh.positions.len() >= count
                && mesh.normals.len() >= count
                && mesh.uvs.len() >= count
                && mesh.elevations.len() >= count
                && mesh.slopes.len() >= count
                && mesh.origin.as_vec().is_finite()
        );
        let last = resolution - 1;
        let edges = TerrainEdge::ALL.map(|edge| {
            (0..resolution)
                .map(|step| {
                    let index = match edge {
                        TerrainEdge::North => step,
                        TerrainEdge::East => step * resolution + last,
                        TerrainEdge::South => last * resolution + step,
                        TerrainEdge::West => step * resolution,
                    };
                    let vertex = TerrainBoundaryVertex {
                        position: mesh.positions[index],
                        normal: mesh.normals[index],
                        uv: mesh.uvs[index],
                        elevation: mesh.elevations[index],
                        slope: mesh.slopes[index],
                    };
                    assert!(
                        vertex.position.into_iter().all(f32::is_finite)
                            && vertex.normal.into_iter().all(f32::is_finite)
                            && vertex.uv.into_iter().all(f32::is_finite)
                            && vertex.elevation.is_finite()
                            && vertex.slope.is_finite()
                    );
                    vertex
                })
                .collect()
        });
        let mut boundary = Self {
            id,
            origin: mesh.origin,
            edges,
            fingerprint: 0,
        };
        boundary.fingerprint = boundary.content_fingerprint();
        boundary
    }

    #[must_use]
    pub fn edge(&self, edge: TerrainEdge) -> &[TerrainBoundaryVertex] {
        &self.edges[edge.index()]
    }

    #[must_use]
    pub fn resolution(&self) -> usize {
        self.edges[0].len()
    }

    /// Number of retained vertices, including the four duplicated corners.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.edges.iter().map(Vec::len).sum()
    }

    /// Allocated boundary storage in bytes, excluding any container holding it.
    #[must_use]
    pub fn memory_footprint(&self) -> usize {
        core::mem::size_of::<Self>()
            + self
                .edges
                .iter()
                .map(|edge| edge.capacity() * core::mem::size_of::<TerrainBoundaryVertex>())
                .sum::<usize>()
    }

    fn vertex(&self, edge: TerrainEdge, step: usize) -> SeamVertex {
        let vertex = self.edge(edge)[step];
        SeamVertex {
            position: self.origin.as_vec() + vector(vertex.position),
            normal: vector(vertex.normal),
            uv: vertex.uv.map(f64::from),
            elevation: f64::from(vertex.elevation),
            slope: f64::from(vertex.slope),
        }
    }

    fn sample(&self, edge: TerrainEdge, coordinate: f64) -> SeamVertex {
        let extent = EdgeExtent::new(self.id, edge);
        let steps = u32::try_from(self.resolution() - 1).expect("mesh resolution fits u32");
        let step = ((coordinate - f64::from(extent.start)) / f64::from(extent.end - extent.start)
            * f64::from(steps))
        .clamp(0.0, f64::from(steps));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = (step.floor() as usize).min(self.resolution() - 2);
        let fraction = step - f64::from(u32::try_from(index).expect("mesh index fits u32"));
        if fraction <= 0.0 {
            self.vertex(edge, index)
        } else if fraction >= 1.0 {
            self.vertex(edge, index + 1)
        } else {
            self.vertex(edge, index)
                .lerp(self.vertex(edge, index + 1), fraction)
        }
    }

    fn content_fingerprint(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for value in self.origin.as_vec().to_array() {
            feed(&value.to_bits().to_le_bytes());
        }
        for edge in &self.edges {
            for vertex in edge {
                for value in vertex
                    .position
                    .into_iter()
                    .chain(vertex.normal)
                    .chain(vertex.uv)
                    .chain([vertex.elevation, vertex.slope])
                {
                    feed(&value.to_bits().to_le_bytes());
                }
            }
        }
        hash
    }
}

/// Stable identity of one positive-length shared boundary.
///
/// `edge` is the edge of `first`. Keeping the edge in the key distinguishes the
/// two root tiles' prime-meridian seam from their dateline seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TerrainSeamKey {
    pub first: TileId,
    pub second: TileId,
    pub edge: TerrainEdge,
}

/// A planned ribbon. Equality includes source fingerprints and common corner
/// anchors: a third neighbour changing at a T-junction invalidates the cap too.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainSeam {
    key: TerrainSeamKey,
    start: u32,
    end: u32,
    anchors: [SeamVertex; 2],
    fingerprints: [u64; 2],
    pole_caps: Vec<PoleCap>,
    corner_patches: [Option<Arc<CornerPatch>>; 2],
}

impl TerrainSeam {
    #[must_use]
    pub const fn key(&self) -> TerrainSeamKey {
        self.key
    }

    #[must_use]
    pub const fn tiles(&self) -> [TileId; 2] {
        [self.key.first, self.key.second]
    }

    /// Upper bounds on `(vertices, indices)` for this ribbon, including both
    /// windings and any pole fans. Bounds depend only on retained resolutions.
    /// For default 33-vertex edges these are at most 138 vertices / 804 indices,
    /// or 266 / 1536 for a root seam owning all four polar fans.
    ///
    /// # Panics
    /// Panics if either source boundary is absent.
    #[must_use]
    pub fn mesh_size_bound(
        &self,
        boundaries: &BTreeMap<TileId, TerrainBoundary>,
    ) -> (usize, usize) {
        let knots = boundaries[&self.key.first].resolution()
            + boundaries[&self.key.second].resolution()
            - 2;
        let mut vertices = 2 * knots;
        let mut indices = 12 * (knots - 1);
        for patch in &self.corner_patches {
            vertices += patch.as_ref().map_or(1, |patch| patch.vertices.len() + 1);
            indices += patch.as_ref().map_or(6, |patch| patch.links.len() * 6);
        }
        for cap in &self.pole_caps {
            let resolution = boundaries[&cap.tile].resolution();
            vertices += resolution + 1;
            indices += 6 * (resolution - 1);
        }
        (vertices, indices)
    }

    /// Construct a two-sided ribbon and endpoint caps. Every polyline knot from
    /// either edge is retained, even when resolutions or LOD levels differ.
    ///
    /// The finer source tile supplies the local origin. Only final render data
    /// are reencoded; [`Self::encoding_error_bound`] quantifies that conversion.
    ///
    /// # Panics
    /// Panics if either source boundary is absent. Callers must retain the
    /// boundaries used by the plan until its budgeted construction completes.
    #[must_use]
    pub fn build_mesh(&self, boundaries: &BTreeMap<TileId, TerrainBoundary>) -> TerrainMesh {
        let first = &boundaries[&self.key.first];
        let second = &boundaries[&self.key.second];
        let origin = if first.id.level > second.id.level {
            first.origin
        } else {
            second.origin
        };
        let mut output = MeshBuilder::new(origin);
        let first_edge = self.key.edge;
        let second_edge = first_edge.opposite();
        let knots = merged_knots(first, first_edge, second, second_edge, self.start, self.end);
        for &coordinate in &knots {
            output.push(first.sample(first_edge, coordinate));
            output.push(second.sample(second_edge, coordinate));
        }
        for index in 0..knots.len() - 1 {
            let a = u32::try_from(index * 2).expect("seam vertices fit u32");
            output.triangle(a, a + 1, a + 2);
            output.triangle(a + 1, a + 3, a + 2);
        }
        let end = u32::try_from((knots.len() - 1) * 2).expect("seam vertices fit u32");
        for (endpoint, [a, b]) in [[1, 0], [end, end + 1]].into_iter().enumerate() {
            let anchor = output.push(self.anchors[endpoint]);
            if let Some(patch) = &self.corner_patches[endpoint] {
                // Complete the small ordinary junction fan in one local frame.
                // Sharing just one triangle per ribbon leaves centimetre-scale
                // cracks along fan spokes when neighbouring origins round them
                // differently. Repeated fans overlap harmlessly; a valid cut has
                // at most four incident tiles, so this is a fixed-size guard.
                let vertices: Vec<_> = patch.vertices.iter().map(|&v| output.push(v)).collect();
                for [first, second] in &patch.links {
                    output.triangle(vertices[*first], vertices[*second], anchor);
                }
            } else {
                output.triangle(a, b, anchor);
            }
        }
        for cap in &self.pole_caps {
            let boundary = &boundaries[&cap.tile];
            let anchor = output.push(cap.anchor);
            let mut previous = output.push(boundary.vertex(cap.edge, 0));
            for step in 1..boundary.resolution() {
                let next = output.push(boundary.vertex(cap.edge, step));
                output.triangle(previous, next, anchor);
                previous = next;
            }
        }
        output.finish()
    }

    /// Conservative Euclidean bound on reencoding one source point into this
    /// mesh's f32 local frame. Covers normal f32 rounding plus f64 cancellation;
    /// it does not include the renderer's subsequent floating-origin transform.
    #[must_use]
    pub fn encoding_error_bound(mesh: &TerrainMesh) -> Meters {
        let extent = mesh.positions.iter().fold(DVec3::ZERO, |extent, point| {
            extent.max(vector(*point).abs())
        });
        Meters(extent.length() * f64::from(f32::EPSILON) + 1.0e-8)
    }
}

#[derive(Debug, PartialEq)]
struct CornerPatch {
    vertices: Vec<SeamVertex>,
    links: Vec<[usize; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
struct PoleCap {
    tile: TileId,
    edge: TerrainEdge,
    anchor: SeamVertex,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeamVertex {
    position: DVec3,
    normal: DVec3,
    uv: [f64; 2],
    elevation: f64,
    slope: f64,
}

impl SeamVertex {
    fn lerp(self, other: Self, fraction: f64) -> Self {
        Self {
            position: self.position.lerp(other.position, fraction),
            normal: self.normal.lerp(other.normal, fraction),
            uv: std::array::from_fn(|index| {
                self.uv[index] + (other.uv[index] - self.uv[index]) * fraction
            }),
            elevation: self.elevation + (other.elevation - self.elevation) * fraction,
            slope: self.slope + (other.slope - self.slope) * fraction,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LineKey {
    vertical: bool,
    coordinate: u32,
}

#[derive(Debug, Clone, Copy)]
struct EdgeExtent {
    tile: TileId,
    edge: TerrainEdge,
    line: LineKey,
    start: u32,
    end: u32,
}

impl EdgeExtent {
    fn new(tile: TileId, edge: TerrainEdge) -> Self {
        let size = 1 << (MAX_LEVEL - tile.level);
        let x = tile.x * size;
        let y = tile.y * size;
        let (vertical, coordinate, start) = match edge {
            TerrainEdge::North => (false, y, x),
            TerrainEdge::East => (true, (x + size) % WORLD_WIDTH, y),
            TerrainEdge::South => (false, y + size, x),
            TerrainEdge::West => (true, x, y),
        };
        Self {
            tile,
            edge,
            line: LineKey {
                vertical,
                coordinate,
            },
            start,
            end: start + size,
        }
    }

    fn corner(self, coordinate: u32) -> CornerKey {
        let (x, y) = if self.line.vertical {
            (self.line.coordinate, coordinate)
        } else {
            (coordinate, self.line.coordinate)
        };
        CornerKey {
            x: if y == 0 || y == WORLD_HEIGHT {
                0
            } else {
                x % WORLD_WIDTH
            },
            y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CornerKey {
    x: u32,
    y: u32,
}

#[derive(Default)]
struct LineEdges {
    before: Vec<EdgeExtent>,
    after: Vec<EdgeExtent>,
}

/// Plan all adjacent visible tiles, including equal-LOD source discontinuities.
///
/// `visible` must be a non-overlapping quadtree cut. Missing boundaries are
/// omitted, permitting a caller to plan only its fully prepared candidates.
/// There are no float-coordinate adjacency tolerances and no pairwise tile scan.
#[must_use]
pub fn plan_seams(
    boundaries: &BTreeMap<TileId, TerrainBoundary>,
    visible: &BTreeSet<TileId>,
) -> Vec<TerrainSeam> {
    let mut lines = BTreeMap::<LineKey, LineEdges>::new();
    for &tile in visible {
        if !boundaries.contains_key(&tile) {
            continue;
        }
        for edge in TerrainEdge::ALL {
            let extent = EdgeExtent::new(tile, edge);
            let line = lines.entry(extent.line).or_default();
            if matches!(edge, TerrainEdge::East | TerrainEdge::South) {
                line.before.push(extent);
            } else {
                line.after.push(extent);
            }
        }
    }
    let mut pairs = Vec::new();
    for edges in lines.values_mut() {
        edges.before.sort_by_key(|edge| (edge.start, edge.end));
        edges.after.sort_by_key(|edge| (edge.start, edge.end));
        let (mut left, mut right) = (0, 0);
        while left < edges.before.len() && right < edges.after.len() {
            let a = edges.before[left];
            let b = edges.after[right];
            let start = a.start.max(b.start);
            let end = a.end.min(b.end);
            if start < end && a.tile != b.tile {
                let (first, second) = if a.tile < b.tile { (a, b) } else { (b, a) };
                pairs.push((first, second, start, end));
            }
            if a.end <= b.end {
                left += 1;
            }
            if b.end <= a.end {
                right += 1;
            }
        }
    }
    pairs.sort_by_key(|(first, second, _, _)| TerrainSeamKey {
        first: first.tile,
        second: second.tile,
        edge: first.edge,
    });

    // At a T-junction the coarse tile contributes its interpolated edge point,
    // not a resampled DEM point. Dedupe by tile at an ordinary corner so two
    // incidences of the same source point do not skew its shared anchor.
    let mut corners = BTreeMap::<CornerKey, BTreeMap<TileId, SeamVertex>>::new();
    let mut corner_links = BTreeMap::<CornerKey, BTreeSet<(TileId, TileId)>>::new();
    for &(first, second, start, end) in &pairs {
        for coordinate in [start, end] {
            let corner = first.corner(coordinate);
            let samples = corners.entry(corner).or_default();
            if corner.y != 0 && corner.y != WORLD_HEIGHT {
                corner_links
                    .entry(corner)
                    .or_default()
                    .insert((first.tile, second.tile));
            }
            for edge in [first, second] {
                samples.insert(
                    edge.tile,
                    boundaries[&edge.tile].sample(edge.edge, f64::from(coordinate)),
                );
            }
        }
    }
    let mut anchors: BTreeMap<_, _> = corners
        .iter()
        .map(|(key, samples)| (*key, mean_vertex(samples.values().copied())))
        .collect();

    // All longitudes at a pole have one key. Include every pole-edge vertex,
    // then close each tile's tiny post-f32 polar polygon exactly once.
    let mut polar_edges = BTreeMap::<CornerKey, Vec<(TileId, TerrainEdge)>>::new();
    for &tile in visible {
        if !boundaries.contains_key(&tile) {
            continue;
        }
        for edge in [TerrainEdge::North, TerrainEdge::South] {
            let extent = EdgeExtent::new(tile, edge);
            if extent.line.coordinate == 0 || extent.line.coordinate == WORLD_HEIGHT {
                polar_edges
                    .entry(extent.corner(extent.start))
                    .or_default()
                    .push((tile, edge));
            }
        }
    }
    for (key, edges) in &polar_edges {
        anchors.insert(
            *key,
            mean_vertex(edges.iter().flat_map(|&(tile, edge)| {
                let boundary = &boundaries[&tile];
                (0..boundary.resolution()).map(move |step| boundary.vertex(edge, step))
            })),
        );
    }
    let corner_patches: BTreeMap<_, _> = corner_links
        .into_iter()
        .map(|(key, links)| {
            let samples = &corners[&key];
            debug_assert!(
                samples.len() <= 4,
                "visible tiles must be a non-overlapping cut"
            );
            debug_assert!(
                links.len() <= 4,
                "ordinary corner has at most four incident edges"
            );
            let indices: BTreeMap<_, _> = samples
                .keys()
                .enumerate()
                .map(|(index, tile)| (*tile, index))
                .collect();
            (
                key,
                Arc::new(CornerPatch {
                    vertices: samples.values().copied().collect(),
                    links: links
                        .into_iter()
                        .map(|(first, second)| [indices[&first], indices[&second]])
                        .collect(),
                }),
            )
        })
        .collect();
    let mut assigned_caps = BTreeSet::new();
    pairs
        .into_iter()
        .map(|(first, second, start, end)| {
            let corner_keys = [first.corner(start), first.corner(end)];
            let mut pole_caps = Vec::new();
            for corner in corner_keys {
                if corner.y == 0 || corner.y == WORLD_HEIGHT {
                    let edge = if corner.y == 0 {
                        TerrainEdge::North
                    } else {
                        TerrainEdge::South
                    };
                    for tile in [first.tile, second.tile] {
                        if assigned_caps.insert((tile, edge)) {
                            pole_caps.push(PoleCap {
                                tile,
                                edge,
                                anchor: anchors[&corner],
                            });
                        }
                    }
                }
            }
            TerrainSeam {
                key: TerrainSeamKey {
                    first: first.tile,
                    second: second.tile,
                    edge: first.edge,
                },
                start,
                end,
                anchors: corner_keys.map(|key| anchors[&key]),
                fingerprints: [
                    boundaries[&first.tile].fingerprint,
                    boundaries[&second.tile].fingerprint,
                ],
                pole_caps,
                corner_patches: corner_keys.map(|key| corner_patches.get(&key).cloned()),
            }
        })
        .collect()
}

fn mean_vertex(vertices: impl Iterator<Item = SeamVertex>) -> SeamVertex {
    let mut vertices = vertices;
    let mut mean = vertices.next().expect("corner has incident vertices");
    let mut count = 1_u32;
    for vertex in vertices {
        count += 1;
        mean = mean.lerp(vertex, 1.0 / f64::from(count));
    }
    mean
}

fn merged_knots(
    first: &TerrainBoundary,
    first_edge: TerrainEdge,
    second: &TerrainBoundary,
    second_edge: TerrainEdge,
    start: u32,
    end: u32,
) -> Vec<f64> {
    let mut knots = vec![f64::from(start), f64::from(end)];
    for (boundary, edge) in [(first, first_edge), (second, second_edge)] {
        let extent = EdgeExtent::new(boundary.id, edge);
        let steps = u32::try_from(boundary.resolution() - 1).expect("mesh resolution fits u32");
        for step in 1..steps {
            let coordinate = f64::from(extent.start)
                + f64::from(extent.end - extent.start) * f64::from(step) / f64::from(steps);
            if coordinate > f64::from(start) && coordinate < f64::from(end) {
                knots.push(coordinate);
            }
        }
    }
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    knots
}

fn vector(value: [f32; 3]) -> DVec3 {
    DVec3::from_array(value.map(f64::from))
}

struct MeshBuilder {
    mesh: TerrainMesh,
}

impl MeshBuilder {
    fn new(origin: Ecef) -> Self {
        Self {
            mesh: TerrainMesh {
                origin,
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                elevations: Vec::new(),
                slopes: Vec::new(),
                indices: Vec::new(),
                surface_vertex_count: 0,
            },
        }
    }

    fn push(&mut self, vertex: SeamVertex) -> u32 {
        let index = u32::try_from(self.mesh.positions.len()).expect("seam vertices fit u32");
        let relative = vertex.position - self.mesh.origin.as_vec();
        let normal = vertex.normal.normalize_or_zero();
        #[allow(
            clippy::cast_possible_truncation,
            reason = "final render attributes use f32"
        )]
        {
            self.mesh
                .positions
                .push(relative.to_array().map(|x| x as f32));
            self.mesh.normals.push(normal.to_array().map(|x| x as f32));
            self.mesh.uvs.push(vertex.uv.map(|x| x as f32));
            self.mesh.elevations.push(vertex.elevation as f32);
            self.mesh.slopes.push(vertex.slope as f32);
        }
        index
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        let point = |index: u32| vector(self.mesh.positions[index as usize]);
        if (point(b) - point(a))
            .cross(point(c) - point(a))
            .length_squared()
            > 0.0
        {
            // Either edge can be higher and corner fans can fold. Both windings
            // are intentional: never depend on the terrain material's cull mode.
            self.mesh.indices.extend_from_slice(&[a, b, c, a, c, b]);
        }
    }

    fn finish(mut self) -> TerrainMesh {
        self.mesh.surface_vertex_count = self.mesh.positions.len();
        self.mesh
    }
}
