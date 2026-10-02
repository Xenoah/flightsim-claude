//! FSSC v1 codec. Header sizes, aggregate counts and individual records are bounded
//! before allocation. Little endian, exact length and FNV-1a payload checksum.

use super::*;
use flightsim_core::Radians;
use std::io::{Read, Write};
use std::path::Path;

pub const MAGIC: [u8; 4] = *b"FSSC";
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 48;
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const ROAD_HEADER: usize = 28;
const BUILDING_HEADER: usize = 28;
const LANDCOVER_HEADER: usize = 20;

/// Stable non-cryptographic corruption/input identity checksum.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

impl SceneryDatabase {
    pub fn read_path(path: impl AsRef<Path>) -> Result<Self, SceneryError> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len()
            > u64::try_from(MAX_PAYLOAD_BYTES + HEADER_LEN).unwrap_or(u64::MAX)
        {
            return Err(SceneryError::Limit("file bytes"));
        }
        Self::read(std::io::BufReader::new(file))
    }
    pub fn read(mut reader: impl Read) -> Result<Self, SceneryError> {
        let mut header = [0u8; HEADER_LEN];
        reader.read_exact(&mut header)?;
        let mut h = Cursor::new(&header);
        if h.take(4)? != MAGIC {
            return Err(SceneryError::Invalid("magic"));
        }
        if h.u16()? != FORMAT_VERSION {
            return Err(SceneryError::Invalid("version"));
        }
        if h.u16()? != 0 {
            return Err(SceneryError::Invalid("header flags"));
        }
        let payload_len = h.count()?;
        let road_count = h.count()?;
        let building_count = h.count()?;
        let landcover_count = h.count()?;
        let point_count = h.count()?;
        let index_count = h.count()?;
        let metadata_len = h.count()?;
        if h.u32()? != 0 {
            return Err(SceneryError::Invalid("reserved header"));
        }
        let checksum = h.u64()?;
        let feature_count = road_count
            .checked_add(building_count)
            .and_then(|n| n.checked_add(landcover_count))
            .ok_or(SceneryError::Limit("features"))?;
        if feature_count == 0
            || feature_count > MAX_FEATURES
            || point_count > MAX_POINTS
            || index_count > MAX_INDICES
            || payload_len > MAX_PAYLOAD_BYTES
            || metadata_len > 8192
        {
            return Err(SceneryError::Limit("header counts"));
        }
        let expected = metadata_len
            .checked_add(road_count * ROAD_HEADER)
            .and_then(|n| n.checked_add(building_count * BUILDING_HEADER))
            .and_then(|n| n.checked_add(landcover_count * LANDCOVER_HEADER))
            .and_then(|n| n.checked_add(point_count * 16))
            .and_then(|n| n.checked_add(index_count * 4))
            .ok_or(SceneryError::Limit("payload size"))?;
        if expected != payload_len || index_count % 3 != 0 {
            return Err(SceneryError::Invalid("payload count relationship"));
        }
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(payload_len)
            .map_err(|_| SceneryError::Limit("payload allocation"))?;
        payload.resize(payload_len, 0);
        reader.read_exact(&mut payload)?;
        let mut extra = [0u8; 1];
        if reader.read(&mut extra)? != 0 {
            return Err(SceneryError::Invalid("trailing bytes"));
        }
        if fingerprint(&payload) != checksum {
            return Err(SceneryError::Invalid("checksum"));
        }
        let mut c = Cursor::new(&payload);
        let source = read_source(c.take(metadata_len)?)?;
        let mut roads = bounded_vec(road_count)?;
        let mut buildings = bounded_vec(building_count)?;
        let mut landcover = bounded_vec(landcover_count)?;
        let mut actual_points = 0usize;
        let mut actual_indices = 0usize;
        for _ in 0..road_count {
            let source_id = c.i64()?;
            let class = read_road_class(c.u8()?)?;
            let width_inferred = match c.u8()? {
                0 => false,
                1 => true,
                _ => return Err(SceneryError::Invalid("width provenance")),
            };
            if c.u16()? != 0 {
                return Err(SceneryError::Invalid("road flags"));
            }
            let width = Meters(c.f64()?);
            let count = c.count()?;
            if c.u32()? != 0 {
                return Err(SceneryError::Invalid("road reserved"));
            }
            let points = c.points(count, &mut actual_points, point_count)?;
            roads.push(SceneryRoad {
                source_id,
                class,
                width,
                width_inferred,
                points,
            });
        }
        for _ in 0..building_count {
            let source_id = c.i64()?;
            let height_source = match c.u8()? {
                1 => BuildingHeightSource::OsmHeight,
                2 => BuildingHeightSource::OsmLevels,
                3 => BuildingHeightSource::Default,
                _ => return Err(SceneryError::Invalid("height provenance")),
            };
            if c.u8()? != 0 || c.u16()? != 0 {
                return Err(SceneryError::Invalid("building flags"));
            }
            let height = Meters(c.f64()?);
            let count = c.count()?;
            let indices = c.count()?;
            let footprint = c.points(count, &mut actual_points, point_count)?;
            let triangles = c.triangles(indices, count, &mut actual_indices, index_count)?;
            buildings.push(SceneryBuilding {
                source_id,
                height,
                height_source,
                footprint,
                triangles,
            });
        }
        for _ in 0..landcover_count {
            let source_id = c.i64()?;
            let class = read_land_class(c.u8()?)?;
            if c.u8()? != 0 || c.u16()? != 0 {
                return Err(SceneryError::Invalid("landcover flags"));
            }
            let count = c.count()?;
            let indices = c.count()?;
            let boundary = c.points(count, &mut actual_points, point_count)?;
            let triangles = c.triangles(indices, count, &mut actual_indices, index_count)?;
            landcover.push(SceneryLandCover {
                source_id,
                class,
                boundary,
                triangles,
            });
        }
        if c.remaining() != 0 || actual_points != point_count || actual_indices != index_count {
            return Err(SceneryError::Invalid("aggregate counts"));
        }
        // Canonical order is part of v1: corruption cannot silently reorder records.
        if !strict_order(roads.iter().map(|f| f.source_id))
            || !strict_order(buildings.iter().map(|f| f.source_id))
            || !strict_order(landcover.iter().map(|f| f.source_id))
        {
            return Err(SceneryError::Invalid("source ordering"));
        }
        Self::new(source, roads, buildings, landcover)
    }
    pub fn write(&self, mut writer: impl Write) -> Result<(), SceneryError> {
        let mut metadata = Vec::new();
        metadata.push(self.source.kind as u8);
        metadata.extend(self.source.input_fingerprint.to_le_bytes());
        for text in [
            &self.source.name,
            &self.source.url,
            &source_notice(self.source.kind),
        ] {
            put_text(&mut metadata, text)?;
        }
        let points = self.roads.iter().map(|f| f.points.len()).sum::<usize>()
            + self
                .buildings
                .iter()
                .map(|f| f.footprint.len())
                .sum::<usize>()
            + self
                .landcover
                .iter()
                .map(|f| f.boundary.len())
                .sum::<usize>();
        let indices = (self
            .buildings
            .iter()
            .map(|f| f.triangles.len())
            .sum::<usize>()
            + self
                .landcover
                .iter()
                .map(|f| f.triangles.len())
                .sum::<usize>())
            * 3;
        let bytes = metadata.len()
            + self.roads.len() * ROAD_HEADER
            + self.buildings.len() * BUILDING_HEADER
            + self.landcover.len() * LANDCOVER_HEADER
            + points * 16
            + indices * 4;
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(SceneryError::Limit("encoded payload"));
        }
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(bytes)
            .map_err(|_| SceneryError::Limit("encoding allocation"))?;
        payload.extend(&metadata);
        for f in &self.roads {
            payload.extend(f.source_id.to_le_bytes());
            payload.extend([f.class as u8, u8::from(f.width_inferred), 0, 0]);
            payload.extend(f.width.get().to_le_bytes());
            put_count(&mut payload, f.points.len())?;
            payload.extend([0; 4]);
            put_points(&mut payload, &f.points);
        }
        for f in &self.buildings {
            payload.extend(f.source_id.to_le_bytes());
            payload.extend([f.height_source as u8, 0, 0, 0]);
            payload.extend(f.height.get().to_le_bytes());
            put_count(&mut payload, f.footprint.len())?;
            put_count(&mut payload, f.triangles.len() * 3)?;
            put_points(&mut payload, &f.footprint);
            put_triangles(&mut payload, &f.triangles);
        }
        for f in &self.landcover {
            payload.extend(f.source_id.to_le_bytes());
            payload.extend([f.class as u8, 0, 0, 0]);
            put_count(&mut payload, f.boundary.len())?;
            put_count(&mut payload, f.triangles.len() * 3)?;
            put_points(&mut payload, &f.boundary);
            put_triangles(&mut payload, &f.triangles);
        }
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend(MAGIC);
        header.extend(FORMAT_VERSION.to_le_bytes());
        header.extend(0u16.to_le_bytes());
        for n in [
            payload.len(),
            self.roads.len(),
            self.buildings.len(),
            self.landcover.len(),
            points,
            indices,
            metadata.len(),
            0,
        ] {
            put_count(&mut header, n)?;
        }
        header.extend(fingerprint(&payload).to_le_bytes());
        writer.write_all(&header)?;
        writer.write_all(&payload)?;
        Ok(())
    }
}

fn source_notice(kind: ScenerySourceKind) -> String {
    match kind {
        ScenerySourceKind::OpenStreetMap => format!(
            "{OSM_ATTRIBUTION}; {OSM_COPYRIGHT_URL}; derived database licensed under ODbL 1.0: {ODBL_URL}"
        ),
        ScenerySourceKind::Synthetic => "Synthetic test geometry; not real geography".to_owned(),
    }
}
fn read_source(bytes: &[u8]) -> Result<ScenerySource, SceneryError> {
    let mut c = Cursor::new(bytes);
    let kind = match c.u8()? {
        1 => ScenerySourceKind::OpenStreetMap,
        2 => ScenerySourceKind::Synthetic,
        _ => return Err(SceneryError::Invalid("source kind")),
    };
    let input_fingerprint = c.u64()?;
    let name = c.text()?;
    let url = c.text()?;
    if c.text()? != source_notice(kind) || c.remaining() != 0 {
        return Err(SceneryError::Invalid("source notice"));
    }
    let source = ScenerySource {
        kind,
        name,
        url,
        input_fingerprint,
    };
    validate_source(&source)?;
    Ok(source)
}
fn strict_order(ids: impl Iterator<Item = i64>) -> bool {
    let mut previous = 0;
    for id in ids {
        if id <= previous {
            return false;
        }
        previous = id;
    }
    true
}
fn bounded_vec<T>(len: usize) -> Result<Vec<T>, SceneryError> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| SceneryError::Limit("record allocation"))?;
    Ok(v)
}
fn put_count(bytes: &mut Vec<u8>, value: usize) -> Result<(), SceneryError> {
    bytes.extend(
        u32::try_from(value)
            .map_err(|_| SceneryError::Limit("count encoding"))?
            .to_le_bytes(),
    );
    Ok(())
}
fn put_text(bytes: &mut Vec<u8>, value: &str) -> Result<(), SceneryError> {
    bytes.extend(
        u16::try_from(value.len())
            .map_err(|_| SceneryError::Limit("text encoding"))?
            .to_le_bytes(),
    );
    bytes.extend(value.as_bytes());
    Ok(())
}
fn put_points(bytes: &mut Vec<u8>, points: &[Geodetic]) {
    for p in points {
        bytes.extend(p.latitude.get().to_le_bytes());
        bytes.extend(p.longitude.get().to_le_bytes());
    }
}
fn put_triangles(bytes: &mut Vec<u8>, triangles: &[[u32; 3]]) {
    for t in triangles {
        for i in t {
            bytes.extend(i.to_le_bytes());
        }
    }
}
fn read_road_class(v: u8) -> Result<RoadClass, SceneryError> {
    match v {
        1 => Ok(RoadClass::Motorway),
        2 => Ok(RoadClass::Primary),
        3 => Ok(RoadClass::Secondary),
        4 => Ok(RoadClass::Residential),
        5 => Ok(RoadClass::Service),
        6 => Ok(RoadClass::Track),
        7 => Ok(RoadClass::Path),
        _ => Err(SceneryError::Invalid("road class")),
    }
}
fn read_land_class(v: u8) -> Result<LandCoverClass, SceneryError> {
    match v {
        1 => Ok(LandCoverClass::Forest),
        2 => Ok(LandCoverClass::Grass),
        3 => Ok(LandCoverClass::Farmland),
        4 => Ok(LandCoverClass::Residential),
        5 => Ok(LandCoverClass::Industrial),
        6 => Ok(LandCoverClass::Water),
        7 => Ok(LandCoverClass::Bare),
        8 => Ok(LandCoverClass::Rock),
        _ => Err(SceneryError::Invalid("landcover class")),
    }
}
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], SceneryError> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(SceneryError::Invalid("truncated record"))?;
        let result = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, SceneryError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, SceneryError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| SceneryError::Invalid("u16"))?,
        ))
    }
    fn u32(&mut self) -> Result<u32, SceneryError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| SceneryError::Invalid("u32"))?,
        ))
    }
    fn count(&mut self) -> Result<usize, SceneryError> {
        usize::try_from(self.u32()?).map_err(|_| SceneryError::Limit("platform count"))
    }
    fn u64(&mut self) -> Result<u64, SceneryError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SceneryError::Invalid("u64"))?,
        ))
    }
    fn i64(&mut self) -> Result<i64, SceneryError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SceneryError::Invalid("i64"))?,
        ))
    }
    fn f64(&mut self) -> Result<f64, SceneryError> {
        Ok(f64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SceneryError::Invalid("f64"))?,
        ))
    }
    fn text(&mut self) -> Result<String, SceneryError> {
        let n = usize::from(self.u16()?);
        if n > 2048 {
            return Err(SceneryError::Limit("metadata text"));
        }
        Ok(std::str::from_utf8(self.take(n)?)
            .map_err(|_| SceneryError::Invalid("UTF-8 metadata"))?
            .to_owned())
    }
    fn points(
        &mut self,
        count: usize,
        total: &mut usize,
        expected: usize,
    ) -> Result<Vec<Geodetic>, SceneryError> {
        *total = total
            .checked_add(count)
            .filter(|&n| n <= expected)
            .ok_or(SceneryError::Limit("point count"))?;
        if count > MAX_FEATURE_POINTS || count * 16 > self.remaining() {
            return Err(SceneryError::Limit("feature point count"));
        }
        let mut points = bounded_vec(count)?;
        for _ in 0..count {
            points.push(Geodetic::new(
                Radians(self.f64()?),
                Radians(self.f64()?),
                Meters::ZERO,
            ));
        }
        Ok(points)
    }
    fn triangles(
        &mut self,
        count: usize,
        points: usize,
        total: &mut usize,
        expected: usize,
    ) -> Result<Vec<[u32; 3]>, SceneryError> {
        *total = total
            .checked_add(count)
            .filter(|&n| n <= expected)
            .ok_or(SceneryError::Limit("index count"))?;
        if points < 3 || count != (points - 2) * 3 || count * 4 > self.remaining() {
            return Err(SceneryError::Invalid("polygon index count"));
        }
        let mut triangles = bounded_vec(count / 3)?;
        for _ in 0..count / 3 {
            triangles.push([self.u32()?, self.u32()?, self.u32()?]);
        }
        Ok(triangles)
    }
}
