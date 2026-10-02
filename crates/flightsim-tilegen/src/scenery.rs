//! Bounded offline OSM surface extraction. Only geometric/classification tags are
//! retained. No personal contributor metadata, names or addresses are exported.
//! v1 supports simple way polygons; multipolygon member ways are suppressed so
//! holes cannot accidentally become solid filled building/landcover polygons.

use flightsim_core::{Feet, Geodetic, LocalFrame, Meters};
use flightsim_world::scenery::{MAX_FEATURE_POINTS, MAX_FEATURES, MAX_POINTS};
use flightsim_world::{
    BuildingHeightSource, LandCoverClass, RoadClass, SceneryBuilding, SceneryDatabase,
    SceneryLandCover, SceneryRoad, ScenerySource, ScenerySourceKind,
};
use osmpbf::{BlobDecode, BlobReader, BlobType, Element, RelMemberType};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

/// Exact source bytes are retained once for deterministic two-pass processing.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// The safe PBF parser separately limits each decompressed block to less than32MiB.
pub const MAX_PBF_BLOCKS: usize = 512;
/// Cumulative uncompressed bytes permitted per pass, charged before decoding.
pub const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
/// Each pass stops before processing more elements than this.
pub const MAX_PBF_ELEMENTS: usize = 5_000_000;
/// Source references and polygon-relation members each have this hard cap.
pub const MAX_REFERENCES: usize = 1_000_000;

/// Input identity supplied by the caller; the fingerprint is computed from bytes.
#[derive(Debug, Clone)]
pub struct SceneryBakeOptions {
    pub source_name: String,
    pub source_url: String,
}

/// Honest extraction counts. Unsupported features are omitted, never invented.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneryGenerationReport {
    pub source_bytes: usize,
    pub source_fingerprint: u64,
    pub elements_seen: usize,
    pub selected_ways: usize,
    pub roads_written: usize,
    pub buildings_written: usize,
    pub landcover_written: usize,
    pub widths_inferred: usize,
    pub heights_from_levels: usize,
    pub heights_defaulted: usize,
    pub skipped_oversized_ways: usize,
    pub skipped_non_ground_roads: usize,
    pub skipped_raised_buildings: usize,
    pub skipped_area_roads: usize,
    pub skipped_open_polygons: usize,
    pub skipped_multipolygon_relations: usize,
    pub skipped_multipolygon_members: usize,
    pub skipped_missing_nodes: usize,
    pub skipped_invalid_coordinates: usize,
    pub skipped_bad_geometry: usize,
    pub referenced_nodes: usize,
}

#[derive(Debug)]
pub enum SceneryGenError {
    Io(std::io::Error),
    Pbf(String),
    Invalid(&'static str),
    Limit(&'static str),
    Database(flightsim_world::SceneryError),
}
impl std::fmt::Display for SceneryGenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "scenery bake I/O: {e}"),
            Self::Pbf(e) => write!(f, "scenery PBF: {e}"),
            Self::Invalid(s) => write!(f, "invalid scenery input: {s}"),
            Self::Limit(s) => write!(f, "scenery bake safety limit exceeded: {s}"),
            Self::Database(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for SceneryGenError {}
impl From<std::io::Error> for SceneryGenError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<flightsim_world::SceneryError> for SceneryGenError {
    fn from(e: flightsim_world::SceneryError) -> Self {
        Self::Database(e)
    }
}

/// Import and atomically replace a regional runtime database. The output is never
/// touched unless both PBF passes and complete database validation succeed.
pub fn generate_scenery_database(
    input: &Path,
    output: &Path,
    options: &SceneryBakeOptions,
) -> Result<SceneryGenerationReport, SceneryGenError> {
    match same_file::is_same_file(input, output) {
        Ok(true) => {
            return Err(SceneryGenError::Invalid(
                "input and output are the same file",
            ));
        }
        Ok(false) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut input_file = std::fs::File::open(input)?;
    let len = usize::try_from(input_file.metadata()?.len())
        .map_err(|_| SceneryGenError::Limit("source bytes"))?;
    if len == 0 || len > MAX_INPUT_BYTES {
        return Err(SceneryGenError::Limit("source bytes (must be1..64MiB)"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| SceneryGenError::Limit("source allocation"))?;
    bytes.resize(len, 0);
    input_file.read_exact(&mut bytes)?;
    if input_file.read(&mut [0u8; 1])? != 0 {
        return Err(SceneryGenError::Invalid("input changed size during read"));
    }
    let (database, report) = bake_scenery_bytes(&bytes, options)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".flightsim-scenerygen-")
        .tempfile_in(parent)?;
    database.write(temporary.as_file_mut())?;
    temporary.flush()?;
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist(output)
        .map_err(|e| SceneryGenError::Io(e.error))?;
    Ok(report)
}

/// Same deterministic importer for bounded in-memory inputs and fixture tests.
pub fn bake_scenery_bytes(
    bytes: &[u8],
    options: &SceneryBakeOptions,
) -> Result<(SceneryDatabase, SceneryGenerationReport), SceneryGenError> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err(SceneryGenError::Limit("source bytes"));
    }
    let mut report = SceneryGenerationReport {
        source_bytes: bytes.len(),
        source_fingerprint: flightsim_world::scenery::io::fingerprint(bytes),
        ..SceneryGenerationReport::default()
    };
    let mut candidates = Vec::new();
    let mut relation_members = HashSet::new();
    let mut reference_count = 0usize;
    let mut relation_reference_count = 0usize;
    report.elements_seen = scan(bytes, |element| {
        match element {
            Element::Way(way) => {
                let tags = Tags::read(way.tags());
                let Some(kind) = classify(&tags) else {
                    return Ok(());
                };
                report.selected_ways += 1;
                if matches!(kind, Kind::Road(_)) {
                    if tags.non_ground() {
                        report.skipped_non_ground_roads += 1;
                        return Ok(());
                    }
                    if tags.area == Some("yes") {
                        report.skipped_area_roads += 1;
                        return Ok(());
                    }
                }
                if matches!(kind, Kind::Building)
                    && tags
                        .min_height
                        .and_then(parse_length)
                        .is_some_and(|v| v > 0.0)
                {
                    report.skipped_raised_buildings += 1;
                    return Ok(());
                }
                let count = way.refs().take(MAX_FEATURE_POINTS + 2).count();
                let polygon = !matches!(kind, Kind::Road(_));
                if count > MAX_FEATURE_POINTS + usize::from(polygon) {
                    report.skipped_oversized_ways += 1;
                    return Ok(());
                }
                if candidates.len() >= MAX_FEATURES {
                    return Err(SceneryGenError::Limit("candidate features"));
                }
                reference_count = reference_count
                    .checked_add(count)
                    .filter(|n| *n <= MAX_REFERENCES)
                    .ok_or(SceneryGenError::Limit("candidate node references"))?;
                let mut refs = Vec::new();
                refs.try_reserve_exact(count)
                    .map_err(|_| SceneryGenError::Limit("way reference allocation"))?;
                refs.extend(way.refs());
                if polygon && (refs.len() < 4 || refs.first() != refs.last()) {
                    report.skipped_open_polygons += 1;
                    return Ok(());
                }
                let (width, width_inferred) = road_width(tags.width, kind);
                let (height, height_source) = building_height(tags.height, tags.levels);
                candidates
                    .try_reserve(1)
                    .map_err(|_| SceneryGenError::Limit("candidate allocation"))?;
                candidates.push(Candidate {
                    id: way.id(),
                    kind,
                    refs,
                    width,
                    width_inferred,
                    height,
                    height_source,
                });
            }
            Element::Relation(relation) => {
                let tags = Tags::read(relation.tags());
                if tags.relation_type == Some("multipolygon") {
                    report.skipped_multipolygon_relations += 1;
                    for member in relation.members() {
                        relation_reference_count += 1;
                        if relation_reference_count > MAX_REFERENCES {
                            return Err(SceneryGenError::Limit("total multipolygon references"));
                        }
                        if member.member_type == RelMemberType::Way {
                            if relation_members.len() >= MAX_REFERENCES {
                                return Err(SceneryGenError::Limit("multipolygon members"));
                            }
                            relation_members.try_reserve(1).map_err(|_| {
                                SceneryGenError::Limit("relation member allocation")
                            })?;
                            relation_members.insert(member.member_id);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if report.elements_seen == 0 {
        return Err(SceneryGenError::Invalid("PBF has no elements"));
    }
    candidates.sort_unstable_by_key(|f| f.id);
    if candidates.windows(2).any(|p| p[0].id == p[1].id) {
        return Err(SceneryGenError::Invalid("duplicate selected way IDs"));
    }
    candidates.retain(|f| {
        let keep = matches!(f.kind, Kind::Road(_)) || !relation_members.contains(&f.id);
        if !keep {
            report.skipped_multipolygon_members += 1;
        }
        keep
    });
    drop(relation_members);
    let mut wanted = HashSet::new();
    wanted
        .try_reserve(reference_count)
        .map_err(|_| SceneryGenError::Limit("node set allocation"))?;
    for candidate in &candidates {
        wanted.extend(candidate.refs.iter().copied());
    }
    if wanted.len() > MAX_POINTS {
        return Err(SceneryGenError::Limit("referenced nodes"));
    }
    let mut nodes = HashMap::new();
    nodes
        .try_reserve(wanted.len())
        .map_err(|_| SceneryGenError::Limit("node coordinate allocation"))?;
    scan(bytes, |element| {
        let node = match element {
            Element::Node(n) => Some((n.id(), n.lat(), n.lon())),
            Element::DenseNode(n) => Some((n.id(), n.lat(), n.lon())),
            _ => None,
        };
        if let Some((id, latitude, longitude)) = node {
            if wanted.contains(&id) && nodes.insert(id, (latitude, longitude)).is_some() {
                return Err(SceneryGenError::Invalid("duplicate referenced node IDs"));
            }
        }
        Ok(())
    })?;
    report.referenced_nodes = nodes.len();
    let mut roads = Vec::new();
    let mut buildings = Vec::new();
    let mut landcover = Vec::new();
    let mut output_points = 0usize;
    for candidate in candidates {
        if candidate.refs.iter().any(|id| !nodes.contains_key(id)) {
            report.skipped_missing_nodes += 1;
            continue;
        }
        if candidate.refs.iter().any(|id| {
            let &(lat, lon) = &nodes[id];
            !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
        }) {
            report.skipped_invalid_coordinates += 1;
            continue;
        }
        let mut points: Vec<_> = candidate
            .refs
            .iter()
            .map(|id| {
                let &(lat, lon) = &nodes[id];
                Geodetic::from_degrees(lat, lon, 0.0)
            })
            .collect();
        if !matches!(candidate.kind, Kind::Road(_)) {
            points.pop();
        }
        if points.len() < 2
            || points
                .windows(2)
                .any(|p| p[0].to_ecef().distance_to(p[1].to_ecef()).get() < 0.001)
        {
            report.skipped_bad_geometry += 1;
            continue;
        }
        output_points = output_points
            .checked_add(points.len())
            .filter(|n| *n <= MAX_POINTS)
            .ok_or(SceneryGenError::Limit("output points"))?;
        match candidate.kind {
            Kind::Road(class) => {
                report.widths_inferred += usize::from(candidate.width_inferred);
                roads.push(SceneryRoad {
                    source_id: candidate.id,
                    class,
                    width: Meters(candidate.width),
                    width_inferred: candidate.width_inferred,
                    points,
                });
            }
            Kind::Building => {
                let Some(triangles) = triangulate(&points) else {
                    report.skipped_bad_geometry += 1;
                    continue;
                };
                report.heights_defaulted +=
                    usize::from(candidate.height_source == BuildingHeightSource::Default);
                report.heights_from_levels +=
                    usize::from(candidate.height_source == BuildingHeightSource::OsmLevels);
                buildings.push(SceneryBuilding {
                    source_id: candidate.id,
                    height: Meters(candidate.height),
                    height_source: candidate.height_source,
                    footprint: points,
                    triangles,
                });
            }
            Kind::Land(class) => {
                let Some(triangles) = triangulate(&points) else {
                    report.skipped_bad_geometry += 1;
                    continue;
                };
                landcover.push(SceneryLandCover {
                    source_id: candidate.id,
                    class,
                    boundary: points,
                    triangles,
                });
            }
        }
    }
    report.roads_written = roads.len();
    report.buildings_written = buildings.len();
    report.landcover_written = landcover.len();
    let source = ScenerySource {
        kind: ScenerySourceKind::OpenStreetMap,
        name: options.source_name.clone(),
        url: options.source_url.clone(),
        input_fingerprint: report.source_fingerprint,
    };
    let database = SceneryDatabase::new(source, roads, buildings, landcover)?;
    Ok((database, report))
}

fn scan(
    bytes: &[u8],
    mut callback: impl for<'a> FnMut(Element<'a>) -> Result<(), SceneryGenError>,
) -> Result<usize, SceneryGenError> {
    let mut elements = 0usize;
    let mut decoded_bytes = 0usize;
    let mut header_seen = false;
    for (index, blob) in BlobReader::new(std::io::Cursor::new(bytes)).enumerate() {
        if index >= MAX_PBF_BLOCKS {
            return Err(SceneryGenError::Limit("PBF block count"));
        }
        let blob = blob.map_err(|e| SceneryGenError::Pbf(e.to_string()))?;
        if !matches!(blob.get_type(), BlobType::Unknown(_)) {
            let size = blob
                .uncompressed_size()
                .map_err(|e| SceneryGenError::Pbf(e.to_string()))?;
            decoded_bytes = charge_decoded_bytes(decoded_bytes, size)?;
        }
        match blob
            .decode()
            .map_err(|e| SceneryGenError::Pbf(e.to_string()))?
        {
            BlobDecode::OsmHeader(_) => header_seen = true,
            BlobDecode::OsmData(block) => {
                if !header_seen {
                    return Err(SceneryGenError::Invalid("PBF data before header"));
                }
                for element in block.elements() {
                    elements += 1;
                    if elements > MAX_PBF_ELEMENTS {
                        return Err(SceneryGenError::Limit("PBF elements"));
                    }
                    callback(element)?;
                }
            }
            BlobDecode::Unknown(_) => {}
        }
    }
    if !header_seen {
        return Err(SceneryGenError::Invalid("PBF header missing"));
    }
    Ok(elements)
}

fn charge_decoded_bytes(previous: usize, next: usize) -> Result<usize, SceneryGenError> {
    previous
        .checked_add(next)
        .filter(|&n| n <= MAX_DECODED_BYTES)
        .ok_or(SceneryGenError::Limit("cumulative decompressed PBF bytes"))
}
#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn cumulative_decompression_is_charged_before_decode() {
        assert_eq!(
            charge_decoded_bytes(MAX_DECODED_BYTES - 1, 1).unwrap(),
            MAX_DECODED_BYTES
        );
        assert!(charge_decoded_bytes(MAX_DECODED_BYTES, 1).is_err());
        assert!(charge_decoded_bytes(usize::MAX, 1).is_err());
    }
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Road(RoadClass),
    Building,
    Land(LandCoverClass),
}
#[derive(Debug)]
struct Candidate {
    id: i64,
    kind: Kind,
    refs: Vec<i64>,
    width: f64,
    width_inferred: bool,
    height: f64,
    height_source: BuildingHeightSource,
}
#[derive(Default)]
struct Tags<'a> {
    highway: Option<&'a str>,
    building: Option<&'a str>,
    landuse: Option<&'a str>,
    natural: Option<&'a str>,
    leisure: Option<&'a str>,
    width: Option<&'a str>,
    height: Option<&'a str>,
    levels: Option<&'a str>,
    min_height: Option<&'a str>,
    area: Option<&'a str>,
    bridge: Option<&'a str>,
    tunnel: Option<&'a str>,
    layer: Option<&'a str>,
    relation_type: Option<&'a str>,
}
impl<'a> Tags<'a> {
    fn read(tags: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        let mut t = Self::default();
        for (k, v) in tags {
            let slot = match k {
                "highway" => &mut t.highway,
                "building" => &mut t.building,
                "landuse" => &mut t.landuse,
                "natural" => &mut t.natural,
                "leisure" => &mut t.leisure,
                "width" => &mut t.width,
                "height" => &mut t.height,
                "building:levels" => &mut t.levels,
                "min_height" => &mut t.min_height,
                "area" => &mut t.area,
                "bridge" => &mut t.bridge,
                "tunnel" => &mut t.tunnel,
                "layer" => &mut t.layer,
                "type" => &mut t.relation_type,
                _ => continue,
            };
            if slot.is_none() {
                *slot = Some(v);
            }
        }
        t
    }
    fn non_ground(&self) -> bool {
        self.bridge.is_some_and(|v| v != "no")
            || self.tunnel.is_some_and(|v| v != "no")
            || self.layer.is_some_and(|v| v.trim() != "0")
    }
}
fn classify(t: &Tags<'_>) -> Option<Kind> {
    if t.building
        .is_some_and(|v| v != "no" && v != "construction" && v != "ruins")
    {
        return Some(Kind::Building);
    }
    let road = match t.highway {
        Some("motorway" | "motorway_link" | "trunk" | "trunk_link") => Some(RoadClass::Motorway),
        Some("primary" | "primary_link") => Some(RoadClass::Primary),
        Some("secondary" | "secondary_link" | "tertiary" | "tertiary_link") => {
            Some(RoadClass::Secondary)
        }
        Some("residential" | "living_street" | "unclassified") => Some(RoadClass::Residential),
        Some("service") => Some(RoadClass::Service),
        Some("track") => Some(RoadClass::Track),
        Some("path" | "footway" | "cycleway" | "pedestrian" | "steps" | "bridleway") => {
            Some(RoadClass::Path)
        }
        _ => None,
    };
    if let Some(class) = road {
        return Some(Kind::Road(class));
    }
    let class = match (t.landuse, t.natural, t.leisure) {
        (Some("forest"), _, _) | (_, Some("wood"), _) => LandCoverClass::Forest,
        (Some("grass" | "meadow" | "recreation_ground" | "village_green"), _, _)
        | (_, Some("grassland" | "heath" | "scrub"), _)
        | (_, _, Some("park" | "garden" | "pitch" | "golf_course")) => LandCoverClass::Grass,
        (Some("farmland" | "farmyard" | "orchard" | "vineyard" | "allotments"), _, _) => {
            LandCoverClass::Farmland
        }
        (Some("residential"), _, _) => LandCoverClass::Residential,
        (Some("industrial" | "commercial" | "retail"), _, _) => LandCoverClass::Industrial,
        (Some("reservoir" | "basin"), _, _) | (_, Some("water"), _) => LandCoverClass::Water,
        (Some("quarry" | "brownfield" | "construction"), _, _)
        | (_, Some("sand" | "beach" | "scree" | "shingle"), _) => LandCoverClass::Bare,
        (_, Some("bare_rock"), _) => LandCoverClass::Rock,
        _ => return None,
    };
    Some(Kind::Land(class))
}
fn parse_length(text: &str) -> Option<f64> {
    let text = text.trim();
    let (number, feet) =
        if let Some(v) = text.strip_suffix("ft").or_else(|| text.strip_suffix('\'')) {
            (v, true)
        } else {
            (text.strip_suffix('m').unwrap_or(text), false)
        };
    let value = number.trim().parse::<f64>().ok()?;
    let meters = if feet {
        Feet(value).to_meters().get()
    } else {
        value
    };
    (meters.is_finite() && meters > 0.0).then_some(meters)
}
fn road_width(text: Option<&str>, kind: Kind) -> (f64, bool) {
    if let Some(width) = text
        .and_then(parse_length)
        .filter(|w| (0.5..=100.0).contains(w))
    {
        return (width, false);
    }
    (
        match kind {
            Kind::Road(RoadClass::Motorway) => 12.0,
            Kind::Road(RoadClass::Primary) => 9.0,
            Kind::Road(RoadClass::Secondary) => 7.0,
            Kind::Road(RoadClass::Residential) => 5.5,
            Kind::Road(RoadClass::Service) => 4.0,
            Kind::Road(RoadClass::Track) => 3.0,
            _ => 1.5,
        },
        true,
    )
}
fn building_height(height: Option<&str>, levels: Option<&str>) -> (f64, BuildingHeightSource) {
    if let Some(height) = height
        .and_then(parse_length)
        .filter(|h| (1.0..=500.0).contains(h))
    {
        return (height, BuildingHeightSource::OsmHeight);
    }
    if let Some(levels) = levels
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite() && (1.0..=166.0).contains(n))
    {
        return (levels * 3.0, BuildingHeightSource::OsmLevels);
    }
    (9.0, BuildingHeightSource::Default)
}

fn triangulate(points: &[Geodetic]) -> Option<Vec<[u32; 3]>> {
    if points.len() < 3 || points.len() > MAX_FEATURE_POINTS {
        return None;
    }
    let frame = LocalFrame::new(points[0]);
    let xy: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            let v = frame.ecef_to_ned_position(p.to_ecef());
            [v.east(), v.north()]
        })
        .collect();
    let n = xy.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if intersect(xy[i], xy[(i + 1) % n], xy[j], xy[(j + 1) % n]) {
                return None;
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
        / 2.0;
    if !area.is_finite() || area < 0.01 {
        return None;
    }
    let flat: Vec<_> = xy.iter().flatten().copied().collect();
    let indices = earcutr::earcut(&flat, &[], 2).ok()?;
    if indices.len() != (n - 2) * 3 {
        return None;
    }
    let mut sum = 0.0;
    let mut triangles = Vec::new();
    for t in indices.chunks_exact(3) {
        let a = *xy.get(t[0])?;
        let b = *xy.get(t[1])?;
        let c = *xy.get(t[2])?;
        let triangle_area = orient(a, b, c).abs() / 2.0;
        if triangle_area < 1e-6 {
            return None;
        }
        sum += triangle_area;
        triangles.push([
            u32::try_from(t[0]).ok()?,
            u32::try_from(t[1]).ok()?,
            u32::try_from(t[2]).ok()?,
        ]);
    }
    ((sum - area).abs() <= area * 1e-6 + 0.001).then_some(triangles)
}
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o = [
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    ];
    if o[0] * o[1] < 0.0 && o[2] * o[3] < 0.0 {
        return true;
    }
    let on = |p: [f64; 2], x: [f64; 2], y: [f64; 2]| {
        p[0] >= x[0].min(y[0]) - 1e-8
            && p[0] <= x[0].max(y[0]) + 1e-8
            && p[1] >= x[1].min(y[1]) - 1e-8
            && p[1] <= x[1].max(y[1]) + 1e-8
    };
    (o[0].abs() < 1e-8 && on(c, a, b))
        || (o[1].abs() < 1e-8 && on(d, a, b))
        || (o[2].abs() < 1e-8 && on(a, c, d))
        || (o[3].abs() < 1e-8 && on(b, c, d))
}
