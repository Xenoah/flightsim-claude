//! Exact, render-only overlays on a frozen set of displayed terrain facets.
//!
//! Airport triangles are clipped at every terrain crease before being lifted.
//! This does not change the DEM, contact sampler, runway definition or replay.
//! All clipping uses f64 coordinates in a small local tangent frame. Only final
//! vertices are converted to the overlay mesh's original f32 ECEF-relative frame.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::Mesh;
use bevy::render::render_resource::VertexFormat;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Ned};
use glam::{DVec2, DVec3};
use std::sync::atomic::{AtomicBool, Ordering};

/// CPU-side authored layer height. It is intentionally not a shader input.
/// Capturing it avoids treating a taxiway's coarse source-node interpolation
/// error as an intended visual offset above terrain.
pub const ATTRIBUTE_OVERLAY_LIFT: MeshVertexAttribute =
    MeshVertexAttribute::new("OverlayLayerLift", 17_862_711_319, VertexFormat::Float32);

/// Maximum source mesh size accepted by the overlay boundary.
pub const MAX_OVERLAY_SOURCE_TRIANGLES: usize = 65_536;
/// Maximum facets intersecting one overlay's bounding rectangle.
pub const MAX_OVERLAY_TERRAIN_TRIANGLES: usize = 32_768;
/// Maximum output vertices, including splits at terrain creases.
pub const MAX_OVERLAY_OUTPUT_VERTICES: usize = 524_288;
/// Maximum narrow-phase intersection tests in one preparation.
pub const MAX_OVERLAY_INTERSECTIONS: usize = 4_000_000;
/// Ground detail supports facets up to 60 degrees from horizontal. Nearly
/// vertical seam walls close gaps but are not pavement or landcover floors;
/// omit those individual facets rather than hiding an entire ground batch.
pub const MIN_OVERLAY_SUPPORT_COSINE: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrapeError {
    InvalidMesh,
    SourceLimit,
    TerrainLimit,
    OutputLimit,
    WorkLimit,
    Cancelled,
    UpdatePending,
}

impl core::fmt::Display for DrapeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidMesh => "invalid terrain-overlay mesh",
            Self::SourceLimit => "terrain-overlay source limit exceeded",
            Self::TerrainLimit => "terrain-overlay candidate limit exceeded",
            Self::OutputLimit => "terrain-overlay output limit exceeded",
            Self::WorkLimit => "terrain-overlay intersection limit exceeded",
            Self::Cancelled => "terrain-overlay preparation cancelled",
            Self::UpdatePending => "an optional overlay generation is still awaiting commit",
        })
    }
}
impl std::error::Error for DrapeError {}

/// Surface geometry already encoded for the GPU. Skirts must be excluded.
#[derive(Debug, Clone)]
pub(crate) struct DrapeTerrain {
    pub origin: Ecef,
    pub footprint: Option<[flightsim_world::TileId; 2]>,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct OverlayPrecision {
    pub terrain_sources: Vec<usize>,
    pub support_cosine: f64,
    pub output_extent: f64,
    pub authored_displacement: Meters,
    pub omitted_support_facets: usize,
}
#[derive(Debug)]
pub(crate) struct DrapedOverlay {
    pub mesh: Mesh,
    pub precision: OverlayPrecision,
}
struct DrapeMetrics {
    used: Vec<bool>,
    support_cosine: f64,
    authored_displacement: f64,
}
impl DrapeMetrics {
    fn include(&mut self, facet: &Facet) {
        self.used[facet.source] = true;
        self.support_cosine = self.support_cosine.min(facet.support_cosine);
    }
}

#[derive(Debug, Clone, Copy)]
struct Vertex {
    point: DVec3,
    lift: f64,
    normal: DVec3,
    color: [f32; 4],
}

/// Immutable footprint and small layer offsets, independent of any terrain LOD.
#[derive(Debug, Clone)]
pub struct TerrainOverlay {
    pub(crate) origin: Ecef,
    pub(crate) radius: Meters,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    fixture_vertices: Option<usize>,
}

impl TerrainOverlay {
    /// Capture a pavement/paint mesh and its original above-ground layer offsets.
    /// Authored layer metadata is preferred. Without it, the callback uses the
    /// same physical source originally used to author the mesh.
    /// It is called only at registration, never in terrain preparation or physics.
    ///
    /// # Errors
    /// Invalid attributes, nonfinite coordinates/heights, excessive dimensions,
    /// source triangles, or offsets outside the deliberately small -1..=2 m band.
    pub fn surface(
        mesh: &Mesh,
        origin: Ecef,
        mut elevation: impl FnMut(Geodetic) -> Meters,
    ) -> Result<Self, DrapeError> {
        Self::capture(mesh, origin, None, &mut elevation)
    }

    /// Capture identical contiguous fixture blocks, preserving their solid shape.
    /// Each block is placed above the highest intersecting terrain point beneath
    /// its footprint. This handles creases through a lamp, not just its corners.
    ///
    /// # Errors
    /// As for [`Self::surface`], or incomplete/oversized fixture blocks.
    pub fn fixtures(
        mesh: &Mesh,
        origin: Ecef,
        vertices_per_fixture: usize,
        mut elevation: impl FnMut(Geodetic) -> Meters,
    ) -> Result<Self, DrapeError> {
        if !(3..=64).contains(&vertices_per_fixture) {
            return Err(DrapeError::InvalidMesh);
        }
        Self::capture(mesh, origin, Some(vertices_per_fixture), &mut elevation)
    }

    fn capture(
        mesh: &Mesh,
        origin: Ecef,
        fixture_vertices: Option<usize>,
        elevation: &mut impl FnMut(Geodetic) -> Meters,
    ) -> Result<Self, DrapeError> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err(DrapeError::InvalidMesh);
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            return Err(DrapeError::InvalidMesh);
        };
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(colors)) if colors.len() == positions.len() => {
                Some(colors)
            }
            None => None,
            _ => return Err(DrapeError::InvalidMesh),
        };
        let authored_lifts = match mesh.attribute(ATTRIBUTE_OVERLAY_LIFT) {
            Some(VertexAttributeValues::Float32(values)) if values.len() == positions.len() => {
                Some(values)
            }
            None => None,
            _ => return Err(DrapeError::InvalidMesh),
        };
        let Some(indices) = mesh.indices() else {
            return Err(DrapeError::InvalidMesh);
        };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList
            || indices.len() % 3 != 0
            || normals.len() != positions.len()
            || !origin.as_vec().is_finite()
            || fixture_vertices.is_some_and(|count| positions.len() % count != 0)
        {
            return Err(DrapeError::InvalidMesh);
        }
        if indices.len() / 3 > MAX_OVERLAY_SOURCE_TRIANGLES
            || positions.len() > MAX_OVERLAY_OUTPUT_VERTICES
        {
            return Err(DrapeError::SourceLimit);
        }
        let indices: Vec<_> = indices
            .iter()
            .map(|index| u32::try_from(index).map_err(|_| DrapeError::InvalidMesh))
            .collect::<Result<_, _>>()?;
        if indices
            .iter()
            .any(|&index| index as usize >= positions.len())
        {
            return Err(DrapeError::InvalidMesh);
        }
        let frame = LocalFrame::new(origin.to_geodetic());
        let mut radius: f64 = 0.0;
        let vertices = positions
            .iter()
            .zip(normals)
            .enumerate()
            .map(|(index, (position, normal))| {
                let world = Ecef(origin.as_vec() + vector(*position));
                let geo = world.to_geodetic();
                let point = local_point(&frame, world);
                let lift = authored_lifts.map_or_else(
                    || geo.altitude.get() - elevation(geo).get(),
                    |values| f64::from(values[index]),
                );
                if !point.is_finite()
                    || !lift.is_finite()
                    || !(-1.0..=2.0).contains(&lift)
                    || !vector(*normal).is_finite()
                    || point.length() > 40_000.0
                {
                    return Err(DrapeError::InvalidMesh);
                }
                radius = radius.max(point.truncate().length());
                Ok(Vertex {
                    point,
                    lift,
                    normal: vector(*normal),
                    color: colors.map_or([1.0; 4], |values| values[index]),
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            origin,
            radius: Meters(radius + 1.0),
            vertices,
            indices,
            fixture_vertices,
        })
    }

    pub(crate) fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub(crate) fn minimum_lift(&self) -> f64 {
        self.vertices
            .iter()
            .map(|vertex| vertex.lift)
            .fold(f64::INFINITY, f64::min)
    }

    pub(crate) fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub(crate) fn intersects_tile(&self, id: flightsim_world::TileId) -> bool {
        let geo = self.origin.to_geodetic();
        let ground = Geodetic::new(geo.latitude, geo.longitude, Meters::ZERO);
        flightsim_world::lod::distance_to_bounds(ground.to_ecef(), ground, id.bounds()).get()
            <= self.radius.get() + 2.0
    }

    #[cfg(test)]
    fn drape(&self, terrain: &[DrapeTerrain]) -> Result<Mesh, DrapeError> {
        self.drape_cancellable(
            terrain,
            &AtomicBool::new(false),
            &mut 0,
            MAX_OVERLAY_OUTPUT_VERTICES,
        )
        .map(|output| output.mesh)
    }

    pub(crate) fn drape_cancellable(
        &self,
        terrain: &[DrapeTerrain],
        cancelled: &AtomicBool,
        work: &mut usize,
        output_limit: usize,
    ) -> Result<DrapedOverlay, DrapeError> {
        let frame = LocalFrame::new(self.origin.to_geodetic());
        let footprint = Bounds::of(self.vertices.iter().map(|vertex| vertex.point.truncate()));
        let mut facets = Vec::new();
        let mut omitted_support_facets = 0;
        for (source, tile) in terrain.iter().enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                return Err(DrapeError::Cancelled);
            }
            if tile
                .footprint
                .is_some_and(|ids| !ids.into_iter().any(|id| self.intersects_tile(id)))
            {
                continue;
            }
            for indices in tile.indices.chunks_exact(3) {
                charge(work)?;
                let vertices = [indices[0], indices[1], indices[2]].map(|index| {
                    local_point(
                        &frame,
                        Ecef(tile.origin.as_vec() + vector(tile.positions[index as usize])),
                    )
                });
                let facet = Facet::new(vertices, source);
                if facet.area < -1.0e-10 && facet.bounds.intersects(footprint) {
                    if facet.support_cosine < MIN_OVERLAY_SUPPORT_COSINE {
                        omitted_support_facets += 1;
                        continue;
                    }
                    if facets.len() == MAX_OVERLAY_TERRAIN_TRIANGLES {
                        return Err(DrapeError::TerrainLimit);
                    }
                    facets.push(facet);
                }
            }
        }
        let index = FacetIndex::new(&facets);
        let mut output = Output {
            limit: output_limit.min(MAX_OVERLAY_OUTPUT_VERTICES),
            ..Default::default()
        };
        let mut metrics = DrapeMetrics {
            used: vec![false; terrain.len()],
            support_cosine: 1.0,
            authored_displacement: 0.0,
        };
        if let Some(count) = self.fixture_vertices {
            self.drape_fixtures(
                count,
                &frame,
                &facets,
                &index,
                &mut output,
                work,
                cancelled,
                &mut metrics,
            )?;
        } else {
            for indices in self.indices.chunks_exact(3) {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(DrapeError::Cancelled);
                }
                let source = [
                    self.vertices[indices[0] as usize],
                    self.vertices[indices[1] as usize],
                    self.vertices[indices[2] as usize],
                ];
                let source_xy = source.map(|vertex| vertex.point.truncate());
                let source_area = cross(source_xy[1] - source_xy[0], source_xy[2] - source_xy[0]);
                if source_area.abs() <= 1.0e-10 {
                    continue;
                }
                let bounds = Bounds::of(source_xy);
                let mut candidates = Vec::new();
                index.query(bounds, &mut candidates);
                for candidate in candidates {
                    charge(work)?;
                    let facet = &facets[candidate];
                    let polygon = intersection(&source_xy, &facet.xy());
                    for corner in 1..polygon.len().saturating_sub(1) {
                        let points = [polygon[0], polygon[corner], polygon[corner + 1]];
                        if cross(points[1] - points[0], points[2] - points[0]).abs() <= 1.0e-10 {
                            continue;
                        }
                        metrics.include(facet);
                        for point in points {
                            let weights = barycentric(point, source_xy, source_area);
                            let lift = weights
                                .iter()
                                .zip(source)
                                .map(|(weight, vertex)| weight * vertex.lift)
                                .sum::<f64>();
                            let authored_base = weights
                                .iter()
                                .zip(source)
                                .map(|(weight, vertex)| weight * (vertex.point.z - vertex.lift))
                                .sum::<f64>();
                            metrics.authored_displacement = metrics
                                .authored_displacement
                                .max((facet.height(point) - authored_base).abs());
                            let normal = weights
                                .iter()
                                .zip(source)
                                .map(|(weight, vertex)| weight * vertex.normal)
                                .sum::<DVec3>()
                                .normalize_or_zero();
                            let color = std::array::from_fn(|channel| {
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    reason = "interpolated finite colour channel"
                                )]
                                {
                                    weights
                                        .iter()
                                        .zip(source)
                                        .map(|(weight, vertex)| {
                                            weight * f64::from(vertex.color[channel])
                                        })
                                        .sum::<f64>() as f32
                                }
                            });
                            output.push(
                                world_relative(
                                    &frame,
                                    DVec3::new(point.x, point.y, facet.height(point) + lift),
                                    self.origin,
                                ),
                                normal,
                                color,
                            )?;
                        }
                    }
                }
            }
        }
        let precision = OverlayPrecision {
            terrain_sources: metrics
                .used
                .iter()
                .enumerate()
                .filter_map(|(index, &used)| used.then_some(index))
                .collect(),
            support_cosine: metrics.support_cosine,
            output_extent: output.extent,
            authored_displacement: Meters(metrics.authored_displacement),
            omitted_support_facets,
        };
        Ok(DrapedOverlay {
            mesh: output.mesh(),
            precision,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one frozen clipping context plus cancellation and output budgets"
    )]
    fn drape_fixtures(
        &self,
        count: usize,
        frame: &LocalFrame,
        facets: &[Facet],
        index: &FacetIndex,
        output: &mut Output,
        work: &mut usize,
        cancelled: &AtomicBool,
        metrics: &mut DrapeMetrics,
    ) -> Result<(), DrapeError> {
        let mut shifts = Vec::with_capacity(self.vertices.len() / count);
        for vertices in self.vertices.chunks_exact(count) {
            if cancelled.load(Ordering::Relaxed) {
                return Err(DrapeError::Cancelled);
            }
            let bounds = Bounds::of(vertices.iter().map(|vertex| vertex.point.truncate()));
            let rectangle = [
                bounds.min,
                DVec2::new(bounds.max.x, bounds.min.y),
                bounds.max,
                DVec2::new(bounds.min.x, bounds.max.y),
            ];
            let mut candidates = Vec::new();
            index.query(bounds, &mut candidates);
            let mut highest = f64::NEG_INFINITY;
            for candidate in candidates {
                charge(work)?;
                let facet = &facets[candidate];
                let polygon = intersection(&rectangle, &facet.xy());
                if !polygon.is_empty() {
                    metrics.include(facet);
                }
                for point in polygon {
                    highest = highest.max(facet.height(point));
                }
            }
            if !highest.is_finite() {
                shifts.push(None);
                continue;
            }
            let bottom = vertices
                .iter()
                .map(|vertex| vertex.point.z)
                .fold(f64::INFINITY, f64::min);
            let lift = vertices
                .iter()
                .map(|vertex| vertex.lift)
                .fold(f64::INFINITY, f64::min)
                .max(0.0);
            let shift = highest + lift - bottom;
            metrics.authored_displacement = metrics.authored_displacement.max(shift.abs());
            shifts.push(Some(shift));
        }
        for triangle in self.indices.chunks_exact(3) {
            let block = triangle[0] as usize / count;
            if triangle
                .iter()
                .any(|&index| index as usize / count != block)
            {
                return Err(DrapeError::InvalidMesh);
            }
            let Some(shift) = shifts[block] else { continue };
            for &index in triangle {
                let vertex = self.vertices[index as usize];
                output.push(
                    world_relative(frame, vertex.point + DVec3::Z * shift, self.origin),
                    vertex.normal,
                    vertex.color,
                )?;
            }
        }
        Ok(())
    }
}

fn charge(work: &mut usize) -> Result<(), DrapeError> {
    *work += 1;
    if *work > MAX_OVERLAY_INTERSECTIONS {
        Err(DrapeError::WorkLimit)
    } else {
        Ok(())
    }
}
fn vector(value: [f32; 3]) -> DVec3 {
    DVec3::from_array(value.map(f64::from))
}
fn local_point(frame: &LocalFrame, world: Ecef) -> DVec3 {
    let ned = frame.ecef_to_ned_position(world).0;
    DVec3::new(ned.x, ned.y, -ned.z)
}
fn world_relative(frame: &LocalFrame, point: DVec3, origin: Ecef) -> DVec3 {
    frame
        .ned_to_ecef_position(Ned(DVec3::new(point.x, point.y, -point.z)))
        .as_vec()
        - origin.as_vec()
}
fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn barycentric(point: DVec2, triangle: [DVec2; 3], area: f64) -> [f64; 3] {
    let b = cross(point - triangle[0], triangle[2] - triangle[0]) / area;
    let c = cross(triangle[1] - triangle[0], point - triangle[0]) / area;
    [1.0 - b - c, b, c]
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: DVec2,
    max: DVec2,
}
impl Bounds {
    fn of(points: impl IntoIterator<Item = DVec2>) -> Self {
        let mut result = Self {
            min: DVec2::splat(f64::INFINITY),
            max: DVec2::splat(f64::NEG_INFINITY),
        };
        for point in points {
            result.min = result.min.min(point);
            result.max = result.max.max(point);
        }
        result
    }
    fn intersects(self, other: Self) -> bool {
        self.min.cmple(other.max).all() && other.min.cmple(self.max).all()
    }
    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}
#[derive(Debug)]
struct Facet {
    vertices: [DVec3; 3],
    bounds: Bounds,
    area: f64,
    source: usize,
    support_cosine: f64,
}
impl Facet {
    fn new(vertices: [DVec3; 3], source: usize) -> Self {
        let xy = vertices.map(DVec3::truncate);
        let face = (vertices[1] - vertices[0]).cross(vertices[2] - vertices[0]);
        Self {
            vertices,
            bounds: Bounds::of(xy),
            area: cross(xy[1] - xy[0], xy[2] - xy[0]),
            source,
            support_cosine: (-face.z / face.length()).clamp(0.0, 1.0),
        }
    }
    fn xy(&self) -> [DVec2; 3] {
        self.vertices.map(DVec3::truncate)
    }
    fn height(&self, point: DVec2) -> f64 {
        barycentric(point, self.xy(), self.area)
            .iter()
            .zip(self.vertices)
            .map(|(weight, vertex)| weight * vertex.z)
            .sum()
    }
}

#[derive(Debug)]
enum FacetIndex {
    Empty,
    Leaf {
        bounds: Bounds,
        entries: Vec<usize>,
    },
    Branch {
        bounds: Bounds,
        children: Box<[Self; 2]>,
    },
}
impl FacetIndex {
    fn new(facets: &[Facet]) -> Self {
        Self::build(facets, (0..facets.len()).collect())
    }
    fn build(facets: &[Facet], mut entries: Vec<usize>) -> Self {
        let Some(&first) = entries.first() else {
            return Self::Empty;
        };
        let bounds = entries
            .iter()
            .skip(1)
            .fold(facets[first].bounds, |bounds, &index| {
                bounds.union(facets[index].bounds)
            });
        if entries.len() <= 8 {
            return Self::Leaf { bounds, entries };
        }
        let axis = usize::from((bounds.max - bounds.min).y > (bounds.max - bounds.min).x);
        entries.sort_by(|&a, &b| {
            (facets[a].bounds.min[axis] + facets[a].bounds.max[axis])
                .total_cmp(&(facets[b].bounds.min[axis] + facets[b].bounds.max[axis]))
                .then(a.cmp(&b))
        });
        let right = entries.split_off(entries.len() / 2);
        Self::Branch {
            bounds,
            children: Box::new([Self::build(facets, entries), Self::build(facets, right)]),
        }
    }
    fn query(&self, query: Bounds, output: &mut Vec<usize>) {
        match self {
            Self::Empty => {}
            Self::Leaf { bounds, entries } if bounds.intersects(query) => output.extend(entries),
            Self::Branch { bounds, children } if bounds.intersects(query) => {
                children[0].query(query, output);
                children[1].query(query, output);
            }
            _ => {}
        }
    }
}

/// Sutherland-Hodgman clipping; either winding is accepted.
fn intersection(subject: &[DVec2], clip: &[DVec2; 3]) -> Vec<DVec2> {
    let mut polygon = subject.to_vec();
    let orientation = cross(clip[1] - clip[0], clip[2] - clip[0]).signum();
    for edge in 0..3 {
        let a = clip[edge];
        let direction = clip[(edge + 1) % 3] - a;
        let input = std::mem::take(&mut polygon);
        let Some(&last) = input.last() else { break };
        let mut previous = last;
        let mut previous_distance = orientation * cross(direction, previous - a);
        for point in input {
            let distance = orientation * cross(direction, point - a);
            if (distance >= 0.0) != (previous_distance >= 0.0) {
                let fraction = previous_distance / (previous_distance - distance);
                polygon.push(previous.lerp(point, fraction.clamp(0.0, 1.0)));
            }
            if distance >= 0.0 {
                polygon.push(point);
            }
            previous = point;
            previous_distance = distance;
        }
    }
    polygon
}

#[derive(Default)]
struct Output {
    limit: usize,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    extent: f64,
}
impl Output {
    fn push(&mut self, point: DVec3, normal: DVec3, color: [f32; 4]) -> Result<(), DrapeError> {
        if self.positions.len() >= self.limit {
            return Err(DrapeError::OutputLimit);
        }
        if !point.is_finite() || !normal.is_finite() || color.iter().any(|value| !value.is_finite())
        {
            return Err(DrapeError::InvalidMesh);
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "validated airport-relative coordinates and bounded normal components"
        )]
        {
            self.positions
                .push(point.to_array().map(|value| value as f32));
            self.normals
                .push(normal.to_array().map(|value| value as f32));
        }
        self.colors.push(color);
        self.extent = self.extent.max(point.length());
        Ok(())
    }
    fn mesh(self) -> Mesh {
        let count = u32::try_from(self.positions.len()).expect("bounded overlay vertices");
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32((0..count).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> Geodetic {
        Geodetic::from_degrees(35.55, 139.78, 40.0)
    }

    fn overlay_at(origin: Geodetic, lift: f64) -> TerrainOverlay {
        let up = LocalFrame::new(origin).up_ecef();
        let vertices = [
            [0.0, 0.0, 0.0],
            [0.0, 100.0, 100.0],
            [100.0, 0.0, 100.0],
            [100.0, 100.0, 0.0],
        ]
        .map(|point| Vertex {
            point: DVec3::from_array(point) + DVec3::Z * lift,
            lift,
            normal: up,
            color: [0.2, 0.3, 0.4, 1.0],
        })
        .to_vec();
        TerrainOverlay {
            origin: origin.to_ecef(),
            radius: Meters(150.0),
            vertices,
            indices: vec![0, 1, 3, 0, 3, 2],
            fixture_vertices: None,
        }
    }

    fn terrain_at(origin: Geodetic) -> DrapeTerrain {
        let frame = LocalFrame::new(origin);
        let positions = [
            [0.0, 0.0, 0.0],
            [0.0, 100.0, 100.0],
            [100.0, 0.0, 100.0],
            [100.0, 100.0, 0.0],
        ]
        .map(|point| {
            let relative = world_relative(&frame, DVec3::from_array(point), origin.to_ecef());
            #[allow(
                clippy::cast_possible_truncation,
                reason = "small test mesh reproduces actual f32 GPU encoding"
            )]
            relative.to_array().map(|value| value as f32)
        })
        .to_vec();
        DrapeTerrain {
            origin: origin.to_ecef(),
            footprint: None,
            positions,
            indices: vec![0, 1, 2, 1, 3, 2],
        }
    }

    fn local_vertices(mesh: &Mesh, origin: Geodetic) -> Vec<DVec3> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        let frame = LocalFrame::new(origin);
        positions
            .iter()
            .map(|position| {
                local_point(&frame, Ecef(origin.to_ecef().as_vec() + vector(*position)))
            })
            .collect()
    }

    #[test]
    fn saddle_is_clipped_at_the_crease_and_clear_across_whole_fragments() {
        let origin = reference();
        let terrain = terrain_at(origin);
        let overlay = overlay_at(origin, 0.08);
        // Bilinear physical height is 50 m at the centre, while the actual
        // NE/SW-rendered diagonal is 100 m: dense point sampling alone is not proof.
        let bilinear = flightsim_world::HeightGrid::new(2, 2, vec![0.0, 100.0, 100.0, 0.0]);
        assert!((bilinear.sample_normalised(0.5, 0.5).get() - 50.0).abs() < 1e-12);
        let output = overlay.drape(&[terrain]).unwrap();
        let points = local_vertices(&output, origin);
        assert!(points.len() >= 12, "opposing diagonals must split");
        let mut area = 0.0;
        for triangle in points.chunks_exact(3) {
            area += cross(
                triangle[1].truncate() - triangle[0].truncate(),
                triangle[2].truncate() - triangle[0].truncate(),
            )
            .abs()
                * 0.5;
            for weights in [
                [1.0 / 3.0; 3],
                [0.8, 0.1, 0.1],
                [0.1, 0.8, 0.1],
                [0.1, 0.1, 0.8],
            ] {
                let point = triangle
                    .iter()
                    .zip(weights)
                    .map(|(point, weight)| point * weight)
                    .sum::<DVec3>();
                let surface = 100.0 - (point.x + point.y - 100.0).abs();
                assert!(
                    (point.z - surface - 0.08).abs() < 0.0001,
                    "clearance at {point:?}: {}",
                    point.z - surface
                );
            }
        }
        assert!(
            (area - 10_000.0).abs() < 0.02,
            "no missing or duplicate footprint area: {area}"
        );
    }

    #[test]
    fn paint_layer_stays_above_pavement_after_clipping() {
        let origin = reference();
        let terrain = [terrain_at(origin)];
        let pavement = local_vertices(&overlay_at(origin, 0.08).drape(&terrain).unwrap(), origin);
        let paint = local_vertices(&overlay_at(origin, 0.13).drape(&terrain).unwrap(), origin);
        assert_eq!(pavement.len(), paint.len());
        for (a, b) in pavement.iter().zip(paint) {
            assert!((b.z - a.z - 0.05).abs() < 0.0001);
        }
    }

    #[test]
    fn mesh_encoding_is_stable_at_dateline_high_latitude_and_rebased_origins() {
        for origin in [
            reference(),
            Geodetic::from_degrees(0.0, 179.9999, -50.0),
            Geodetic::from_degrees(85.0, -179.9999, 3200.0),
        ] {
            let output = overlay_at(origin, 0.08)
                .drape(&[terrain_at(origin)])
                .unwrap();
            for point in local_vertices(&output, origin) {
                let surface = 100.0 - (point.x + point.y - 100.0).abs();
                assert!((point.z - surface - 0.08).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn double_wound_bridge_facets_are_only_draped_once() {
        let origin = reference();
        let mut terrain = terrain_at(origin);
        terrain.indices.extend_from_slice(&[2, 1, 0, 2, 3, 1]);
        let double = overlay_at(origin, 0.08).drape(&[terrain]).unwrap();
        let single = overlay_at(origin, 0.08)
            .drape(&[terrain_at(origin)])
            .unwrap();
        assert_eq!(
            double.attribute(Mesh::ATTRIBUTE_POSITION),
            single.attribute(Mesh::ATTRIBUTE_POSITION)
        );
    }

    #[test]
    fn a_fixture_is_supported_above_the_highest_intersecting_crease() {
        let origin = reference();
        let mut overlay = overlay_at(origin, 0.12);
        for vertex in &mut overlay.vertices {
            vertex.point.z = 0.12;
        }
        overlay.fixture_vertices = Some(4);
        let output = overlay.drape(&[terrain_at(origin)]).unwrap();
        for point in local_vertices(&output, origin) {
            assert!((point.z - 100.12).abs() < 0.0001);
        }
    }

    #[test]
    fn output_is_repeatable_and_no_coverage_yields_an_empty_mesh() {
        let source = overlay_at(reference(), 0.08);
        let terrain = [terrain_at(reference())];
        let a = source.drape(&terrain).unwrap();
        let b = source.drape(&terrain).unwrap();
        assert_eq!(
            a.attribute(Mesh::ATTRIBUTE_POSITION),
            b.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        assert_eq!(source.drape(&[]).unwrap().count_vertices(), 0);
    }

    #[test]
    fn real_runway_capture_preserves_only_the_small_authored_lifts() {
        let threshold = reference();
        let (mesh, origin) = crate::runway::runway_mesh(
            threshold,
            flightsim_core::Radians::ZERO,
            Meters(500.0),
            Meters(30.0),
        );
        let source = TerrainOverlay::surface(&mesh, origin, |_| threshold.altitude).unwrap();
        assert!(source.vertices.iter().all(
            |vertex| (vertex.lift - 0.08).abs() < 0.0001 || (vertex.lift - 0.13).abs() < 0.0001
        ));
        let mut legacy = mesh.clone();
        legacy.remove_attribute(ATTRIBUTE_OVERLAY_LIFT);
        assert!(TerrainOverlay::surface(&legacy, origin, |_| Meters(f64::NAN)).is_err());
        assert!(TerrainOverlay::surface(&legacy, origin, |_| Meters(-100.0)).is_err());
        assert!(
            TerrainOverlay::surface(&mesh, origin, |_| panic!(
                "authored lift requires no terrain query"
            ))
            .is_ok()
        );
        assert!(TerrainOverlay::fixtures(&mesh, origin, 0, |_| threshold.altitude).is_err());
    }

    #[test]
    fn clipping_handles_both_windings_and_touching_edges() {
        let triangle = [
            DVec2::new(0.0, 0.0),
            DVec2::new(0.0, 1.0),
            DVec2::new(1.0, 0.0),
        ];
        let reversed = [triangle[2], triangle[1], triangle[0]];
        for clip in [triangle, reversed] {
            assert_eq!(intersection(&triangle, &clip).len(), 3);
            assert!(
                intersection(
                    &[
                        DVec2::new(2.0, 2.0),
                        DVec2::new(3.0, 2.0),
                        DVec2::new(2.0, 3.0)
                    ],
                    &clip
                )
                .is_empty()
            );
        }
    }
    #[test]
    fn precision_metadata_uses_only_intersected_facets_and_their_actual_slope() {
        let origin = reference();
        let terrain = terrain_at(origin);
        let mut unrelated = terrain_at(origin);
        unrelated.origin = origin.offset_by(Meters(10_000.0), Meters::ZERO).to_ecef();
        let output = overlay_at(origin, 0.08)
            .drape_cancellable(
                &[terrain, unrelated],
                &AtomicBool::new(false),
                &mut 0,
                MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap();
        assert_eq!(output.precision.terrain_sources, vec![0]);
        assert!((output.precision.support_cosine - 1.0 / 3.0_f64.sqrt()).abs() < 1e-6);
        assert!(output.precision.output_extent > 100.0);
    }
    #[test]
    fn a_nearly_vertical_seam_cannot_hide_the_rest_of_a_ground_batch() {
        let origin = reference();
        let frame = LocalFrame::new(origin);
        let mut wall = terrain_at(origin);
        let points = [
            [49.999, 0.0, 0.0],
            [49.999, 100.0, 0.0],
            [50.001, 0.0, 100.0],
            [50.001, 100.0, 100.0],
        ];
        #[allow(
            clippy::cast_possible_truncation,
            reason = "small test mesh reproduces actual f32 GPU encoding"
        )]
        {
            wall.positions = points
                .map(|point| {
                    world_relative(&frame, DVec3::from_array(point), origin.to_ecef())
                        .to_array()
                        .map(|value| value as f32)
                })
                .to_vec();
        }
        let source = overlay_at(origin, 0.055);
        let output = source
            .drape_cancellable(
                &[terrain_at(origin), wall.clone()],
                &AtomicBool::new(false),
                &mut 0,
                MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap();
        let baseline = source.drape(&[terrain_at(origin)]).unwrap();
        assert_eq!(
            output.mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            baseline.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        assert_eq!(output.precision.terrain_sources, vec![0]);
        assert!(output.precision.support_cosine >= MIN_OVERLAY_SUPPORT_COSINE);
        assert_eq!(output.precision.omitted_support_facets, 2);
        let unsupported = source
            .drape_cancellable(
                &[wall],
                &AtomicBool::new(false),
                &mut 0,
                MAX_OVERLAY_OUTPUT_VERTICES,
            )
            .unwrap();
        assert_eq!(
            unsupported.mesh.count_vertices(),
            0,
            "unsupported walls cannot invent a ground floor"
        );
        assert_eq!(unsupported.precision.omitted_support_facets, 2);
    }
}
