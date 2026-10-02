//! Bounded, offline regional surface scenery. No raw GIS decoder or network is used.
//!
//! Positions are horizontal WGS84 coordinates with zero altitude. Heights and road
//! widths are lengths, not elevations. Consumers drape scenery on their selected
//! ellipsoidal DEM; scenery never changes terrain or flight dynamics.

use flightsim_core::{Geodetic, LocalFrame, Meters};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub mod io;

/// Hard limits shared by the reader, writer and offline importer.
pub const MAX_FEATURES: usize = 50_000;
/// Maximum points owned by a single polyline or simple polygon.
pub const MAX_FEATURE_POINTS: usize = 512;
/// Maximum points across the entire regional database.
pub const MAX_POINTS: usize = 1_000_000;
/// Maximum triangle index entries across the database.
pub const MAX_INDICES: usize = 3_000_000;
/// Upper bound for one spatial query's returned references.
pub const MAX_QUERY_FEATURES: usize = 4_096;
/// Conservative total allowance for quadratic polygon validation predicates.
/// A complete database is rejected before polygon validation when exceeded.
pub const MAX_VALIDATION_WORK: usize = 50_000_000;
/// This is a regional format, limited to 200 km from its first coordinate.
pub const MAX_REGION_RADIUS: Meters = Meters(200_000.0);
/// Required credit for OSM-derived scenery.
pub const OSM_ATTRIBUTION: &str = "Map data © OpenStreetMap contributors";
/// OSM source, credit and database licence information.
pub const OSM_COPYRIGHT_URL: &str = "https://www.openstreetmap.org/copyright";
/// Licence applying to OSM-derived scenery databases, separately from code.
pub const ODBL_URL: &str = "https://opendatacommons.org/licenses/odbl/1-0/";

/// Whether coordinates are sourced geography or an explicitly synthetic test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScenerySourceKind {
    OpenStreetMap = 1,
    Synthetic = 2,
}

/// Bounded source identity. No addresses, names of residents or contributor metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenerySource {
    pub kind: ScenerySourceKind,
    pub name: String,
    pub url: String,
    /// FNV-1a of the exact input file; fixture manifests also carry SHA-256.
    pub input_fingerprint: u64,
}

/// Simplified display classes, not a navigational road network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RoadClass {
    Motorway = 1,
    Primary = 2,
    Secondary = 3,
    Residential = 4,
    Service = 5,
    Track = 6,
    Path = 7,
}

/// Why a building extrusion has its stated height. Only OsmHeight is direct height data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BuildingHeightSource {
    OsmHeight = 1,
    OsmLevels = 2,
    Default = 3,
}

/// Selected OSM landuse/natural classes. Coverage is incomplete, not a world land mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LandCoverClass {
    Forest = 1,
    Grass = 2,
    Farmland = 3,
    Residential = 4,
    Industrial = 5,
    Water = 6,
    Bare = 7,
    Rock = 8,
}

/// An original OSM way's complete centreline (no inferred extension).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneryRoad {
    pub source_id: i64,
    pub class: RoadClass,
    pub width: Meters,
    pub width_inferred: bool,
    pub points: Vec<Geodetic>,
}

/// A simple building footprint, with a display extrusion height above sampled ground.
/// Rings are open (first point is not repeated); triangle indices address footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneryBuilding {
    pub source_id: i64,
    pub height: Meters,
    pub height_source: BuildingHeightSource,
    pub footprint: Vec<Geodetic>,
    pub triangles: Vec<[u32; 3]>,
}

/// A simple landcover polygon. Multipolygon holes are not flattened into filled land.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneryLandCover {
    pub source_id: i64,
    pub class: LandCoverClass,
    pub boundary: Vec<Geodetic>,
    pub triangles: Vec<[u32; 3]>,
}

/// Conservative bounding sphere in zero-altitude ECEF, not an elevation bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneryBounds {
    pub center: Geodetic,
    pub radius: Meters,
}

impl SceneryBounds {
    /// Invalid queries cannot select data. Altitude is intentionally ignored.
    #[must_use]
    pub fn intersects(self, center: Geodetic, radius: Meters) -> bool {
        if !valid_horizontal(center) || !radius.is_finite() || radius.get() < 0.0 {
            return false;
        }
        let center = Geodetic {
            altitude: Meters::ZERO,
            ..center
        };
        self.center.to_ecef().distance_to(center.to_ecef()).get()
            <= self.radius.get() + radius.get()
    }
}

/// Borrowed, immutable feature returned by a bounded geographic query.
#[derive(Debug, Clone, Copy)]
pub enum SceneryFeatureRef<'a> {
    Road(&'a SceneryRoad),
    Building(&'a SceneryBuilding),
    LandCover(&'a SceneryLandCover),
}

/// Invalid, oversized, corrupt or unsupported scenery is rejected before exposure.
#[derive(Debug)]
pub enum SceneryError {
    Invalid(&'static str),
    Limit(&'static str),
    Io(std::io::Error),
}
impl fmt::Display for SceneryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid scenery: {s}"),
            Self::Limit(s) => write!(f, "scenery safety limit exceeded: {s}"),
            Self::Io(e) => write!(f, "scenery I/O: {e}"),
        }
    }
}
impl std::error::Error for SceneryError {}
impl From<std::io::Error> for SceneryError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A fully validated, immutable regional scenery database.
#[derive(Debug, Clone)]
pub struct SceneryDatabase {
    source: ScenerySource,
    roads: Vec<SceneryRoad>,
    buildings: Vec<SceneryBuilding>,
    landcover: Vec<SceneryLandCover>,
    bounds: SceneryBounds,
    feature_bounds: Vec<SceneryBounds>,
}

impl SceneryDatabase {
    /// Validate data and canonical source-ID ordering. Duplicate IDs within a class
    /// are rejected. Callers may drop their mutable input after construction.
    pub fn new(
        source: ScenerySource,
        mut roads: Vec<SceneryRoad>,
        mut buildings: Vec<SceneryBuilding>,
        mut landcover: Vec<SceneryLandCover>,
    ) -> Result<Self, SceneryError> {
        validate_source(&source)?;
        let count = roads
            .len()
            .checked_add(buildings.len())
            .and_then(|n| n.checked_add(landcover.len()))
            .ok_or(SceneryError::Limit("features"))?;
        if count == 0 {
            return Err(SceneryError::Invalid("empty database"));
        }
        if count > MAX_FEATURES {
            return Err(SceneryError::Limit("features"));
        }
        // Preflight the entire input before any quadratic geometry work. The
        // 3*n²+10*n charge covers ring pairs, all interior-edge/ring pairs,
        // midpoint-in-ring scans and linear geometry checks conservatively.
        let mut validation_work = 0usize;
        for n in buildings
            .iter()
            .map(|f| f.footprint.len())
            .chain(landcover.iter().map(|f| f.boundary.len()))
        {
            if n > MAX_FEATURE_POINTS {
                return Err(SceneryError::Limit("feature points"));
            }
            validation_work = validation_work
                .checked_add(3 * n * n + 10 * n)
                .filter(|&work| work <= MAX_VALIDATION_WORK)
                .ok_or(SceneryError::Limit("aggregate polygon validation work"))?;
        }
        roads.sort_unstable_by_key(|f| f.source_id);
        buildings.sort_unstable_by_key(|f| f.source_id);
        landcover.sort_unstable_by_key(|f| f.source_id);
        for ids in [
            roads.iter().map(|f| f.source_id).collect::<Vec<_>>(),
            buildings.iter().map(|f| f.source_id).collect(),
            landcover.iter().map(|f| f.source_id).collect(),
        ] {
            if ids.iter().any(|&id| id <= 0) || ids.windows(2).any(|p| p[0] == p[1]) {
                return Err(SceneryError::Invalid("nonpositive or duplicate source ID"));
            }
        }
        let mut point_count = 0usize;
        let mut index_count = 0usize;
        let mut feature_bounds = Vec::new();
        feature_bounds
            .try_reserve_exact(count)
            .map_err(|_| SceneryError::Limit("bounds allocation"))?;
        let mut origin: Option<Geodetic> = None;
        let mut radius = 0.0_f64;
        for road in &roads {
            if !road.width.is_finite() || !(0.5..=100.0).contains(&road.width.get()) {
                return Err(SceneryError::Invalid("road width"));
            }
            validate_points(&road.points, 2)?;
            accumulate(
                &road.points,
                &mut point_count,
                &mut origin,
                &mut radius,
                &mut feature_bounds,
            )?;
        }
        for building in &buildings {
            if !building.height.is_finite() || !(1.0..=500.0).contains(&building.height.get()) {
                return Err(SceneryError::Invalid("building height"));
            }
            validate_polygon(&building.footprint, &building.triangles)?;
            index_count = index_count
                .checked_add(building.triangles.len() * 3)
                .ok_or(SceneryError::Limit("indices"))?;
            accumulate(
                &building.footprint,
                &mut point_count,
                &mut origin,
                &mut radius,
                &mut feature_bounds,
            )?;
        }
        for polygon in &landcover {
            validate_polygon(&polygon.boundary, &polygon.triangles)?;
            index_count = index_count
                .checked_add(polygon.triangles.len() * 3)
                .ok_or(SceneryError::Limit("indices"))?;
            accumulate(
                &polygon.boundary,
                &mut point_count,
                &mut origin,
                &mut radius,
                &mut feature_bounds,
            )?;
        }
        if index_count > MAX_INDICES {
            return Err(SceneryError::Limit("indices"));
        }
        Ok(Self {
            source,
            roads,
            buildings,
            landcover,
            bounds: SceneryBounds {
                center: origin.ok_or(SceneryError::Invalid("no coordinates"))?,
                radius: Meters(radius),
            },
            feature_bounds,
        })
    }
    #[must_use]
    pub fn source(&self) -> &ScenerySource {
        &self.source
    }
    #[must_use]
    pub fn roads(&self) -> &[SceneryRoad] {
        &self.roads
    }
    #[must_use]
    pub fn buildings(&self) -> &[SceneryBuilding] {
        &self.buildings
    }
    #[must_use]
    pub fn landcover(&self) -> &[SceneryLandCover] {
        &self.landcover
    }
    #[must_use]
    pub fn bounds(&self) -> SceneryBounds {
        self.bounds
    }
    #[must_use]
    pub fn feature_count(&self) -> usize {
        self.feature_bounds.len()
    }
    /// Scans at most MAX_FEATURES bounds, returns at most min(limit, MAX_QUERY_FEATURES).
    /// After conservative broad-phase rejection, rank/filter by point-to-line or
    /// polygon distance in the query's local tangent plane. Polygon interiors
    /// have distance zero. Ties use class/source ID. Temporary candidates and
    /// examined points are bounded by MAX_FEATURES and MAX_POINTS respectively.
    #[must_use]
    pub fn query_near(
        &self,
        center: Geodetic,
        radius: Meters,
        limit: usize,
    ) -> Vec<SceneryFeatureRef<'_>> {
        if limit == 0 || !self.bounds.intersects(center, radius) {
            return Vec::new();
        }
        let limit = limit.min(MAX_QUERY_FEATURES);
        let center_ecef = Geodetic {
            altitude: Meters::ZERO,
            ..center
        }
        .to_ecef();
        let frame = LocalFrame::new(Geodetic {
            altitude: Meters::ZERO,
            ..center
        });
        let mut candidates: Vec<_> = self
            .roads
            .iter()
            .map(SceneryFeatureRef::Road)
            .chain(self.buildings.iter().map(SceneryFeatureRef::Building))
            .chain(self.landcover.iter().map(SceneryFeatureRef::LandCover))
            .zip(&self.feature_bounds)
            .enumerate()
            .filter_map(|(order, (feature, bounds))| {
                let broad_distance = (bounds.center.to_ecef().distance_to(center_ecef).get()
                    - bounds.radius.get())
                .max(0.0);
                if broad_distance > radius.get() {
                    return None;
                }
                let (points, closed) = match feature {
                    SceneryFeatureRef::Road(f) => (f.points.as_slice(), false),
                    SceneryFeatureRef::Building(f) => (f.footprint.as_slice(), true),
                    SceneryFeatureRef::LandCover(f) => (f.boundary.as_slice(), true),
                };
                let distance = geometry_distance(points, closed, &frame);
                (distance <= radius.get()).then_some((distance, order, feature))
            })
            .collect();
        let compare = |a: &(f64, usize, SceneryFeatureRef<'_>),
                       b: &(f64, usize, SceneryFeatureRef<'_>)| {
            a.0.total_cmp(&b.0).then(a.1.cmp(&b.1))
        };
        if candidates.len() > limit {
            candidates.select_nth_unstable_by(limit, compare);
            candidates.truncate(limit);
        }
        candidates.sort_unstable_by(compare);
        candidates
            .into_iter()
            .map(|(_, _, feature)| feature)
            .collect()
    }
}

fn validate_source(source: &ScenerySource) -> Result<(), SceneryError> {
    for text in [&source.name, &source.url] {
        if text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control) {
            return Err(SceneryError::Invalid("source metadata"));
        }
    }
    if source.kind == ScenerySourceKind::OpenStreetMap && !source.url.starts_with("https://") {
        return Err(SceneryError::Invalid(
            "OSM source must have an HTTPS provenance URL",
        ));
    }
    Ok(())
}
fn valid_horizontal(p: Geodetic) -> bool {
    p.latitude.is_finite()
        && p.longitude.is_finite()
        && p.altitude.is_finite()
        && p.latitude.get().abs() <= std::f64::consts::FRAC_PI_2
        && p.longitude.get().abs() <= std::f64::consts::PI
}
fn validate_points(points: &[Geodetic], minimum: usize) -> Result<(), SceneryError> {
    if points.len() < minimum {
        return Err(SceneryError::Invalid("too few points"));
    }
    if points.len() > MAX_FEATURE_POINTS {
        return Err(SceneryError::Limit("feature points"));
    }
    if points
        .iter()
        .any(|&p| !valid_horizontal(p) || p.altitude.get().abs() > 0.0)
    {
        return Err(SceneryError::Invalid(
            "coordinates must be finite horizontal WGS84, altitude zero",
        ));
    }
    if points
        .windows(2)
        .any(|p| p[0].to_ecef().distance_to(p[1].to_ecef()).get() < 0.001)
    {
        return Err(SceneryError::Invalid("duplicate adjacent points"));
    }
    Ok(())
}
// Coordinates stay in core; this module only performs ordinary planar geometry
// in a core-provided local frame. n<=512 makes the quadratic validation explicit.
type Point2 = [f64; 2];
const GEOMETRY_EPSILON: f64 = 1e-8;
fn projected(point: Geodetic, frame: &LocalFrame) -> Point2 {
    let p = frame.ecef_to_ned_position(point.to_ecef());
    [p.east(), p.north()]
}
fn orientation(a: Point2, b: Point2, c: Point2) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn on_segment(p: Point2, a: Point2, b: Point2) -> bool {
    orientation(a, b, p).abs() <= GEOMETRY_EPSILON
        && p[0] >= a[0].min(b[0]) - GEOMETRY_EPSILON
        && p[0] <= a[0].max(b[0]) + GEOMETRY_EPSILON
        && p[1] >= a[1].min(b[1]) - GEOMETRY_EPSILON
        && p[1] <= a[1].max(b[1]) + GEOMETRY_EPSILON
}
fn segments_intersect(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let o = [
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    ];
    (o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0)
        || on_segment(c, a, b)
        || on_segment(d, a, b)
        || on_segment(a, c, d)
        || on_segment(b, c, d)
}
fn point_in_ring(p: Point2, ring: &[Point2]) -> bool {
    let mut inside = false;
    for (&a, &b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        if on_segment(p, a, b) {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}
fn ordered_edge(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}
fn validate_polygon(points: &[Geodetic], triangles: &[[u32; 3]]) -> Result<(), SceneryError> {
    validate_points(points, 3)?;
    if points[0]
        .to_ecef()
        .distance_to(points[points.len() - 1].to_ecef())
        .get()
        < 0.001
    {
        return Err(SceneryError::Invalid(
            "polygon ring must omit repeated endpoint",
        ));
    }
    if triangles.len() != points.len() - 2 {
        return Err(SceneryError::Invalid("simple polygon triangle count"));
    }
    let frame = LocalFrame::new(points[0]);
    let xy: Vec<_> = points.iter().map(|&p| projected(p, &frame)).collect();
    let n = xy.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_intersect(xy[i], xy[(i + 1) % n], xy[j], xy[(j + 1) % n]) {
                return Err(SceneryError::Invalid("self-intersecting polygon ring"));
            }
        }
    }
    let area = xy
        .iter()
        .zip(xy.iter().cycle().skip(1))
        .take(n)
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        .abs()
        * 0.5;
    if !area.is_finite() || area < 0.01 {
        return Err(SceneryError::Invalid("degenerate polygon area"));
    }
    let mut edges: BTreeMap<(u32, u32), (u8, i8)> = BTreeMap::new();
    let mut unique_triangles = BTreeSet::new();
    let mut triangle_area = 0.0;
    let mut winding = None;
    for triangle in triangles {
        let mut key = *triangle;
        key.sort_unstable();
        if !unique_triangles.insert(key) {
            return Err(SceneryError::Invalid("duplicate triangle"));
        }
        let mut p = [[0.0; 2]; 3];
        for (out, &index) in p.iter_mut().zip(triangle) {
            *out = *xy
                .get(index as usize)
                .ok_or(SceneryError::Invalid("triangle index"))?;
        }
        let signed = orientation(p[0], p[1], p[2]);
        if signed.abs() <= GEOMETRY_EPSILON {
            return Err(SceneryError::Invalid("degenerate triangle"));
        }
        let sign = signed.is_sign_positive();
        if winding.is_some_and(|old| old != sign) {
            return Err(SceneryError::Invalid("inconsistent triangle winding"));
        }
        winding = Some(sign);
        triangle_area += signed.abs() * 0.5;
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let edge = edges.entry(ordered_edge(a, b)).or_default();
            edge.0 += 1;
            edge.1 += if a < b { 1 } else { -1 };
            if edge.0 > 2 {
                return Err(SceneryError::Invalid("nonmanifold triangle edge"));
            }
        }
    }
    if (triangle_area - area).abs() > area * 1e-6 + 0.001 {
        return Err(SceneryError::Invalid("triangle coverage area"));
    }
    let boundary: BTreeSet<_> = (0..n)
        .map(|i| {
            ordered_edge(
                u32::try_from(i).expect("512 point bound"),
                u32::try_from((i + 1) % n).expect("512 point bound"),
            )
        })
        .collect();
    for edge in &boundary {
        if edges.get(edge).is_none_or(|&(count, _)| count != 1) {
            return Err(SceneryError::Invalid("uncovered polygon boundary"));
        }
    }
    for (&(a, b), &(count, direction)) in &edges {
        if boundary.contains(&(a, b)) {
            continue;
        }
        if count != 2 || direction != 0 {
            return Err(SceneryError::Invalid("unpaired interior triangle edge"));
        }
        let a = a as usize;
        let b = b as usize;
        for i in 0..n {
            let j = (i + 1) % n;
            if a == i || a == j || b == i || b == j {
                continue;
            }
            if segments_intersect(xy[a], xy[b], xy[i], xy[j]) {
                return Err(SceneryError::Invalid("triangle crosses polygon boundary"));
            }
        }
        let midpoint = [(xy[a][0] + xy[b][0]) * 0.5, (xy[a][1] + xy[b][1]) * 0.5];
        if !point_in_ring(midpoint, &xy) {
            return Err(SceneryError::Invalid("triangle outside polygon"));
        }
    }
    Ok(())
}
fn geometry_distance(points: &[Geodetic], closed: bool, frame: &LocalFrame) -> f64 {
    let xy: Vec<_> = points.iter().map(|&p| projected(p, frame)).collect();
    if closed && point_in_ring([0.0, 0.0], &xy) {
        return 0.0;
    }
    let count = if closed { xy.len() } else { xy.len() - 1 };
    let mut best = f64::INFINITY;
    for i in 0..count {
        let a = xy[i];
        let b = xy[(i + 1) % xy.len()];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length2 = dx * dx + dy * dy;
        let t = if length2 > 0.0 {
            (-(a[0] * dx + a[1] * dy) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        best = best.min((a[0] + t * dx).hypot(a[1] + t * dy));
    }
    best
}

fn accumulate(
    points: &[Geodetic],
    count: &mut usize,
    origin: &mut Option<Geodetic>,
    radius: &mut f64,
    bounds: &mut Vec<SceneryBounds>,
) -> Result<(), SceneryError> {
    *count = count
        .checked_add(points.len())
        .ok_or(SceneryError::Limit("points"))?;
    if *count > MAX_POINTS {
        return Err(SceneryError::Limit("points"));
    }
    let regional_origin = *origin.get_or_insert(points[0]);
    let mut feature_radius = 0.0_f64;
    for &p in points {
        *radius = radius.max(regional_origin.to_ecef().distance_to(p.to_ecef()).get());
        feature_radius = feature_radius.max(points[0].to_ecef().distance_to(p.to_ecef()).get());
    }
    if *radius > MAX_REGION_RADIUS.get() {
        return Err(SceneryError::Limit("regional extent"));
    }
    bounds.push(SceneryBounds {
        center: points[0],
        radius: Meters(feature_radius),
    });
    Ok(())
}
