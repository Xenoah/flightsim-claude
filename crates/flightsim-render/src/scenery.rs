//! Bounded, batched depiction of an offline regional scenery database.
//!
//! Footprints and road centrelines remain the source geography. Facade colours,
//! windows, flat roofs and individual trees are original procedural depiction,
//! not surveyed architecture, obstacles, traffic or navigation data. This module
//! never modifies the DEM, contact sampler, aircraft state or replay.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Ned};
use flightsim_world::scenery::{
    LandCoverClass, RoadClass, SceneryBuilding, SceneryFeatureRef, SceneryLandCover, SceneryRoad,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Maximum GPU vertices in one scenery batch, including original facade detail.
pub const MAX_BATCH_VERTICES: usize = 65_532;
/// Hard cap on procedural trees in one batch, independent of polygon size.
pub const MAX_BATCH_TREES: usize = 128;
/// Includes rejected candidates, preventing dense exclusions from multiplying work.
pub const MAX_BATCH_TREE_CANDIDATES: usize = 512;
/// Runtime selection divides work into batches no larger than this bound.
pub const MAX_BATCH_FEATURES: usize = 64;
/// Features exceeding these local display bounds are counted and skipped.
const MAX_ROAD_STEPS: usize = 512;
const MAX_LAND_TRIANGLES: usize = 1_024;
/// Render-only shallow embedment for metre-scale visible-triangle/physical
/// bilinear disagreement. Extends only wall/trunk bottoms; crowns, roofs, DEM
/// samples and contact remain unchanged. It is not a global clearance guarantee.
const FOUNDATION_DEPTH: Meters = Meters(2.0);

/// Counts describe prepared geometry, including explicitly skipped work. A later
/// terrain transaction may omit bounded optional ground batches from visibility.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SceneryMeshStatistics {
    pub roads: usize,
    pub buildings: usize,
    pub land_polygons: usize,
    pub procedural_trees: usize,
    pub tree_candidates: usize,
    pub skipped_features: usize,
    pub vertices: usize,
    pub triangles: usize,
}

/// Deliberately limited depiction options. Altitude and coordinates stay in core units.
#[derive(Debug, Clone, Copy)]
pub struct SceneryMeshOptions<'a> {
    pub facades: bool,
    pub vegetation: bool,
    pub observer: Geodetic,
    pub vegetation_distance: Meters,
    pub max_vertices: usize,
    pub max_ground_vertices: usize,
    pub max_trees: usize,
    pub max_tree_candidates: usize,
    /// Optional cooperative cancellation for relocation/background preparation.
    pub cancellation: Option<&'a AtomicBool>,
    pub airport_ground_exclusions: Option<&'a crate::scenery_exclusions::GroundSceneryExclusions>,
}

impl SceneryMeshOptions<'_> {
    #[must_use]
    pub fn near(observer: Geodetic) -> Self {
        Self {
            facades: true,
            vegetation: true,
            observer,
            vegetation_distance: Meters(1_800.0),
            max_vertices: MAX_BATCH_VERTICES,
            max_ground_vertices: MAX_BATCH_VERTICES,
            max_trees: MAX_BATCH_TREES,
            max_tree_candidates: MAX_BATCH_TREE_CANDIDATES,
            cancellation: None,
            airport_ground_exclusions: None,
        }
    }
}

/// One batch has one origin, mesh and shared material, rather than an entity per tree.
#[derive(Debug)]
pub struct SceneryMesh {
    pub origin: Ecef,
    /// Solid buildings/trees; may be empty when this batch contains ground only.
    pub mesh: Mesh,
    pub ground: Option<SceneryGroundMesh>,
    pub statistics: SceneryMeshStatistics,
}

/// Ground triangles remain separate so the app can bind them to the actual
/// displayed terrain cut without deforming building walls or trees.
#[derive(Debug)]
pub struct SceneryGroundMesh {
    pub mesh: Mesh,
    /// Authored small metre offsets, excluding source DEM interpolation error.
    pub lifts: Vec<f32>,
    /// Optional precomputed capture, prepared by the app's background worker.
    pub overlay: Option<crate::terrain_drape::TerrainOverlay>,
}

/// Build a bounded visual batch. `elevation` is a render-support callback supplied
/// by the app; it must return a finite ellipsoidal height or None. The exclusion
/// callback suppresses procedural tree placement on roads, buildings or airports.
/// A rejected feature is rolled back in full, never emitted half-built.
#[must_use]
pub fn scenery_mesh(
    features: &[SceneryFeatureRef<'_>],
    anchor: Geodetic,
    options: SceneryMeshOptions<'_>,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
    exclude_tree: &dyn Fn(Geodetic) -> bool,
) -> Option<SceneryMesh> {
    if !valid_point(anchor)
        || features.len() > MAX_BATCH_FEATURES
        || !valid_point(options.observer)
        || !options.vegetation_distance.is_finite()
        || options.vegetation_distance.get() < 0.0
        || options.max_vertices < 3
    {
        return None;
    }
    let mut builder = Builder::new(
        anchor.to_ecef(),
        options.max_vertices.min(MAX_BATCH_VERTICES),
        options.cancellation,
    );
    let mut ground_builder = Builder::new(
        anchor.to_ecef(),
        options
            .max_ground_vertices
            .min(options.max_vertices)
            .min(MAX_BATCH_VERTICES),
        options.cancellation,
    );
    ground_builder.airport_exclusions = options.airport_ground_exclusions;
    let total_limit = options.max_vertices.min(MAX_BATCH_VERTICES);
    let mut stats = SceneryMeshStatistics::default();
    // Prioritize mapped structures and roads if a batch reaches its geometry
    // cap. Height offsets, not submission order, order overlapping ground layers.
    for pass in 0..3 {
        for feature in features {
            if builder.cancelled() {
                return None;
            }
            let relevant = matches!(
                (pass, feature),
                (0, SceneryFeatureRef::Building(_))
                    | (1, SceneryFeatureRef::Road(_))
                    | (2, SceneryFeatureRef::LandCover(_))
            );
            if !relevant {
                continue;
            }
            let target = if matches!(feature, SceneryFeatureRef::Building(_)) {
                builder.limit = total_limit.saturating_sub(ground_builder.len());
                &mut builder
            } else {
                ground_builder.limit = options
                    .max_ground_vertices
                    .min(total_limit.saturating_sub(builder.len()));
                &mut ground_builder
            };
            let start = target.len();
            let result = match feature {
                SceneryFeatureRef::Road(road) => build_road(target, road, elevation),
                SceneryFeatureRef::Building(building) => {
                    build_building(target, building, options.facades, elevation)
                }
                SceneryFeatureRef::LandCover(land) => build_land(target, land, elevation),
            };
            if result {
                match feature {
                    SceneryFeatureRef::Road(_) => stats.roads += 1,
                    SceneryFeatureRef::Building(_) => stats.buildings += 1,
                    SceneryFeatureRef::LandCover(_) => stats.land_polygons += 1,
                }
            } else {
                target.truncate(start);
                stats.skipped_features += 1;
            }
        }
    }
    if options.vegetation {
        builder.limit = total_limit.saturating_sub(ground_builder.len());
        let max_trees = options.max_trees.min(MAX_BATCH_TREES);
        for feature in features {
            let SceneryFeatureRef::LandCover(land) = feature else {
                continue;
            };
            if land.class != LandCoverClass::Forest
                || stats.procedural_trees >= max_trees
                || stats.tree_candidates
                    >= options.max_tree_candidates.min(MAX_BATCH_TREE_CANDIDATES)
            {
                continue;
            }
            plant_trees(
                &mut builder,
                land,
                options,
                max_trees,
                &mut stats,
                elevation,
                exclude_tree,
            );
        }
    }
    if (builder.len() == 0 && ground_builder.len() == 0) || builder.cancelled() {
        return None;
    }
    stats.vertices = builder.len() + ground_builder.len();
    stats.triangles = stats.vertices / 3;
    let ground = if ground_builder.len() > 0 {
        let lifts = std::mem::take(&mut ground_builder.lifts);
        Some(SceneryGroundMesh {
            mesh: ground_builder.finish(),
            lifts,
            overlay: None,
        })
    } else {
        None
    };
    Some(SceneryMesh {
        origin: builder.origin,
        mesh: builder.finish(),
        ground,
        statistics: stats,
    })
}

fn valid_point(p: Geodetic) -> bool {
    p.latitude.is_finite()
        && p.longitude.is_finite()
        && p.altitude.is_finite()
        && p.latitude.get().abs() <= core::f64::consts::FRAC_PI_2
        && p.longitude.get().abs() <= core::f64::consts::PI
}

fn at_height(p: Geodetic, height: f64) -> Ecef {
    Geodetic::new(p.latitude, p.longitude, Meters(height)).to_ecef()
}

fn height_at(p: Geodetic, elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>) -> Option<f64> {
    let value = elevation(p)?;
    (value.is_finite() && (-20_000.0..=100_000.0).contains(&value.get())).then_some(value.get())
}

fn color(srgb: [f32; 3]) -> [f32; 4] {
    [
        crate::srgb_to_linear(srgb[0]),
        crate::srgb_to_linear(srgb[1]),
        crate::srgb_to_linear(srgb[2]),
        1.0,
    ]
}

/// Terrain-independent, public-source-ID seed. No frame/camera/LOD random state.
fn hash(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn random_unit(seed: u64) -> f64 {
    let bounded = u32::try_from(hash(seed) >> 32).expect("upper 32 bits");
    f64::from(bounded) / f64::from(u32::MAX)
}

fn source_seed(id: i64) -> u64 {
    u64::from_ne_bytes(id.to_ne_bytes())
}

struct Builder<'a> {
    origin: Ecef,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    lifts: Vec<f32>,
    layer_lift: f32,
    limit: usize,
    cancellation: Option<&'a AtomicBool>,
    airport_exclusions: Option<&'a crate::scenery_exclusions::GroundSceneryExclusions>,
    exclusion_work: usize,
}

impl<'a> Builder<'a> {
    fn new(origin: Ecef, limit: usize, cancellation: Option<&'a AtomicBool>) -> Self {
        Self {
            origin,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            lifts: Vec::new(),
            layer_lift: 0.0,
            limit,
            cancellation,
            airport_exclusions: None,
            exclusion_work: 100_000,
        }
    }
    fn len(&self) -> usize {
        self.positions.len()
    }
    fn cancelled(&self) -> bool {
        self.cancellation
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
    fn truncate(&mut self, len: usize) {
        self.positions.truncate(len);
        self.normals.truncate(len);
        self.colors.truncate(len);
        self.lifts.truncate(len);
    }
    fn triangle(&mut self, points: [Ecef; 3], tint: [f32; 4], top: bool) -> bool {
        if self.cancelled() {
            return false;
        }
        if self.len().saturating_add(3) > self.limit {
            return false;
        }
        let mut p = points.map(Ecef::as_vec);
        if p.iter().any(|p| !p.is_finite()) {
            return false;
        }
        if self
            .airport_exclusions
            .is_some_and(|mask| mask.intersects_triangle(points, &mut self.exclusion_work))
        {
            return true;
        }
        let mut normal = (p[1] - p[0]).cross(p[2] - p[0]);
        if normal.length_squared() < 1.0e-12 {
            return true;
        }
        if top && normal.dot(p[0]) < 0.0 {
            p.swap(1, 2);
            normal = -normal;
        }
        normal = normal.normalize();
        for point in p {
            let local = point - self.origin.as_vec();
            if local.length_squared() > 400_000.0_f64.powi(2) {
                return false;
            }
            self.positions.push(local.as_vec3().to_array());
            self.normals.push(normal.as_vec3().to_array());
            self.colors.push(tint);
            self.lifts.push(self.layer_lift);
        }
        true
    }
    fn quad(&mut self, p: [Ecef; 4], tint: [f32; 4], top: bool) -> bool {
        self.triangle([p[0], p[1], p[2]], tint, top) && self.triangle([p[0], p[2], p[3]], tint, top)
    }
    fn finish(self) -> Mesh {
        let indices: Vec<_> = (0..self.positions.len())
            .map(|n| u32::try_from(n).expect("bounded batch"))
            .collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(indices))
    }
}

fn build_building(
    builder: &mut Builder,
    building: &SceneryBuilding,
    facades: bool,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
) -> bool {
    build_building_with_foundation(builder, building, facades, elevation, FOUNDATION_DEPTH)
}

fn build_building_with_foundation(
    builder: &mut Builder,
    building: &SceneryBuilding,
    facades: bool,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
    foundation_depth: Meters,
) -> bool {
    if building.footprint.len() < 3
        || building.footprint.len() > 512
        || building.footprint.iter().any(|&p| !valid_point(p))
        || !building.height.is_finite()
        || !(1.0..=600.0).contains(&building.height.get())
    {
        return false;
    }
    let Some(ground): Option<Vec<f64>> = building
        .footprint
        .iter()
        .map(|&p| height_at(p, elevation))
        .collect()
    else {
        return false;
    };
    let highest = ground.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lowest = ground.iter().copied().fold(f64::INFINITY, f64::min);
    // Extreme source/DEM disagreement must not create a skyscraper foundation.
    if highest - lowest > 30.0 {
        return false;
    }
    let roof_height = highest + building.height.get();
    let seed = source_seed(building.source_id);
    let palette = [
        [0.75, 0.71, 0.61],
        [0.79, 0.75, 0.68],
        [0.66, 0.67, 0.62],
        [0.70, 0.63, 0.51],
        [0.79, 0.76, 0.71],
    ];
    let tint = color(palette[usize::try_from(hash(seed) % 5).expect("palette bound")]);
    let roof_palette = [[0.41, 0.23, 0.17], [0.25, 0.28, 0.30], [0.39, 0.37, 0.32]];
    let roof = color(
        roof_palette[usize::try_from(hash(seed.wrapping_add(1)) % 3).expect("palette bound")],
    );
    for triangle in &building.triangles {
        let Some(points): Option<Vec<_>> = triangle
            .iter()
            .map(|&index| building.footprint.get(index as usize).copied())
            .collect()
        else {
            return false;
        };
        if !builder.triangle(
            [
                at_height(points[0], roof_height),
                at_height(points[1], roof_height),
                at_height(points[2], roof_height),
            ],
            roof,
            true,
        ) {
            return false;
        }
    }
    // Input simple polygons are validated/canonicalized by the database; use
    // outward winding derived from the footprint's signed local area as well.
    let local = LocalFrame::new(building.footprint[0]);
    let area: f64 = building
        .footprint
        .iter()
        .zip(building.footprint.iter().cycle().skip(1))
        .take(building.footprint.len())
        .map(|(&a, &b)| {
            let a = local.ecef_to_ned_position(a.to_ecef());
            let b = local.ecef_to_ned_position(b.to_ecef());
            a.east() * b.north() - b.east() * a.north()
        })
        .sum();
    for index in 0..building.footprint.len() {
        let next = (index + 1) % building.footprint.len();
        let a = building.footprint[index];
        let b = building.footprint[next];
        let mut wall = [
            at_height(a, ground[index] - foundation_depth.get()),
            at_height(b, ground[next] - foundation_depth.get()),
            at_height(b, roof_height),
            at_height(a, roof_height),
        ];
        if area < 0.0 {
            wall.reverse();
        }
        if !builder.quad(wall, tint, false) {
            return false;
        }
        if facades
            && !windows(
                builder,
                a,
                b,
                ground[index].max(ground[next]),
                roof_height,
                area,
                seed.wrapping_add(index as u64),
            )
        {
            return false;
        }
    }
    true
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "window grid dimensions are explicitly floored and capped to12columns/6rows"
)]
fn windows(
    builder: &mut Builder,
    a: Geodetic,
    b: Geodetic,
    ground: f64,
    roof: f64,
    area: f64,
    seed: u64,
) -> bool {
    let left = at_height(a, ground);
    let right = at_height(b, ground);
    let width = left.distance_to(right).get();
    if !(4.0..=120.0).contains(&width) || roof - ground < 3.0 {
        return true;
    }
    let columns = (width / 3.2).floor().clamp(1.0, 12.0) as u32;
    let rows = ((roof - ground - 1.0) / 3.0).floor().clamp(1.0, 6.0) as u32;
    let up = LocalFrame::new(a).ned_to_ecef_vector(Ned::new(0.0, 0.0, -1.0));
    let edge = right.as_vec() - left.as_vec();
    let outward = edge.cross(up).normalize() * if area < 0.0 { -0.025 } else { 0.025 };
    for row in 0..rows {
        for col in 0..columns {
            let x = (f64::from(col) + 0.5) / f64::from(columns);
            let half = (0.62 / width).min(0.3 / f64::from(columns));
            let z = 1.2 + f64::from(row) * 3.0;
            let base = left.as_vec() + outward;
            let mut quad = [
                base + edge * (x - half) + up * z,
                base + edge * (x + half) + up * z,
                base + edge * (x + half) + up * (z + 1.1),
                base + edge * (x - half) + up * (z + 1.1),
            ]
            .map(Ecef::from_vec);
            if area < 0.0 {
                quad.reverse();
            }
            let tint = if hash(seed.wrapping_add(u64::from(row) * 31 + u64::from(col)))
                .is_multiple_of(7)
            {
                color([0.39, 0.43, 0.43])
            } else {
                color([0.14, 0.22, 0.25])
            };
            if !builder.quad(quad, tint, false) {
                return false;
            }
        }
    }
    true
}

fn road_tint(class: RoadClass) -> [f32; 4] {
    color(match class {
        RoadClass::Track => [0.46, 0.39, 0.27],
        RoadClass::Path => [0.56, 0.48, 0.33],
        _ => [0.20, 0.22, 0.22],
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "road step count is finite, explicitly ceiled and checked against512 before integer conversion"
)]
fn build_road(
    builder: &mut Builder,
    road: &SceneryRoad,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
) -> bool {
    if road.points.len() < 2
        || road.points.len() > 512
        || road.points.iter().any(|&p| !valid_point(p))
        || !road.width.is_finite()
        || !(0.2..=100.0).contains(&road.width.get())
    {
        return false;
    }
    let mut steps = 0usize;
    for pair in road.points.windows(2) {
        let frame = LocalFrame::new(pair[0]);
        let delta = frame.ecef_to_ned_position(pair[1].to_ecef());
        let length = delta.horizontal_magnitude();
        if length < 0.01 {
            continue;
        }
        let n = (length / 24.0).ceil().max(1.0);
        if n > f64::from(u32::try_from(MAX_ROAD_STEPS).expect("small fixed budget"))
            || steps.saturating_add(n as usize) > MAX_ROAD_STEPS
        {
            return false;
        }
        steps += n as usize;
        let sideways = Ned::new(
            -delta.east() / length * road.width.get() * 0.5,
            delta.north() / length * road.width.get() * 0.5,
            0.0,
        );
        for step in 0..n as u32 {
            builder.layer_lift = 0.12;
            let p0 = delta.0 * (f64::from(step) / n);
            let p1 = delta.0 * (f64::from(step + 1) / n);
            let points = [
                p0 + sideways.0,
                p1 + sideways.0,
                p1 - sideways.0,
                p0 - sideways.0,
            ]
            .map(|p| frame.ned_to_ecef_position(Ned(p)).to_geodetic());
            let Some(heights): Option<Vec<_>> =
                points.iter().map(|&p| height_at(p, elevation)).collect()
            else {
                return false;
            };
            if !builder.quad(
                std::array::from_fn(|i| at_height(points[i], heights[i] + 0.12)),
                road_tint(road.class),
                true,
            ) {
                return false;
            }
            // These generic dashed centre cues are depiction, not lane metadata.
            if matches!(
                road.class,
                RoadClass::Motorway | RoadClass::Primary | RoadClass::Secondary
            ) && step % 2 == 0
            {
                builder.layer_lift = 0.15;
                let mid = p0 + (p1 - p0) * 0.5;
                let slim = sideways.0 * (0.24 / road.width.get());
                let mark = [p0 + slim, mid + slim, mid - slim, p0 - slim]
                    .map(|p| frame.ned_to_ecef_position(Ned(p)).to_geodetic());
                let Some(heights): Option<Vec<_>> =
                    mark.iter().map(|&p| height_at(p, elevation)).collect()
                else {
                    return false;
                };
                if !builder.quad(
                    std::array::from_fn(|i| at_height(mark[i], heights[i] + 0.15)),
                    color([0.70, 0.69, 0.58]),
                    true,
                ) {
                    return false;
                }
            }
        }
    }
    steps > 0
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "authored finite colour channels remain in[0,1]"
)]
fn land_tint(class: LandCoverClass, id: i64) -> [f32; 4] {
    let variation = (random_unit(source_seed(id)) - 0.5) * 0.08;
    let base = match class {
        LandCoverClass::Forest => [0.15, 0.27, 0.12],
        LandCoverClass::Grass => [0.39, 0.48, 0.24],
        LandCoverClass::Farmland => [0.51, 0.48, 0.28],
        LandCoverClass::Residential => [0.40, 0.44, 0.31],
        LandCoverClass::Industrial => [0.41, 0.42, 0.39],
        LandCoverClass::Water => [0.055, 0.24, 0.31],
        LandCoverClass::Bare => [0.54, 0.47, 0.34],
        LandCoverClass::Rock => [0.47, 0.46, 0.42],
    };
    color(base.map(|v| (v + variation) as f32))
}

fn build_land(
    builder: &mut Builder,
    land: &SceneryLandCover,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
) -> bool {
    builder.layer_lift = 0.055;
    if land.boundary.len() < 3
        || land.boundary.len() > 512
        || land.triangles.len() > 510
        || land.boundary.iter().any(|&p| !valid_point(p))
    {
        return false;
    }
    let tint = land_tint(land.class, land.source_id);
    let mut emitted = 0usize;
    for triangle in &land.triangles {
        let Some(points): Option<Vec<_>> = triangle
            .iter()
            .map(|&i| land.boundary.get(i as usize).copied())
            .collect()
        else {
            return false;
        };
        let p = points
            .iter()
            .map(|p| p.to_ecef().as_vec())
            .collect::<Vec<_>>();
        let mut stack = vec![[p[0], p[1], p[2]]];
        while let Some(t) = stack.pop() {
            if builder.cancelled() {
                return false;
            }
            let lengths = [
                t[0].distance_squared(t[1]),
                t[1].distance_squared(t[2]),
                t[2].distance_squared(t[0]),
            ];
            let edge = (0..3)
                .max_by(|&a, &b| lengths[a].total_cmp(&lengths[b]))
                .expect("three edges");
            if lengths[edge] > 80.0 * 80.0 {
                if emitted + stack.len() + 2 > MAX_LAND_TRIANGLES {
                    return false;
                }
                let a = edge;
                let b = (edge + 1) % 3;
                let c = (edge + 2) % 3;
                let middle = (t[a] + t[b]) * 0.5;
                stack.push([t[a], middle, t[c]]);
                stack.push([middle, t[b], t[c]]);
                continue;
            }
            emitted += 1;
            if emitted > MAX_LAND_TRIANGLES {
                return false;
            }
            let points = t.map(|v| Ecef::from_vec(v).to_geodetic());
            let Some(heights): Option<Vec<_>> =
                points.iter().map(|&p| height_at(p, elevation)).collect()
            else {
                return false;
            };
            if !builder.triangle(
                std::array::from_fn(|i| at_height(points[i], heights[i] + 0.055)),
                tint,
                true,
            ) {
                return false;
            }
        }
    }
    true
}

/// Original tree positions live on a polygon-local, source-ID-jittered lattice.
/// Camera motion changes the bounded selection order, never a cell's position.
/// Testing nearest cells avoids spending every attempt on remote triangles of a
/// large forest while the observer stands inside its unsampled middle.
#[allow(
    clippy::cast_possible_truncation,
    reason = "horizontal Earth coordinates divided by40 metres fit i32"
)]
fn plant_trees(
    builder: &mut Builder,
    land: &SceneryLandCover,
    options: SceneryMeshOptions<'_>,
    max_trees: usize,
    stats: &mut SceneryMeshStatistics,
    elevation: &mut dyn FnMut(Geodetic) -> Option<Meters>,
    exclude: &dyn Fn(Geodetic) -> bool,
) {
    if !(3..=512).contains(&land.boundary.len()) || land.boundary.iter().any(|&p| !valid_point(p)) {
        return;
    }
    let first = land.boundary[0];
    let frame = LocalFrame::new(Geodetic::new(first.latitude, first.longitude, Meters::ZERO));
    let horizontal = |p: Geodetic| frame.ecef_to_ned_position(at_height(p, 0.0)).0.truncate();
    let polygon: Vec<_> = land.boundary.iter().map(|&p| horizontal(p)).collect();
    let lower = polygon
        .iter()
        .copied()
        .fold(glam::DVec2::splat(f64::INFINITY), glam::DVec2::min);
    let upper = polygon
        .iter()
        .copied()
        .fold(glam::DVec2::splat(f64::NEG_INFINITY), glam::DVec2::max);
    let observer = horizontal(options.observer);
    if observer.distance(observer.clamp(lower, upper)) > options.vegetation_distance.get() + 40.0 {
        return;
    }
    let nearest = if inside_forest(observer, &polygon) {
        observer
    } else {
        closest_forest_boundary(observer, &polygon)
    };
    if observer.distance(nearest) > options.vegetation_distance.get() + 40.0 {
        return;
    }
    let base = [
        (nearest.x / 40.0).floor() as i32,
        (nearest.y / 40.0).floor() as i32,
    ];
    let mut offset = [0_i32; 2];
    let mut direction = [0_i32, -1_i32];
    let attempts = options
        .max_tree_candidates
        .min(MAX_BATCH_TREE_CANDIDATES)
        .saturating_sub(stats.tree_candidates);
    for _ in 0..attempts {
        if builder.cancelled() || stats.procedural_trees >= max_trees {
            return;
        }
        stats.tree_candidates += 1;
        let cell = [base[0] + offset[0], base[1] + offset[1]];
        if offset[0] == offset[1]
            || (offset[0] < 0 && offset[0] == -offset[1])
            || (offset[0] > 0 && offset[0] == 1 - offset[1])
        {
            direction = [-direction[1], direction[0]];
        }
        offset[0] += direction[0];
        offset[1] += direction[1];
        let seed = hash(source_seed(land.source_id))
            ^ hash(u64::from(u32::from_ne_bytes(cell[0].to_ne_bytes())))
            ^ hash(u64::from(u32::from_ne_bytes(cell[1].to_ne_bytes())).wrapping_add(0x517c_c1b7));
        let candidate = glam::DVec2::new(
            f64::from(cell[0]) * 40.0 + 20.0 + (random_unit(seed) - 0.5) * 24.0,
            f64::from(cell[1]) * 40.0 + 20.0 + (random_unit(seed.wrapping_add(1)) - 0.5) * 24.0,
        );
        if !inside_forest(candidate, &polygon) {
            continue;
        }
        let p = frame
            .ned_to_ecef_position(Ned::new(candidate.x, candidate.y, 0.0))
            .to_geodetic();
        let p = Geodetic::new(p.latitude, p.longitude, Meters::ZERO);
        // Projection from the source tangent plane onto the ellipsoid changes
        // horizontal coordinates. Recheck the final location, especially for
        // large/concave source polygons; a pre-projection test is insufficient.
        if !inside_forest(horizontal(p), &polygon)
            || p.great_circle_distance(options.observer).get() > options.vegetation_distance.get()
            || exclude(p)
        {
            continue;
        }
        let Some(ground) = height_at(p, elevation) else {
            continue;
        };
        let start = builder.len();
        if tree(
            builder,
            Geodetic::new(p.latitude, p.longitude, Meters(ground)),
            Meters(7.0 + random_unit(seed.wrapping_add(2)) * 9.0),
            seed,
        ) {
            stats.procedural_trees += 1;
        } else {
            builder.truncate(start);
            return;
        }
    }
}

fn closest_forest_boundary(point: glam::DVec2, polygon: &[glam::DVec2]) -> glam::DVec2 {
    let mut closest = polygon[0];
    let mut distance = f64::INFINITY;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        let edge = current - previous;
        let denominator = edge.length_squared();
        let fraction = if denominator > 0.0 {
            ((point - previous).dot(edge) / denominator).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let candidate = previous + edge * fraction;
        let next_distance = candidate.distance_squared(point);
        if next_distance < distance {
            closest = candidate;
            distance = next_distance;
        }
        previous = current;
    }
    closest
}

/// Even/odd test for the validated simple polygon; boundary candidates may be
/// conservatively omitted. No triangle-order dependence or filled holes.
fn inside_forest(point: glam::DVec2, polygon: &[glam::DVec2]) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if (current.y > point.y) != (previous.y > point.y)
            && point.x
                < (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y)
                    + current.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn tree(builder: &mut Builder, p: Geodetic, height: Meters, seed: u64) -> bool {
    tree_with_roots(builder, p, height, seed, FOUNDATION_DEPTH)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite original foliage colour variation is bounded below0.1"
)]
fn tree_with_roots(
    builder: &mut Builder,
    p: Geodetic,
    height: Meters,
    seed: u64,
    root_depth: Meters,
) -> bool {
    let frame = LocalFrame::new(p);
    let h = height.get();
    let foliage = color([
        0.12 + (random_unit(seed) * 0.07) as f32,
        0.24 + (random_unit(seed.wrapping_add(1)) * 0.09) as f32,
        0.10,
    ]);
    let trunk = color([0.27, 0.20, 0.13]);
    let vertex =
        |north: f64, east: f64, up: f64| frame.ned_to_ecef_position(Ned::new(north, east, -up));
    // Retain the original side/trunk vertices and collect their six existing
    // rim positions per crown. The bases below need no new coordinate transforms.
    let mut crown_rims = [[builder.origin; 6]; 2];
    for side in 0..6_u8 {
        let a = f64::from(side) * core::f64::consts::TAU / 6.0;
        let b = f64::from(side + 1) * core::f64::consts::TAU / 6.0;
        if !builder.quad(
            [
                vertex(b.cos() * 0.20, b.sin() * 0.20, -root_depth.get()),
                vertex(a.cos() * 0.20, a.sin() * 0.20, -root_depth.get()),
                vertex(a.cos() * 0.20, a.sin() * 0.20, h * 0.45),
                vertex(b.cos() * 0.20, b.sin() * 0.20, h * 0.45),
            ],
            trunk,
            false,
        ) {
            return false;
        }
        for (crown, (bottom, top, radius)) in [(0.24, 0.80, 0.24), (0.48, 1.0, 0.18)]
            .into_iter()
            .enumerate()
        {
            let rim = vertex(a.cos() * h * radius, a.sin() * h * radius, h * bottom);
            crown_rims[crown][usize::from(side)] = rim;
            if !builder.triangle(
                [
                    vertex(b.cos() * h * radius, b.sin() * h * radius, h * bottom),
                    rim,
                    vertex(0.0, 0.0, h * top),
                ],
                foliage,
                false,
            ) {
                return false;
            }
        }
    }
    // Opaque, back-face-culled crown shells otherwise disappear from below.
    // Four triangles close each hexagon, bringing one tree from 72 to 96
    // vertices (24 to 32 triangles). Increasing north/east angle winds down;
    // top=false must preserve that winding. The caller rolls back a whole tree
    // if a cap exhausts the unchanged vertex budget or observes cancellation.
    for rim in crown_rims {
        for side in 1..5 {
            if !builder.triangle([rim[0], rim[side], rim[side + 1]], foliage, false) {
                return false;
            }
        }
    }
    true
}

/// Original geometry uses opaque vertex colours; no texture dependencies or
/// separate shadow-casting light/entity is created per building or tree.
#[must_use]
pub fn scenery_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.94,
        metallic: 0.0,
        ..default()
    }
}

#[cfg(test)]
#[path = "scenery_canopy_tests.rs"]
mod canopy_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use flightsim_world::scenery::BuildingHeightSource;

    fn anchor() -> Geodetic {
        Geodetic::from_degrees(47.13, 9.53, 0.0)
    }
    fn point(north: f64, east: f64) -> Geodetic {
        let p = LocalFrame::new(anchor())
            .ned_to_ecef_position(Ned::new(north, east, 0.0))
            .to_geodetic();
        Geodetic::new(p.latitude, p.longitude, Meters::ZERO)
    }
    fn building() -> SceneryBuilding {
        SceneryBuilding {
            source_id: 1,
            height: Meters(9.0),
            height_source: BuildingHeightSource::Default,
            footprint: vec![
                point(0.0, 0.0),
                point(0.0, 20.0),
                point(16.0, 20.0),
                point(16.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }
    }
    fn road() -> SceneryRoad {
        SceneryRoad {
            source_id: 2,
            class: RoadClass::Residential,
            width: Meters(6.0),
            width_inferred: false,
            points: vec![point(0.0, 0.0), point(80.0, 0.0)],
        }
    }
    fn forest() -> SceneryLandCover {
        SceneryLandCover {
            source_id: 3,
            class: LandCoverClass::Forest,
            boundary: vec![
                point(0.0, 0.0),
                point(0.0, 200.0),
                point(200.0, 200.0),
                point(200.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }
    }
    fn positions(mesh: &Mesh) -> &Vec<[f32; 3]> {
        let Some(VertexAttributeValues::Float32x3(values)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        values
    }
    fn plain(features: &[SceneryFeatureRef<'_>]) -> SceneryMesh {
        scenery_mesh(
            features,
            anchor(),
            SceneryMeshOptions::near(anchor()),
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap()
    }
    #[test]
    fn buildings_are_mapped_footprints_with_bounded_original_facades() {
        let source = building();
        let output = plain(&[SceneryFeatureRef::Building(&source)]);
        assert_eq!(output.statistics.buildings, 1);
        assert_eq!(output.statistics.roads, 0);
        assert!(output.statistics.vertices > 36 && output.statistics.vertices < 1000);
        for p in positions(&output.mesh) {
            let world =
                Ecef::from_vec(output.origin.as_vec() + glam::Vec3::from_array(*p).as_dvec3())
                    .to_geodetic();
            assert!(
                (447.99..=459.01).contains(&world.altitude.get()),
                "{}",
                world.altitude.get()
            );
        }
    }
    #[test]
    fn shallow_foundations_preserve_roofs_facades_and_mesh_topology() {
        let source = building();
        let before = source.clone();
        let origin = anchor().to_ecef();
        let make = |depth| {
            let mut builder = Builder::new(origin, MAX_BATCH_VERTICES, None);
            assert!(build_building_with_foundation(
                &mut builder,
                &source,
                true,
                &mut |_| Some(Meters(450.0)),
                depth
            ));
            builder.finish()
        };
        let old = make(Meters(0.4));
        let new = make(FOUNDATION_DEPTH);
        assert_eq!(source, before);
        assert_eq!(old.count_vertices(), new.count_vertices());
        assert_eq!(
            old.indices().unwrap().iter().collect::<Vec<_>>(),
            new.indices().unwrap().iter().collect::<Vec<_>>()
        );
        assert_eq!(
            old.attribute(Mesh::ATTRIBUTE_COLOR),
            new.attribute(Mesh::ATTRIBUTE_COLOR)
        );
        let altitude = |p: &[f32; 3]| {
            Ecef(origin.as_vec() + glam::Vec3::from_array(*p).as_dvec3())
                .to_geodetic()
                .altitude
                .get()
        };
        let mut lowered = 0;
        let mut unchanged = 0;
        for (old, new) in positions(&old).iter().zip(positions(&new)) {
            let old_altitude = altitude(old);
            let new_altitude = altitude(new);
            if old_altitude < 449.9 {
                assert!((new_altitude - 448.0).abs() < 0.001);
                assert!((old_altitude - new_altitude - 1.6).abs() < 0.001);
                lowered += 1;
            } else {
                assert_eq!(old.map(f32::to_bits), new.map(f32::to_bits));
                unchanged += 1;
            }
        }
        assert_eq!(lowered, 12, "only duplicated bottom wall endpoints move");
        assert!(
            unchanged > 12,
            "roofs, upper walls and facade decoration stay exact"
        );
    }

    #[test]
    fn shallow_roots_preserve_tree_crowns_and_topology_with_bounded_depth() {
        let p = Geodetic::from_degrees(47.1742644457, 9.5925149085, 1084.88);
        let origin = p.to_ecef();
        let frame = LocalFrame::new(p);
        for height in [7.0, 11.0, 16.0] {
            let make = |depth| {
                let mut builder = Builder::new(origin, MAX_BATCH_VERTICES, None);
                assert!(tree_with_roots(&mut builder, p, Meters(height), 17, depth));
                builder.finish()
            };
            let old = make(Meters::ZERO);
            let new = make(FOUNDATION_DEPTH);
            assert_eq!(old.count_vertices(), 96);
            assert_eq!(new.count_vertices(), old.count_vertices());
            assert_eq!(
                old.indices().unwrap().iter().collect::<Vec<_>>(),
                new.indices().unwrap().iter().collect::<Vec<_>>()
            );
            assert_eq!(
                old.attribute(Mesh::ATTRIBUTE_COLOR),
                new.attribute(Mesh::ATTRIBUTE_COLOR)
            );
            let local = |p: &[f32; 3]| {
                frame.ecef_to_ned_position(Ecef(
                    origin.as_vec() + glam::Vec3::from_array(*p).as_dvec3(),
                ))
            };
            let mut lowered = 0;
            for (old, new) in positions(&old).iter().zip(positions(&new)) {
                let old_local = local(old);
                let new_local = local(new);
                if old_local.up().abs() < 0.001 {
                    assert!((new_local.up() + FOUNDATION_DEPTH.get()).abs() < 0.001);
                    assert!((old_local.north() - new_local.north()).abs() < 0.001);
                    assert!((old_local.east() - new_local.east()).abs() < 0.001);
                    lowered += 1;
                } else {
                    assert_eq!(old.map(f32::to_bits), new.map(f32::to_bits));
                }
            }
            assert_eq!(lowered, 18, "only duplicated trunk bottom vertices move");
        }
    }

    #[test]
    fn changing_facade_detail_never_changes_source_geometry() {
        let source = building();
        let before = source.clone();
        let detailed = plain(&[SceneryFeatureRef::Building(&source)]);
        let mut options = SceneryMeshOptions::near(anchor());
        options.facades = false;
        let simple = scenery_mesh(
            &[SceneryFeatureRef::Building(&source)],
            anchor(),
            options,
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap();
        assert_eq!(source, before);
        assert_eq!(simple.statistics.vertices, 30);
        assert!(detailed.statistics.vertices > simple.statistics.vertices);
    }
    #[test]
    fn failed_feature_rolls_back_without_partial_walls() {
        let source = building();
        let mut options = SceneryMeshOptions::near(anchor());
        options.max_vertices = 12;
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Building(&source)],
                anchor(),
                options,
                &mut |_| Some(Meters(0.0)),
                &|_| false
            )
            .is_none()
        );
    }
    #[test]
    fn invalid_heights_indices_and_nonfinite_geometry_are_rejected() {
        let mut source = building();
        source.height = Meters(f64::NAN);
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Building(&source)],
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| Some(Meters(0.0)),
                &|_| false
            )
            .is_none()
        );
        source = building();
        source.triangles[0][0] = 999;
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Building(&source)],
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| Some(Meters(0.0)),
                &|_| false
            )
            .is_none()
        );
        source = building();
        source.footprint[0].latitude = flightsim_core::Radians(f64::NAN);
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Building(&source)],
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| Some(Meters(0.0)),
                &|_| false
            )
            .is_none()
        );
    }
    #[test]
    fn roads_use_finite_opaque_geometry_and_source_width() {
        let source = road();
        let output = plain(&[SceneryFeatureRef::Road(&source)]);
        assert_eq!(output.statistics.roads, 1);
        assert_eq!(output.statistics.vertices, 24);
        let ground = &output.ground.as_ref().unwrap().mesh;
        let local = LocalFrame::new(anchor());
        let east: Vec<_> = positions(ground)
            .iter()
            .map(|p| {
                local
                    .ecef_to_ned_position(Ecef::from_vec(
                        output.origin.as_vec() + glam::Vec3::from_array(*p).as_dvec3(),
                    ))
                    .east()
            })
            .collect();
        let min = east.iter().copied().fold(f64::INFINITY, f64::min);
        let max = east.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!((max - min - 6.0).abs() < 0.01);
        let Some(VertexAttributeValues::Float32x4(colors)) =
            ground.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors")
        };
        assert!(colors.iter().all(
            |c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && (c[3] - 1.0).abs() < f32::EPSILON
        ));
    }
    #[test]
    fn excessive_road_subdivision_is_rejected_before_expansion() {
        let mut source = road();
        source.points[1] = point(20_000.0, 0.0);
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Road(&source)],
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| Some(Meters(0.0)),
                &|_| false
            )
            .is_none()
        );
    }
    #[test]
    fn procedural_forest_is_stable_bounded_and_respects_exclusion() {
        let source = forest();
        let a = plain(&[SceneryFeatureRef::LandCover(&source)]);
        let b = plain(&[SceneryFeatureRef::LandCover(&source)]);
        assert!(a.statistics.procedural_trees > 0);
        assert!(a.statistics.procedural_trees <= MAX_BATCH_TREES);
        assert_eq!(positions(&a.mesh), positions(&b.mesh));
        assert_eq!(a.statistics, b.statistics);
        let excluded = scenery_mesh(
            &[SceneryFeatureRef::LandCover(&source)],
            anchor(),
            SceneryMeshOptions::near(anchor()),
            &mut |_| Some(Meters(450.0)),
            &|_| true,
        )
        .unwrap();
        assert_eq!(excluded.statistics.procedural_trees, 0);
        assert_eq!(excluded.statistics.land_polygons, 1);
        let mut limited = SceneryMeshOptions::near(anchor());
        limited.max_trees = 2;
        let limited = scenery_mesh(
            &[SceneryFeatureRef::LandCover(&source)],
            anchor(),
            limited,
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap();
        assert_eq!(limited.statistics.procedural_trees, 2);
    }
    #[test]
    fn no_missing_terrain_is_silently_assumed_to_be_sea_level() {
        let source = building();
        assert!(
            scenery_mesh(
                &[SceneryFeatureRef::Building(&source)],
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| None,
                &|_| false
            )
            .is_none()
        );
    }
    #[test]
    fn local_origin_changes_preserve_world_geometry() {
        let source = building();
        let a = plain(&[SceneryFeatureRef::Building(&source)]);
        let b = scenery_mesh(
            &[SceneryFeatureRef::Building(&source)],
            point(2000.0, 1000.0),
            SceneryMeshOptions::near(anchor()),
            &mut |_| Some(Meters(450.0)),
            &|_| false,
        )
        .unwrap();
        for (x, y) in positions(&a.mesh).iter().zip(positions(&b.mesh)) {
            let x = a.origin.as_vec() + glam::Vec3::from_array(*x).as_dvec3();
            let y = b.origin.as_vec() + glam::Vec3::from_array(*y).as_dvec3();
            assert!(x.distance(y) < 0.001);
        }
    }
    #[test]
    fn external_batch_size_is_checked_before_geometry_work() {
        let source = building();
        let features = vec![SceneryFeatureRef::Building(&source); MAX_BATCH_FEATURES + 1];
        assert!(
            scenery_mesh(
                &features,
                anchor(),
                SceneryMeshOptions::near(anchor()),
                &mut |_| panic!("must not sample"),
                &|_| false
            )
            .is_none()
        );
    }
}
