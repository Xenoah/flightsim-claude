use super::*;
use flightsim_core::{LocalFrame, Radians};

const DIRECTORY_ENTRY_LEN: usize = 32;
const DIRECTORY_ENTRY_LEN_FIELD: u32 = 32;
const MAX_SECTION_COUNT: u32 = 16;
pub const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 96 * 1024 * 1024;
const OPTIONAL_SECTION: u32 = 1;
const NONE_STRING: u32 = u32::MAX;

const CORE: u16 = 1;
const APRON_TRIANGLES: u16 = 2;
const HOLDING_POSITIONS: u16 = 3;
const GROUND_LIGHTS: u16 = 4;
const TAXIWAY_ATTRIBUTES: u16 = 5;
const STRING_INDEX: u16 = 6;
const STRING_BYTES: u16 = 7;

const APRON_RECORD_LEN: usize = 64;
const APRON_RECORD_LEN_FIELD: u32 = 64;
const HOLDING_RECORD_LEN: usize = 64;
const HOLDING_RECORD_LEN_FIELD: u32 = 64;
const LIGHT_RECORD_LEN: usize = 40;
const LIGHT_RECORD_LEN_FIELD: u32 = 40;
const ATTRIBUTE_RECORD_LEN: usize = 24;
const ATTRIBUTE_RECORD_LEN_FIELD: u32 = 24;
const STRING_INDEX_RECORD_LEN: usize = 8;
const STRING_INDEX_RECORD_LEN_FIELD: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AirportSourceKind {
    Node = 0,
    Way = 1,
    Relation = 2,
}

impl AirportSourceKind {
    fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Node),
            1 => Some(Self::Way),
            2 => Some(Self::Relation),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AirportSurface {
    #[default]
    Unknown = 0,
    Asphalt = 1,
    Concrete = 2,
    Paved = 3,
    Grass = 4,
    Gravel = 5,
    Dirt = 6,
    Sand = 7,
}

impl AirportSurface {
    fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Asphalt),
            2 => Some(Self::Concrete),
            3 => Some(Self::Paved),
            4 => Some(Self::Grass),
            5 => Some(Self::Gravel),
            6 => Some(Self::Dirt),
            7 => Some(Self::Sand),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum TaxiwayLighting {
    #[default]
    None = 0,
    Edge = 1,
    Centerline = 2,
    EdgeAndCenterline = 3,
}

impl TaxiwayLighting {
    fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Edge),
            2 => Some(Self::Centerline),
            3 => Some(Self::EdgeAndCenterline),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TaxiwayMetadata {
    reference: Option<String>,
    surface: AirportSurface,
    lighting: TaxiwayLighting,
}

impl TaxiwayMetadata {
    #[must_use]
    pub fn new(
        reference: Option<String>,
        surface: AirportSurface,
        lighting: TaxiwayLighting,
    ) -> Self {
        Self {
            reference,
            surface,
            lighting,
        }
    }

    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    #[must_use]
    pub const fn surface(&self) -> AirportSurface {
        self.surface
    }

    #[must_use]
    pub const fn lighting(&self) -> TaxiwayLighting {
        self.lighting
    }

    pub(super) fn is_default(&self) -> bool {
        self.reference.is_none()
            && self.surface == AirportSurface::Unknown
            && self.lighting == TaxiwayLighting::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundFeatureGeometryError {
    EmptyApron,
    InvalidCoordinate { point_index: usize },
    CollapsedTriangle { triangle_index: usize },
    InvalidHeading,
    InvalidWidth,
    EmptyReference,
    AllocationFailed { requested: usize },
}

impl core::fmt::Display for GroundFeatureGeometryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyApron => write!(formatter, "apron needs at least one triangle"),
            Self::InvalidCoordinate { point_index } => {
                write!(formatter, "point {point_index} has an invalid coordinate")
            }
            Self::CollapsedTriangle { triangle_index } => {
                write!(formatter, "apron triangle {triangle_index} is collapsed")
            }
            Self::InvalidHeading => write!(formatter, "heading must be finite"),
            Self::InvalidWidth => write!(formatter, "width must be positive and finite"),
            Self::EmptyReference => write!(formatter, "reference must not be empty"),
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate {requested} ground-feature items"
            ),
        }
    }
}

impl std::error::Error for GroundFeatureGeometryError {}

#[derive(Debug, Clone, PartialEq)]
pub struct AirportApron {
    source_kind: AirportSourceKind,
    source_id: i64,
    surface: AirportSurface,
    triangles: Vec<[Geodetic; 3]>,
    triangle_degrees: Vec<[(f64, f64); 3]>,
}

impl AirportApron {
    pub fn new(
        source_kind: AirportSourceKind,
        source_id: i64,
        surface: AirportSurface,
        triangles: Vec<[Geodetic; 3]>,
    ) -> Result<Self, GroundFeatureGeometryError> {
        let mut degrees = Vec::new();
        degrees.try_reserve_exact(triangles.len()).map_err(|_| {
            GroundFeatureGeometryError::AllocationFailed {
                requested: triangles.len(),
            }
        })?;
        degrees.extend(triangles.into_iter().map(|triangle| {
            triangle.map(|point| (point.latitude_degrees(), point.longitude_degrees()))
        }));
        Self::from_degree_triangles(source_kind, source_id, surface, degrees)
    }

    fn from_degree_triangles(
        source_kind: AirportSourceKind,
        source_id: i64,
        surface: AirportSurface,
        triangle_degrees: Vec<[(f64, f64); 3]>,
    ) -> Result<Self, GroundFeatureGeometryError> {
        validate_triangles(&triangle_degrees)?;
        let mut triangles = Vec::new();
        triangles
            .try_reserve_exact(triangle_degrees.len())
            .map_err(|_| GroundFeatureGeometryError::AllocationFailed {
                requested: triangle_degrees.len(),
            })?;
        triangles.extend(
            triangle_degrees
                .iter()
                .map(|triangle| triangle.map(|(lat, lon)| Geodetic::from_degrees(lat, lon, 0.0))),
        );
        Ok(Self {
            source_kind,
            source_id,
            surface,
            triangles,
            triangle_degrees,
        })
    }

    #[must_use]
    pub const fn source_kind(&self) -> AirportSourceKind {
        self.source_kind
    }
    #[must_use]
    pub const fn source_id(&self) -> i64 {
        self.source_id
    }
    #[must_use]
    pub const fn surface(&self) -> AirportSurface {
        self.surface
    }
    #[must_use]
    pub fn triangles(&self) -> &[[Geodetic; 3]] {
        &self.triangles
    }
}

fn validate_triangles(triangles: &[[(f64, f64); 3]]) -> Result<(), GroundFeatureGeometryError> {
    if triangles.is_empty() {
        return Err(GroundFeatureGeometryError::EmptyApron);
    }
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        for (vertex, &(lat, lon)) in triangle.iter().enumerate() {
            let point = Geodetic::from_degrees(lat, lon, 0.0);
            if validate_horizontal_coordinate(point, "apron_point").is_err() {
                return Err(GroundFeatureGeometryError::InvalidCoordinate {
                    point_index: triangle_index * 3 + vertex,
                });
            }
        }
        let points = triangle.map(|(lat, lon)| Geodetic::from_degrees(lat, lon, 0.0));
        let frame = LocalFrame::new(points[0]);
        let first = frame.ecef_to_ned_position(points[1].to_ecef()).0;
        let second = frame.ecef_to_ned_position(points[2].to_ecef()).0;
        let twice_area = (first.x * second.y - first.y * second.x).abs();
        let scale = first
            .x
            .abs()
            .max(first.y.abs())
            .max(second.x.abs())
            .max(second.y.abs());
        let tolerance = f64::EPSILON * scale * scale * 16.0;
        if twice_area.partial_cmp(&tolerance) != Some(Ordering::Greater) {
            return Err(GroundFeatureGeometryError::CollapsedTriangle { triangle_index });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum HoldingPositionType {
    Runway = 0,
    Intermediate = 1,
    Ils = 2,
}
impl HoldingPositionType {
    fn decode(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Runway),
            1 => Some(Self::Intermediate),
            2 => Some(Self::Ils),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RunwaySide {
    #[default]
    Unknown = 0,
    Forward = 1,
    Backward = 2,
}
impl RunwaySide {
    fn decode(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Forward),
            2 => Some(Self::Backward),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirportHoldingPosition {
    source_kind: AirportSourceKind,
    source_id: i64,
    position: Geodetic,
    position_degrees: (f64, f64),
    holding_type: HoldingPositionType,
    heading: Radians,
    width: Meters,
    reference: Option<String>,
    related_taxiway: Option<i64>,
    runway_side: RunwaySide,
}

impl AirportHoldingPosition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_kind: AirportSourceKind,
        source_id: i64,
        position: Geodetic,
        holding_type: HoldingPositionType,
        heading: Radians,
        width: Meters,
        reference: Option<String>,
        related_taxiway: Option<i64>,
        runway_side: RunwaySide,
    ) -> Result<Self, GroundFeatureGeometryError> {
        Self::from_degrees(
            source_kind,
            source_id,
            (position.latitude_degrees(), position.longitude_degrees()),
            holding_type,
            heading,
            width,
            reference,
            related_taxiway,
            runway_side,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_degrees(
        source_kind: AirportSourceKind,
        source_id: i64,
        position_degrees: (f64, f64),
        holding_type: HoldingPositionType,
        heading: Radians,
        width: Meters,
        reference: Option<String>,
        related_taxiway: Option<i64>,
        runway_side: RunwaySide,
    ) -> Result<Self, GroundFeatureGeometryError> {
        let position = Geodetic::from_degrees(position_degrees.0, position_degrees.1, 0.0);
        if validate_horizontal_coordinate(position, "holding_position").is_err() {
            return Err(GroundFeatureGeometryError::InvalidCoordinate { point_index: 0 });
        }
        if !heading.is_finite() {
            return Err(GroundFeatureGeometryError::InvalidHeading);
        }
        if !width.is_finite() || width.get() <= 0.0 {
            return Err(GroundFeatureGeometryError::InvalidWidth);
        }
        if reference.as_ref().is_some_and(String::is_empty) {
            return Err(GroundFeatureGeometryError::EmptyReference);
        }
        Ok(Self {
            source_kind,
            source_id,
            position,
            position_degrees,
            holding_type,
            heading: heading.wrap_positive(),
            width,
            reference,
            related_taxiway,
            runway_side,
        })
    }

    #[must_use]
    pub const fn source_kind(&self) -> AirportSourceKind {
        self.source_kind
    }
    #[must_use]
    pub const fn source_id(&self) -> i64 {
        self.source_id
    }
    #[must_use]
    pub const fn position(&self) -> Geodetic {
        self.position
    }
    #[must_use]
    pub const fn holding_type(&self) -> HoldingPositionType {
        self.holding_type
    }
    #[must_use]
    pub const fn heading(&self) -> Radians {
        self.heading
    }
    #[must_use]
    pub const fn width(&self) -> Meters {
        self.width
    }
    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }
    #[must_use]
    pub const fn related_taxiway(&self) -> Option<i64> {
        self.related_taxiway
    }
    #[must_use]
    pub const fn runway_side(&self) -> RunwaySide {
        self.runway_side
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum GroundLightKind {
    TaxiwayEdge = 0,
    TaxiwayCenterline = 1,
    RunwayGuard = 2,
}
impl GroundLightKind {
    fn decode(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::TaxiwayEdge),
            1 => Some(Self::TaxiwayCenterline),
            2 => Some(Self::RunwayGuard),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirportGroundLight {
    source_kind: AirportSourceKind,
    source_id: i64,
    position: Geodetic,
    position_degrees: (f64, f64),
    kind: GroundLightKind,
}

impl AirportGroundLight {
    pub fn new(
        source_kind: AirportSourceKind,
        source_id: i64,
        position: Geodetic,
        kind: GroundLightKind,
    ) -> Result<Self, GroundFeatureGeometryError> {
        let position_degrees = (position.latitude_degrees(), position.longitude_degrees());
        Self::from_degrees(source_kind, source_id, position_degrees, kind)
    }

    fn from_degrees(
        source_kind: AirportSourceKind,
        source_id: i64,
        position_degrees: (f64, f64),
        kind: GroundLightKind,
    ) -> Result<Self, GroundFeatureGeometryError> {
        let position = Geodetic::from_degrees(position_degrees.0, position_degrees.1, 0.0);
        if validate_horizontal_coordinate(position, "ground_light").is_err() {
            return Err(GroundFeatureGeometryError::InvalidCoordinate { point_index: 0 });
        }
        Ok(Self {
            source_kind,
            source_id,
            position,
            position_degrees,
            kind,
        })
    }
    #[must_use]
    pub const fn source_kind(&self) -> AirportSourceKind {
        self.source_kind
    }
    #[must_use]
    pub const fn source_id(&self) -> i64 {
        self.source_id
    }
    #[must_use]
    pub const fn position(&self) -> Geodetic {
        self.position
    }
    #[must_use]
    pub const fn kind(&self) -> GroundLightKind {
        self.kind
    }
}

#[derive(Clone, Copy)]
struct Directory {
    kind: u16,
    offset: usize,
    byte_len: usize,
}

impl AirportDatabase {
    /// Builds a canonical FSAP v3 database containing surface ground features.
    pub fn with_ground_features(
        mut runways: Vec<AirportRunway>,
        mut taxiways: Vec<AirportTaxiway>,
        mut aprons: Vec<AirportApron>,
        mut holding_positions: Vec<AirportHoldingPosition>,
        mut ground_lights: Vec<AirportGroundLight>,
    ) -> Result<Self, AirportDatabaseError> {
        let segments = taxiways
            .iter()
            .try_fold(0usize, |n, t| {
                n.checked_add(t.points.len().saturating_sub(1))
            })
            .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
        let triangles = aprons
            .iter()
            .try_fold(0usize, |n, a| n.checked_add(a.triangles.len()))
            .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
        let attrs = taxiways.iter().filter(|t| !t.metadata.is_default()).count();
        let mut fixed = runways
            .len()
            .checked_add(segments)
            .and_then(|v| v.checked_add(triangles))
            .and_then(|v| v.checked_add(holding_positions.len()))
            .and_then(|v| v.checked_add(ground_lights.len()))
            .and_then(|v| v.checked_add(attrs))
            .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
        let string_refs = canonical_string_slices(&taxiways, &holding_positions)?.len();
        fixed = fixed
            .checked_add(string_refs)
            .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
        if fixed > MAX_RECORD_COUNT as usize {
            return Err(AirportDatabaseError::TooManyRecords { count: fixed });
        }
        for (i, runway) in runways.iter().enumerate() {
            runway.validate_for_storage().map_err(|source| {
                AirportDatabaseError::InvalidRunway {
                    record_index: i,
                    source_way_id: runway.source_way_id,
                    source,
                }
            })?;
        }
        for (i, taxiway) in taxiways.iter().enumerate() {
            taxiway.validate_for_storage().map_err(|source| {
                AirportDatabaseError::InvalidTaxiway {
                    record_index: i,
                    source_way_id: taxiway.source_way_id,
                    source,
                }
            })?;
        }
        for apron in &aprons {
            validate_triangles(&apron.triangle_degrees).map_err(|source| {
                AirportDatabaseError::InvalidGroundFeature {
                    source_id: apron.source_id,
                    source,
                }
            })?;
        }
        for holding in &holding_positions {
            if validate_horizontal_coordinate(holding.position, "holding_position").is_err() {
                return Err(AirportDatabaseError::InvalidGroundFeature {
                    source_id: holding.source_id,
                    source: GroundFeatureGeometryError::InvalidCoordinate { point_index: 0 },
                });
            }
            if !holding.heading.is_finite() {
                return Err(AirportDatabaseError::InvalidGroundFeature {
                    source_id: holding.source_id,
                    source: GroundFeatureGeometryError::InvalidHeading,
                });
            }
            if !holding.width.is_finite() || holding.width.get() <= 0.0 {
                return Err(AirportDatabaseError::InvalidGroundFeature {
                    source_id: holding.source_id,
                    source: GroundFeatureGeometryError::InvalidWidth,
                });
            }
        }
        runways.sort_unstable_by(compare_runways);
        if let Some(pair) = runways
            .windows(2)
            .find(|pair| pair[0].source_way_id == pair[1].source_way_id)
        {
            return Err(AirportDatabaseError::DuplicateRunwayWayId {
                source_way_id: pair[0].source_way_id,
            });
        }
        taxiways.sort_unstable_by_key(|v| v.source_way_id);
        reject_duplicate_taxiways(&taxiways)?;
        aprons.sort_unstable_by_key(|v| (v.source_kind, v.source_id));
        holding_positions.sort_unstable_by_key(|v| (v.source_kind, v.source_id));
        ground_lights.sort_unstable_by_key(|v| (v.source_kind, v.source_id));
        reject_duplicate_features(&aprons, APRON_TRIANGLES, |v| (v.source_kind, v.source_id))?;
        reject_duplicate_features(&holding_positions, HOLDING_POSITIONS, |v| {
            (v.source_kind, v.source_id)
        })?;
        reject_duplicate_features(&ground_lights, GROUND_LIGHTS, |v| {
            (v.source_kind, v.source_id)
        })?;
        for holding in &holding_positions {
            if let Some(id) = holding.related_taxiway {
                if taxiways
                    .binary_search_by_key(&id, |v| v.source_way_id)
                    .is_err()
                {
                    return Err(AirportDatabaseError::OrphanTaxiwayReference { source_way_id: id });
                }
            }
        }
        Ok(Self {
            runways,
            taxiways,
            aprons,
            holding_positions,
            ground_lights,
            format_version: FORMAT_VERSION_V3,
        })
    }
}

fn reject_duplicate_taxiways(values: &[AirportTaxiway]) -> Result<(), AirportDatabaseError> {
    if let Some(pair) = values
        .windows(2)
        .find(|p| p[0].source_way_id == p[1].source_way_id)
    {
        return Err(AirportDatabaseError::DuplicateTaxiwayWayId {
            source_way_id: pair[0].source_way_id,
        });
    }
    Ok(())
}

fn reject_duplicate_features<T, F>(
    values: &[T],
    section: u16,
    key: F,
) -> Result<(), AirportDatabaseError>
where
    F: Fn(&T) -> (AirportSourceKind, i64),
{
    if let Some(pair) = values.windows(2).find(|p| key(&p[0]) == key(&p[1])) {
        let (source_kind, source_id) = key(&pair[0]);
        return Err(AirportDatabaseError::DuplicateGroundFeature {
            section_kind: section,
            source_kind,
            source_id,
        });
    }
    Ok(())
}

pub(super) fn to_bytes(database: &AirportDatabase) -> Result<Vec<u8>, AirportDatabaseError> {
    let strings = canonical_strings(database)?;
    let mut sections = Vec::<(u16, u32, Vec<u8>)>::new();
    sections
        .try_reserve_exact(7)
        .map_err(|_| allocation(7 * core::mem::size_of::<(u16, u32, Vec<u8>)>()))?;
    sections.push((CORE, V2_RECORD_LEN_FIELD, encode_core(database)?));
    sections.push((
        APRON_TRIANGLES,
        APRON_RECORD_LEN_FIELD,
        encode_aprons(database)?,
    ));
    sections.push((
        HOLDING_POSITIONS,
        HOLDING_RECORD_LEN_FIELD,
        encode_holdings(database, &strings)?,
    ));
    sections.push((
        GROUND_LIGHTS,
        LIGHT_RECORD_LEN_FIELD,
        encode_lights(database)?,
    ));
    sections.push((
        TAXIWAY_ATTRIBUTES,
        ATTRIBUTE_RECORD_LEN_FIELD,
        encode_attributes(database, &strings)?,
    ));
    sections.push((
        STRING_INDEX,
        STRING_INDEX_RECORD_LEN_FIELD,
        encode_string_index(&strings)?,
    ));
    sections.push((STRING_BYTES, 1, encode_string_bytes(&strings)?));
    let directory_len = checked_bytes(sections.len(), DIRECTORY_ENTRY_LEN)?;
    let data_len = sections
        .iter()
        .try_fold(0usize, |n, (_, _, bytes)| n.checked_add(bytes.len()))
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    let payload_len = directory_len
        .checked_add(data_len)
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    if payload_len > MAX_PAYLOAD_BYTES {
        return Err(AirportDatabaseError::AllocationFailed {
            requested: payload_len,
        });
    }
    let mut payload = reserve_vec(payload_len)?;
    let mut offset = directory_len;
    for (index, (kind, size, bytes)) in sections.iter().enumerate() {
        let count = if *kind == STRING_BYTES {
            bytes.len()
        } else {
            bytes.len() / *size as usize
        };
        payload.extend_from_slice(&kind.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&(if index == 0 { 0 } else { OPTIONAL_SECTION }).to_le_bytes());
        payload.extend_from_slice(&size.to_le_bytes());
        payload.extend_from_slice(
            &u32::try_from(count)
                .map_err(|_| AirportDatabaseError::TooManyRecords { count })?
                .to_le_bytes(),
        );
        payload.extend_from_slice(&(offset as u64).to_le_bytes());
        payload.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        offset += bytes.len();
    }
    for (_, _, bytes) in &sections {
        payload.extend_from_slice(bytes);
    }
    let output_len = HEADER_LEN
        .checked_add(payload.len())
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    let mut output = reserve_vec(output_len)?;
    output.extend_from_slice(&MAGIC);
    output.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(
        &u32::try_from(sections.len())
            .expect("section count is a small format constant")
            .to_le_bytes(),
    );
    output.extend_from_slice(&DIRECTORY_ENTRY_LEN_FIELD.to_le_bytes());
    output.extend_from_slice(&fnv1a(&payload).to_le_bytes());
    output.extend_from_slice(&payload);
    Ok(output)
}

fn canonical_strings(database: &AirportDatabase) -> Result<Vec<&str>, AirportDatabaseError> {
    canonical_string_slices(&database.taxiways, &database.holding_positions)
}

fn canonical_string_slices<'a>(
    taxiways: &'a [AirportTaxiway],
    holding_positions: &'a [AirportHoldingPosition],
) -> Result<Vec<&'a str>, AirportDatabaseError> {
    validate_reference_expansion(taxiways, holding_positions)?;
    let count = taxiways.iter().filter(|v| v.reference().is_some()).count()
        + holding_positions
            .iter()
            .filter(|v| v.reference().is_some())
            .count();
    let mut strings = Vec::new();
    strings
        .try_reserve_exact(count)
        .map_err(|_| allocation(allocation_bytes::<&str>(count)))?;
    strings.extend(taxiways.iter().filter_map(AirportTaxiway::reference));
    strings.extend(
        holding_positions
            .iter()
            .filter_map(AirportHoldingPosition::reference),
    );
    if strings.iter().any(|v| v.is_empty()) {
        return Err(AirportDatabaseError::InvalidV3 {
            section_kind: STRING_BYTES,
            record_index: 0,
            message: "empty strings are not canonical",
        });
    }
    strings.sort_unstable();
    strings.dedup();
    let bytes = strings
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .ok_or(AirportDatabaseError::StringBytesExceedLimit {
            found: usize::MAX,
            maximum: MAX_STRING_BYTES,
        })?;
    if bytes > MAX_STRING_BYTES {
        return Err(AirportDatabaseError::StringBytesExceedLimit {
            found: bytes,
            maximum: MAX_STRING_BYTES,
        });
    }
    Ok(strings)
}

fn validate_reference_expansion(
    taxiways: &[AirportTaxiway],
    holding_positions: &[AirportHoldingPosition],
) -> Result<(), AirportDatabaseError> {
    let mut expanded = 0usize;
    for value in taxiways.iter().filter_map(AirportTaxiway::reference).chain(
        holding_positions
            .iter()
            .filter_map(AirportHoldingPosition::reference),
    ) {
        expanded = expanded.checked_add(value.len()).ok_or(
            AirportDatabaseError::StringBytesExceedLimit {
                found: usize::MAX,
                maximum: MAX_STRING_BYTES,
            },
        )?;
        if expanded > MAX_STRING_BYTES {
            return Err(AirportDatabaseError::StringBytesExceedLimit {
                found: expanded,
                maximum: MAX_STRING_BYTES,
            });
        }
    }
    Ok(())
}

fn string_id(strings: &[&str], value: Option<&str>) -> u32 {
    value.map_or(NONE_STRING, |v| {
        u32::try_from(strings.binary_search(&v).expect("canonical string exists"))
            .expect("record cap bounds strings")
    })
}

fn encode_core(db: &AirportDatabase) -> Result<Vec<u8>, AirportDatabaseError> {
    let segments = db
        .taxiways
        .iter()
        .try_fold(0usize, |count, taxiway| {
            count.checked_add(taxiway.points.len().saturating_sub(1))
        })
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    let records = db
        .runways
        .len()
        .checked_add(segments)
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    let mut out = reserve_vec(checked_bytes(records, V2_RECORD_LEN)?)?;
    for (i, v) in db.runways.iter().enumerate() {
        v.validate_for_storage()
            .map_err(|source| AirportDatabaseError::InvalidRunway {
                record_index: i,
                source_way_id: v.source_way_id,
                source,
            })?;
        write_v2_record(
            &mut out,
            RECORD_KIND_RUNWAY,
            v.source_way_id,
            0,
            (v.threshold_latitude_degrees, v.threshold_longitude_degrees),
            (v.opposite_latitude_degrees, v.opposite_longitude_degrees),
            v.runway.width,
        );
    }
    for (i, v) in db.taxiways.iter().enumerate() {
        v.validate_for_storage()
            .map_err(|source| AirportDatabaseError::InvalidTaxiway {
                record_index: i,
                source_way_id: v.source_way_id,
                source,
            })?;
        for (segment, pair) in v.point_degrees.windows(2).enumerate() {
            write_v2_record(
                &mut out,
                RECORD_KIND_TAXIWAY,
                v.source_way_id,
                u32::try_from(segment)
                    .map_err(|_| AirportDatabaseError::TooManyRecords { count: segment })?,
                pair[0],
                pair[1],
                v.width,
            );
        }
    }
    Ok(out)
}

fn encode_aprons(db: &AirportDatabase) -> Result<Vec<u8>, AirportDatabaseError> {
    let count = db
        .aprons
        .iter()
        .try_fold(0usize, |count, apron| {
            count.checked_add(apron.triangle_degrees.len())
        })
        .ok_or(AirportDatabaseError::TooManyRecords { count: usize::MAX })?;
    let mut out = reserve_vec(checked_bytes(count, APRON_RECORD_LEN)?)?;
    for apron in &db.aprons {
        for (index, triangle) in apron.triangle_degrees.iter().enumerate() {
            out.extend_from_slice(&apron.source_id.to_le_bytes());
            out.push(apron.source_kind as u8);
            out.push(apron.surface as u8);
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(
                &u32::try_from(index)
                    .map_err(|_| AirportDatabaseError::TooManyRecords { count: index })?
                    .to_le_bytes(),
            );
            for &(lat, lon) in triangle {
                out.extend_from_slice(&lat.to_le_bytes());
                out.extend_from_slice(&lon.to_le_bytes());
            }
        }
    }
    Ok(out)
}

fn encode_holdings(
    db: &AirportDatabase,
    strings: &[&str],
) -> Result<Vec<u8>, AirportDatabaseError> {
    let mut out = reserve_vec(checked_bytes(
        db.holding_positions.len(),
        HOLDING_RECORD_LEN,
    )?)?;
    for h in &db.holding_positions {
        out.extend_from_slice(&h.source_id.to_le_bytes());
        out.push(h.source_kind as u8);
        out.push(h.holding_type as u8);
        out.push(h.runway_side as u8);
        out.push(u8::from(h.related_taxiway.is_some()));
        out.extend_from_slice(&string_id(strings, h.reference()).to_le_bytes());
        out.extend_from_slice(&h.position_degrees.0.to_le_bytes());
        out.extend_from_slice(&h.position_degrees.1.to_le_bytes());
        out.extend_from_slice(&h.heading.to_degrees().get().to_le_bytes());
        out.extend_from_slice(&h.width.get().to_le_bytes());
        out.extend_from_slice(&h.related_taxiway.unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    Ok(out)
}

fn encode_lights(db: &AirportDatabase) -> Result<Vec<u8>, AirportDatabaseError> {
    let mut out = reserve_vec(checked_bytes(db.ground_lights.len(), LIGHT_RECORD_LEN)?)?;
    for v in &db.ground_lights {
        out.extend_from_slice(&v.source_id.to_le_bytes());
        out.push(v.source_kind as u8);
        out.push(v.kind as u8);
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&v.position_degrees.0.to_le_bytes());
        out.extend_from_slice(&v.position_degrees.1.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    Ok(out)
}

fn encode_attributes(
    db: &AirportDatabase,
    strings: &[&str],
) -> Result<Vec<u8>, AirportDatabaseError> {
    let count = db
        .taxiways
        .iter()
        .filter(|v| !v.metadata.is_default())
        .count();
    let mut out = reserve_vec(checked_bytes(count, ATTRIBUTE_RECORD_LEN)?)?;
    for v in db.taxiways.iter().filter(|v| !v.metadata.is_default()) {
        out.extend_from_slice(&v.source_way_id.to_le_bytes());
        out.extend_from_slice(&string_id(strings, v.reference()).to_le_bytes());
        out.push(v.surface() as u8);
        out.push(v.lighting() as u8);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    Ok(out)
}

fn encode_string_index(strings: &[&str]) -> Result<Vec<u8>, AirportDatabaseError> {
    let mut out = reserve_vec(checked_bytes(strings.len(), STRING_INDEX_RECORD_LEN)?)?;
    let mut offset = 0u32;
    for value in strings {
        out.extend_from_slice(&offset.to_le_bytes());
        let length = u32::try_from(value.len()).map_err(|_| {
            AirportDatabaseError::StringBytesExceedLimit {
                found: value.len(),
                maximum: MAX_STRING_BYTES,
            }
        })?;
        out.extend_from_slice(&length.to_le_bytes());
        offset = offset
            .checked_add(length)
            .expect("string byte cap fits u32");
    }
    Ok(out)
}
fn encode_string_bytes(strings: &[&str]) -> Result<Vec<u8>, AirportDatabaseError> {
    let size = strings
        .iter()
        .try_fold(0usize, |size, value| size.checked_add(value.len()))
        .ok_or(AirportDatabaseError::StringBytesExceedLimit {
            found: usize::MAX,
            maximum: MAX_STRING_BYTES,
        })?;
    let mut out = reserve_vec(size)?;
    for value in strings {
        out.extend_from_slice(value.as_bytes());
    }
    Ok(out)
}

pub(super) fn from_bytes(bytes: &[u8]) -> Result<AirportDatabase, AirportDatabaseError> {
    let (directories, payload_len, checksum) = parse_header_and_directory(bytes)?;
    let expected = HEADER_LEN + payload_len;
    match bytes.len().cmp(&expected) {
        Ordering::Less => {
            return Err(AirportDatabaseError::Truncated {
                expected,
                actual: bytes.len(),
            });
        }
        Ordering::Greater => {
            return Err(AirportDatabaseError::TrailingData {
                expected,
                actual: bytes.len(),
            });
        }
        Ordering::Equal => {}
    }
    let payload = &bytes[HEADER_LEN..];
    let actual = fnv1a(payload);
    if actual != checksum {
        return Err(AirportDatabaseError::ChecksumMismatch {
            expected: checksum,
            actual,
        });
    }
    decode(payload, &directories)
}

pub(super) fn read_from<R: Read>(
    reader: &mut R,
    header: [u8; HEADER_LEN],
) -> Result<AirportDatabase, AirportDatabaseError> {
    validate_v3_header(&header)?;
    let count = read_u32(&header, 8) as usize;
    let directory_len = count * DIRECTORY_ENTRY_LEN;
    let mut directory_bytes = reserve_vec(directory_len)?;
    directory_bytes.resize(directory_len, 0);
    read_exact_or_truncated(
        reader,
        &mut directory_bytes,
        HEADER_LEN + directory_len,
        HEADER_LEN,
    )?;
    let directories = parse_directories(&directory_bytes)?;
    let payload_len = directories
        .last()
        .map_or(directory_len, |v| v.offset + v.byte_len);
    let remaining = payload_len - directory_len;
    let mut data = reserve_vec(remaining)?;
    data.resize(remaining, 0);
    read_exact_or_truncated(
        reader,
        &mut data,
        HEADER_LEN + payload_len,
        HEADER_LEN + directory_len,
    )?;
    let mut trailing = [0u8; 1];
    loop {
        match reader.read(&mut trailing) {
            Ok(0) => break,
            Ok(_) => {
                return Err(AirportDatabaseError::TrailingData {
                    expected: HEADER_LEN + payload_len,
                    actual: HEADER_LEN + payload_len + 1,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    directory_bytes.extend_from_slice(&data);
    let actual = fnv1a(&directory_bytes);
    let checksum = read_u64(&header, 16);
    if actual != checksum {
        return Err(AirportDatabaseError::ChecksumMismatch {
            expected: checksum,
            actual,
        });
    }
    decode(&directory_bytes, &directories)
}

fn parse_header_and_directory(
    bytes: &[u8],
) -> Result<(Vec<Directory>, usize, u64), AirportDatabaseError> {
    if bytes.len() < HEADER_LEN {
        return Err(AirportDatabaseError::Truncated {
            expected: HEADER_LEN,
            actual: bytes.len(),
        });
    }
    validate_v3_header(&bytes[..HEADER_LEN])?;
    let directory_len = read_u32(bytes, 8) as usize * DIRECTORY_ENTRY_LEN;
    let minimum = HEADER_LEN + directory_len;
    if bytes.len() < minimum {
        return Err(AirportDatabaseError::Truncated {
            expected: minimum,
            actual: bytes.len(),
        });
    }
    let directories = parse_directories(&bytes[HEADER_LEN..minimum])?;
    let payload_len = directories
        .last()
        .map_or(directory_len, |v| v.offset + v.byte_len);
    Ok((directories, payload_len, read_u64(bytes, 16)))
}

fn validate_v3_header(bytes: &[u8]) -> Result<(), AirportDatabaseError> {
    let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if magic != MAGIC {
        return Err(AirportDatabaseError::NotAnAirportDatabase { found: magic });
    }
    if read_u16(bytes, 4) != FORMAT_VERSION_V3 {
        return Err(AirportDatabaseError::UnsupportedVersion {
            found: read_u16(bytes, 4),
            supported: FORMAT_VERSION_V3,
        });
    }
    if read_u16(bytes, 6) != 0 {
        return Err(AirportDatabaseError::UnsupportedFlags(read_u16(bytes, 6)));
    }
    let count = read_u32(bytes, 8);
    if count == 0 || count > MAX_SECTION_COUNT {
        return Err(AirportDatabaseError::RecordCountExceedsLimit {
            found: count,
            maximum: MAX_SECTION_COUNT,
        });
    }
    if read_u32(bytes, 12) != DIRECTORY_ENTRY_LEN_FIELD {
        return Err(AirportDatabaseError::UnsupportedRecordSize {
            found: read_u32(bytes, 12),
            supported: DIRECTORY_ENTRY_LEN_FIELD,
        });
    }
    Ok(())
}

fn parse_directories(bytes: &[u8]) -> Result<Vec<Directory>, AirportDatabaseError> {
    let count = bytes.len() / DIRECTORY_ENTRY_LEN;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| allocation(allocation_bytes::<Directory>(count)))?;
    let directory_len = bytes.len();
    let mut expected_offset = directory_len;
    let mut prior = 0u16;
    let mut records = 0usize;
    for (index, raw) in bytes.chunks_exact(DIRECTORY_ENTRY_LEN).enumerate() {
        let kind = read_u16(raw, 0);
        let schema = read_u16(raw, 2);
        let flags = read_u32(raw, 4);
        let size = read_u32(raw, 8);
        let count = read_u32(raw, 12);
        let offset = usize::try_from(read_u64(raw, 16))
            .map_err(|_| invalid(kind, index, "section offset cannot be represented"))?;
        let byte_len = usize::try_from(read_u64(raw, 24))
            .map_err(|_| invalid(kind, index, "section length cannot be represented"))?;
        if index > 0 && kind <= prior {
            return Err(invalid(
                kind,
                index,
                "section kinds are not strictly increasing",
            ));
        }
        prior = kind;
        if flags & !OPTIONAL_SECTION != 0 {
            return Err(invalid(kind, index, "reserved section flags are non-zero"));
        }
        let known = matches!(
            kind,
            CORE | APRON_TRIANGLES
                | HOLDING_POSITIONS
                | GROUND_LIGHTS
                | TAXIWAY_ATTRIBUTES
                | STRING_INDEX
                | STRING_BYTES
        );
        if !known && flags & OPTIONAL_SECTION == 0 {
            return Err(invalid(kind, index, "unknown required section"));
        }
        if known && ((kind == CORE && flags != 0) || (kind != CORE && flags != OPTIONAL_SECTION)) {
            return Err(invalid(
                kind,
                index,
                "known section has non-canonical flags",
            ));
        }
        if known && schema != 1 {
            return Err(invalid(kind, index, "unsupported section schema"));
        }
        let expected_size = match kind {
            CORE => Some(V2_RECORD_LEN_FIELD),
            APRON_TRIANGLES => Some(APRON_RECORD_LEN_FIELD),
            HOLDING_POSITIONS => Some(HOLDING_RECORD_LEN_FIELD),
            GROUND_LIGHTS => Some(LIGHT_RECORD_LEN_FIELD),
            TAXIWAY_ATTRIBUTES => Some(ATTRIBUTE_RECORD_LEN_FIELD),
            STRING_INDEX => Some(STRING_INDEX_RECORD_LEN_FIELD),
            STRING_BYTES => Some(1),
            _ => None,
        };
        if expected_size.is_some_and(|v| v != size) || size == 0 {
            return Err(invalid(kind, index, "invalid section record size"));
        }
        let calculated = usize::try_from(count)
            .ok()
            .and_then(|v| v.checked_mul(size as usize))
            .ok_or_else(|| invalid(kind, index, "section byte length overflows"))?;
        if calculated != byte_len {
            return Err(invalid(
                kind,
                index,
                "section byte length does not match count",
            ));
        }
        if offset != expected_offset {
            return Err(invalid(
                kind,
                index,
                "sections are not contiguous and non-overlapping",
            ));
        }
        expected_offset = offset
            .checked_add(byte_len)
            .ok_or_else(|| invalid(kind, index, "section end overflows"))?;
        if expected_offset > MAX_PAYLOAD_BYTES {
            return Err(invalid(kind, index, "payload exceeds safe limit"));
        }
        if kind != STRING_BYTES {
            records = records
                .checked_add(count as usize)
                .ok_or_else(|| invalid(kind, index, "fixed record count overflows"))?;
            if records > MAX_RECORD_COUNT as usize {
                return Err(AirportDatabaseError::RecordCountExceedsLimit {
                    found: count,
                    maximum: MAX_RECORD_COUNT,
                });
            }
        } else if byte_len > MAX_STRING_BYTES {
            return Err(AirportDatabaseError::StringBytesExceedLimit {
                found: byte_len,
                maximum: MAX_STRING_BYTES,
            });
        }
        result.push(Directory {
            kind,
            offset,
            byte_len,
        });
    }
    if !result.iter().any(|v| v.kind == CORE) {
        return Err(invalid(CORE, 0, "required core section is missing"));
    }
    Ok(result)
}

fn decode(payload: &[u8], dirs: &[Directory]) -> Result<AirportDatabase, AirportDatabaseError> {
    let section = |kind| {
        dirs.iter()
            .find(|v| v.kind == kind)
            .map(|d| &payload[d.offset..d.offset + d.byte_len])
            .unwrap_or(&[])
    };
    let core = section(CORE);
    let header = ParsedHeader {
        version: FORMAT_VERSION_V2,
        count: core.len() / V2_RECORD_LEN,
        record_len: V2_RECORD_LEN,
        payload_len: core.len(),
        expected_len: HEADER_LEN + core.len(),
        checksum: fnv1a(core),
    };
    let base = AirportDatabase::from_payload(&header, core)?;
    let strings = decode_strings(section(STRING_INDEX), section(STRING_BYTES))?;
    let mut referenced = Vec::new();
    referenced
        .try_reserve_exact(strings.len())
        .map_err(|_| allocation(strings.len()))?;
    referenced.resize(strings.len(), false);
    let mut expanded_string_bytes = 0usize;
    let mut taxiways = base.taxiways;
    decode_attributes(
        section(TAXIWAY_ATTRIBUTES),
        &strings,
        &mut referenced,
        &mut expanded_string_bytes,
        &mut taxiways,
    )?;
    let aprons = decode_aprons(section(APRON_TRIANGLES))?;
    let holdings = decode_holdings(
        section(HOLDING_POSITIONS),
        &strings,
        &mut referenced,
        &mut expanded_string_bytes,
    )?;
    let lights = decode_lights(section(GROUND_LIGHTS))?;
    if referenced.iter().any(|v| !v) {
        return Err(invalid(
            STRING_INDEX,
            0,
            "string index contains an orphan entry",
        ));
    }
    AirportDatabase::with_ground_features(base.runways, taxiways, aprons, holdings, lights)
}

fn decode_strings(index: &[u8], bytes: &[u8]) -> Result<Vec<String>, AirportDatabaseError> {
    let count = index.len() / STRING_INDEX_RECORD_LEN;
    let mut out: Vec<String> = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| allocation(allocation_bytes::<String>(count)))?;
    let mut expected = 0usize;
    for (i, record) in index.chunks_exact(STRING_INDEX_RECORD_LEN).enumerate() {
        let offset = read_u32(record, 0) as usize;
        let len = read_u32(record, 4) as usize;
        if offset != expected || len == 0 {
            return Err(invalid(STRING_INDEX, i, "string ranges are not canonical"));
        }
        let end = offset
            .checked_add(len)
            .ok_or_else(|| invalid(STRING_INDEX, i, "string range overflows"))?;
        let raw = bytes
            .get(offset..end)
            .ok_or_else(|| invalid(STRING_INDEX, i, "string range is out of bounds"))?;
        let value = std::str::from_utf8(raw)
            .map_err(|_| invalid(STRING_BYTES, i, "string is not valid UTF-8"))?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(value.len())
            .map_err(|_| allocation(value.len()))?;
        owned.push_str(value);
        if out
            .last()
            .is_some_and(|prior| prior.as_str() >= owned.as_str())
        {
            return Err(invalid(
                STRING_INDEX,
                i,
                "strings are not strictly sorted and unique",
            ));
        }
        out.push(owned);
        expected = end;
    }
    if expected != bytes.len() {
        return Err(invalid(
            STRING_BYTES,
            count,
            "string bytes contain orphan data",
        ));
    }
    Ok(out)
}

fn decode_attributes(
    records: &[u8],
    strings: &[String],
    referenced: &mut [bool],
    expanded_string_bytes: &mut usize,
    taxiways: &mut [AirportTaxiway],
) -> Result<(), AirportDatabaseError> {
    let mut prior = None;
    for (i, r) in records.chunks_exact(ATTRIBUTE_RECORD_LEN).enumerate() {
        if read_u16(r, 14) != 0 || read_u64(r, 16) != 0 {
            return Err(invalid(
                TAXIWAY_ATTRIBUTES,
                i,
                "reserved bytes are non-zero",
            ));
        }
        let id = read_i64(r, 0);
        if prior.is_some_and(|v| v >= id) {
            return Err(invalid(
                TAXIWAY_ATTRIBUTES,
                i,
                "attributes are not sorted and unique",
            ));
        }
        prior = Some(id);
        let reference = decode_reference(
            read_u32(r, 8),
            strings,
            referenced,
            expanded_string_bytes,
            TAXIWAY_ATTRIBUTES,
            i,
        )?;
        let surface = AirportSurface::decode(r[12])
            .ok_or_else(|| invalid(TAXIWAY_ATTRIBUTES, i, "unknown surface"))?;
        let lighting = TaxiwayLighting::decode(r[13])
            .ok_or_else(|| invalid(TAXIWAY_ATTRIBUTES, i, "unknown taxiway lighting"))?;
        if reference.is_none()
            && surface == AirportSurface::Unknown
            && lighting == TaxiwayLighting::None
        {
            return Err(invalid(
                TAXIWAY_ATTRIBUTES,
                i,
                "default attribute record is not canonical",
            ));
        }
        let taxiway = taxiways
            .binary_search_by_key(&id, |v| v.source_way_id)
            .ok()
            .and_then(|n| taxiways.get_mut(n))
            .ok_or(AirportDatabaseError::OrphanTaxiwayReference { source_way_id: id })?;
        taxiway.metadata = TaxiwayMetadata::new(reference, surface, lighting);
    }
    Ok(())
}

fn decode_reference(
    id: u32,
    strings: &[String],
    referenced: &mut [bool],
    expanded_string_bytes: &mut usize,
    section: u16,
    record: usize,
) -> Result<Option<String>, AirportDatabaseError> {
    if id == NONE_STRING {
        return Ok(None);
    }
    let index = id as usize;
    let value = strings
        .get(index)
        .ok_or_else(|| invalid(section, record, "string index is out of bounds"))?;
    *expanded_string_bytes = expanded_string_bytes.checked_add(value.len()).ok_or(
        AirportDatabaseError::StringBytesExceedLimit {
            found: usize::MAX,
            maximum: MAX_STRING_BYTES,
        },
    )?;
    if *expanded_string_bytes > MAX_STRING_BYTES {
        return Err(AirportDatabaseError::StringBytesExceedLimit {
            found: *expanded_string_bytes,
            maximum: MAX_STRING_BYTES,
        });
    }
    referenced[index] = true;
    let mut copy = String::new();
    copy.try_reserve_exact(value.len())
        .map_err(|_| allocation(value.len()))?;
    copy.push_str(value);
    Ok(Some(copy))
}

fn decode_aprons(records: &[u8]) -> Result<Vec<AirportApron>, AirportDatabaseError> {
    let record_count = records.len() / APRON_RECORD_LEN;
    let record_at =
        |index: usize| &records[index * APRON_RECORD_LEN..(index + 1) * APRON_RECORD_LEN];
    let group_count = (0..record_count)
        .filter(|&index| {
            index == 0
                || read_i64(record_at(index - 1), 0) != read_i64(record_at(index), 0)
                || record_at(index - 1)[8] != record_at(index)[8]
        })
        .count();
    let mut out = Vec::new();
    out.try_reserve_exact(group_count)
        .map_err(|_| allocation(allocation_bytes::<AirportApron>(group_count)))?;
    let mut cursor = 0;
    while cursor < record_count {
        let first = record_at(cursor);
        let id = read_i64(first, 0);
        let source = AirportSourceKind::decode(first[8])
            .ok_or_else(|| invalid(APRON_TRIANGLES, cursor, "unknown source kind"))?;
        let surface = AirportSurface::decode(first[9])
            .ok_or_else(|| invalid(APRON_TRIANGLES, cursor, "unknown surface"))?;
        let start = cursor;
        while cursor < record_count
            && read_i64(record_at(cursor), 0) == id
            && record_at(cursor)[8] == first[8]
        {
            cursor += 1;
        }
        let mut triangles = Vec::new();
        triangles
            .try_reserve_exact(cursor - start)
            .map_err(|_| allocation((cursor - start) * 48))?;
        for (expected, record_index) in (start..cursor).enumerate() {
            let r = record_at(record_index);
            if read_u16(r, 10) != 0
                || usize::try_from(read_u32(r, 12)) != Ok(expected)
                || r[9] != first[9]
            {
                return Err(invalid(
                    APRON_TRIANGLES,
                    start + expected,
                    "apron triangle group is not canonical",
                ));
            }
            triangles.push([
                (read_f64(r, 16), read_f64(r, 24)),
                (read_f64(r, 32), read_f64(r, 40)),
                (read_f64(r, 48), read_f64(r, 56)),
            ]);
        }
        out.push(
            AirportApron::from_degree_triangles(source, id, surface, triangles).map_err(
                |source| AirportDatabaseError::InvalidGroundFeature {
                    source_id: id,
                    source,
                },
            )?,
        );
    }
    Ok(out)
}

fn decode_holdings(
    records: &[u8],
    strings: &[String],
    referenced: &mut [bool],
    expanded_string_bytes: &mut usize,
) -> Result<Vec<AirportHoldingPosition>, AirportDatabaseError> {
    let mut out = Vec::new();
    out.try_reserve_exact(records.len() / HOLDING_RECORD_LEN)
        .map_err(|_| allocation(records.len()))?;
    for (i, r) in records.chunks_exact(HOLDING_RECORD_LEN).enumerate() {
        if r[11] & !1 != 0 || read_u64(r, 56) != 0 {
            return Err(invalid(HOLDING_POSITIONS, i, "reserved bytes are non-zero"));
        }
        let source = AirportSourceKind::decode(r[8])
            .ok_or_else(|| invalid(HOLDING_POSITIONS, i, "unknown source kind"))?;
        let kind = HoldingPositionType::decode(r[9])
            .ok_or_else(|| invalid(HOLDING_POSITIONS, i, "unknown holding type"))?;
        let side = RunwaySide::decode(r[10])
            .ok_or_else(|| invalid(HOLDING_POSITIONS, i, "unknown runway side"))?;
        let reference = decode_reference(
            read_u32(r, 12),
            strings,
            referenced,
            expanded_string_bytes,
            HOLDING_POSITIONS,
            i,
        )?;
        let related = if r[11] & 1 == 1 {
            Some(read_i64(r, 48))
        } else {
            if read_i64(r, 48) != 0 {
                return Err(invalid(
                    HOLDING_POSITIONS,
                    i,
                    "absent related taxiway is non-zero",
                ));
            }
            None
        };
        let id = read_i64(r, 0);
        let heading_degrees = read_f64(r, 32);
        if !heading_degrees.is_finite() || !(0.0..360.0).contains(&heading_degrees) {
            return Err(invalid(HOLDING_POSITIONS, i, "heading is not canonical"));
        }
        let heading = Radians(heading_degrees.to_radians());
        out.push(
            AirportHoldingPosition::from_degrees(
                source,
                id,
                (read_f64(r, 16), read_f64(r, 24)),
                kind,
                heading,
                Meters(read_f64(r, 40)),
                reference,
                related,
                side,
            )
            .map_err(|source| AirportDatabaseError::InvalidGroundFeature {
                source_id: id,
                source,
            })?,
        );
    }
    Ok(out)
}

fn decode_lights(records: &[u8]) -> Result<Vec<AirportGroundLight>, AirportDatabaseError> {
    let mut out = Vec::new();
    out.try_reserve_exact(records.len() / LIGHT_RECORD_LEN)
        .map_err(|_| allocation(records.len()))?;
    for (i, r) in records.chunks_exact(LIGHT_RECORD_LEN).enumerate() {
        if r[10..16].iter().any(|v| *v != 0) || read_u64(r, 32) != 0 {
            return Err(invalid(GROUND_LIGHTS, i, "reserved bytes are non-zero"));
        }
        let source = AirportSourceKind::decode(r[8])
            .ok_or_else(|| invalid(GROUND_LIGHTS, i, "unknown source kind"))?;
        let kind = GroundLightKind::decode(r[9])
            .ok_or_else(|| invalid(GROUND_LIGHTS, i, "unknown light kind"))?;
        let id = read_i64(r, 0);
        out.push(
            AirportGroundLight::from_degrees(source, id, (read_f64(r, 16), read_f64(r, 24)), kind)
                .map_err(|source| AirportDatabaseError::InvalidGroundFeature {
                    source_id: id,
                    source,
                })?,
        );
    }
    Ok(out)
}

fn invalid(section_kind: u16, record_index: usize, message: &'static str) -> AirportDatabaseError {
    AirportDatabaseError::InvalidV3 {
        section_kind,
        record_index,
        message,
    }
}
fn allocation(requested: usize) -> AirportDatabaseError {
    AirportDatabaseError::AllocationFailed { requested }
}
fn reserve_vec(size: usize) -> Result<Vec<u8>, AirportDatabaseError> {
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|_| allocation(size))?;
    Ok(out)
}

fn checked_bytes(count: usize, record_len: usize) -> Result<usize, AirportDatabaseError> {
    count
        .checked_mul(record_len)
        .ok_or(AirportDatabaseError::TooManyRecords { count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::Degrees;
    use std::io::Cursor;

    fn taxiway(id: i64, reference: &str) -> AirportTaxiway {
        taxiway_with_reference(id, reference.to_owned())
    }

    fn taxiway_with_reference(id: i64, reference: String) -> AirportTaxiway {
        AirportTaxiway::from_points_with_metadata(
            id,
            vec![
                Geodetic::from_degrees(35.0, 139.0, 20.0),
                Geodetic::from_degrees(35.001, 139.002, -20.0),
                Geodetic::from_degrees(35.003, 139.004, 0.0),
            ],
            Meters(15.0),
            TaxiwayMetadata::new(
                Some(reference),
                AirportSurface::Asphalt,
                TaxiwayLighting::EdgeAndCenterline,
            ),
        )
        .expect("valid taxiway")
    }

    fn apron(id: i64) -> AirportApron {
        AirportApron::new(
            AirportSourceKind::Way,
            id,
            AirportSurface::Concrete,
            vec![[
                Geodetic::from_degrees(35.0, 139.0, 9.0),
                Geodetic::from_degrees(35.0, 139.001, 8.0),
                Geodetic::from_degrees(35.001, 139.0, 7.0),
            ]],
        )
        .expect("valid apron")
    }

    fn holding(id: i64, taxiway: i64) -> AirportHoldingPosition {
        AirportHoldingPosition::new(
            AirportSourceKind::Node,
            id,
            Geodetic::from_degrees(35.001, 139.002, 100.0),
            HoldingPositionType::Runway,
            Degrees(90.0).to_radians(),
            Meters(8.0),
            Some("H1".to_owned()),
            Some(taxiway),
            RunwaySide::Forward,
        )
        .expect("valid holding position")
    }

    fn light(id: i64) -> AirportGroundLight {
        AirportGroundLight::new(
            AirportSourceKind::Node,
            id,
            Geodetic::from_degrees(35.002, 139.003, 50.0),
            GroundLightKind::RunwayGuard,
        )
        .expect("valid light")
    }

    fn runway(id: i64, latitude: f64) -> AirportRunway {
        AirportRunway::from_endpoints(
            id,
            Geodetic::from_degrees(latitude, 139.0, 0.0),
            Geodetic::from_degrees(latitude + 0.01, 139.0, 0.0),
            Meters(45.0),
        )
        .expect("valid runway")
    }

    fn sample_v3(reverse: bool) -> AirportDatabase {
        let mut taxiways = vec![taxiway(20, "B"), taxiway(10, "A")];
        let mut aprons = vec![apron(40), apron(30)];
        let mut holdings = vec![holding(60, 20), holding(50, 10)];
        let mut lights = vec![light(80), light(70)];
        if reverse {
            taxiways.reverse();
            aprons.reverse();
            holdings.reverse();
            lights.reverse();
        }
        AirportDatabase::with_ground_features(Vec::new(), taxiways, aprons, holdings, lights)
            .expect("valid v3 database")
    }

    fn directory(bytes: &[u8], kind: u16) -> usize {
        let count = read_u32(bytes, 8) as usize;
        (0..count)
            .map(|index| HEADER_LEN + index * DIRECTORY_ENTRY_LEN)
            .find(|&offset| read_u16(bytes, offset) == kind)
            .expect("section exists")
    }

    fn section_offset(bytes: &[u8], kind: u16) -> usize {
        HEADER_LEN
            + usize::try_from(read_u64(bytes, directory(bytes, kind) + 16))
                .expect("test section offset fits usize")
    }

    fn refresh_checksum(bytes: &mut [u8]) {
        let checksum = fnv1a(&bytes[HEADER_LEN..]);
        bytes[16..24].copy_from_slice(&checksum.to_le_bytes());
    }

    #[test]
    fn v3_roundtrip_is_byte_stable_and_preserves_all_features() {
        let database = sample_v3(false);
        let bytes = database.to_bytes().expect("encode v3");
        assert_eq!(read_u16(&bytes, 4), FORMAT_VERSION_V3);
        assert_eq!(read_u32(&bytes, 8), 7);
        assert_eq!(read_u32(&bytes, 12), DIRECTORY_ENTRY_LEN_FIELD);

        let restored = AirportDatabase::from_bytes(&bytes).expect("decode v3");
        assert_eq!(restored, database);
        assert_eq!(restored.to_bytes().expect("re-encode v3"), bytes);
        assert_eq!(restored.aprons().len(), 2);
        assert_eq!(
            restored.holding_positions()[0].runway_side(),
            RunwaySide::Forward
        );
        assert_eq!(
            restored.ground_lights()[0].kind(),
            GroundLightKind::RunwayGuard
        );
        assert_eq!(restored.taxiways()[0].reference(), Some("A"));

        let mut cursor = Cursor::new(bytes);
        assert_eq!(
            AirportDatabase::read_from(&mut cursor).expect("stream v3"),
            database
        );
    }

    #[test]
    fn v3_serialization_is_independent_of_input_order() {
        assert_eq!(
            sample_v3(false).to_bytes().expect("first ordering"),
            sample_v3(true).to_bytes().expect("reverse ordering")
        );
    }

    #[test]
    fn independently_hand_built_v3_light_fixture_is_accepted() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&CORE.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&V2_RECORD_LEN_FIELD.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&64u64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&GROUND_LIGHTS.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&OPTIONAL_SECTION.to_le_bytes());
        payload.extend_from_slice(&LIGHT_RECORD_LEN_FIELD.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&64u64.to_le_bytes());
        payload.extend_from_slice(&40u64.to_le_bytes());
        payload.extend_from_slice(&77i64.to_le_bytes());
        payload.push(AirportSourceKind::Node as u8);
        payload.push(GroundLightKind::TaxiwayEdge as u8);
        payload.extend_from_slice(&[0; 6]);
        payload.extend_from_slice(&35.0f64.to_le_bytes());
        payload.extend_from_slice(&139.0f64.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        const INDEPENDENT_CHECKSUM: u64 = 0x88e6_6da3_d549_5a4d;
        assert_eq!(fnv1a(&payload), INDEPENDENT_CHECKSUM);

        let mut fixture = Vec::new();
        fixture.extend_from_slice(&MAGIC);
        fixture.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
        fixture.extend_from_slice(&0u16.to_le_bytes());
        fixture.extend_from_slice(&2u32.to_le_bytes());
        fixture.extend_from_slice(&DIRECTORY_ENTRY_LEN_FIELD.to_le_bytes());
        fixture.extend_from_slice(&INDEPENDENT_CHECKSUM.to_le_bytes());
        fixture.extend_from_slice(&payload);

        let database = AirportDatabase::from_bytes(&fixture).expect("hand-built fixture");
        assert_eq!(database.ground_lights().len(), 1);
        assert_eq!(database.ground_lights()[0].source_id(), 77);
    }

    #[test]
    fn unknown_optional_sections_are_skipped_but_required_ones_are_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION_V3.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&DIRECTORY_ENTRY_LEN_FIELD.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        let mut payload = vec![0u8; 64];
        payload[0..2].copy_from_slice(&CORE.to_le_bytes());
        payload[2..4].copy_from_slice(&1u16.to_le_bytes());
        payload[8..12].copy_from_slice(&V2_RECORD_LEN_FIELD.to_le_bytes());
        payload[16..24].copy_from_slice(&64u64.to_le_bytes());
        payload[32..34].copy_from_slice(&99u16.to_le_bytes());
        payload[34..36].copy_from_slice(&42u16.to_le_bytes());
        payload[36..40].copy_from_slice(&OPTIONAL_SECTION.to_le_bytes());
        payload[40..44].copy_from_slice(&8u32.to_le_bytes());
        payload[48..56].copy_from_slice(&64u64.to_le_bytes());
        bytes.extend_from_slice(&payload);
        refresh_checksum(&mut bytes);
        assert!(AirportDatabase::from_bytes(&bytes).is_ok());

        bytes[HEADER_LEN + DIRECTORY_ENTRY_LEN + 4..HEADER_LEN + DIRECTORY_ENTRY_LEN + 8]
            .copy_from_slice(&0u32.to_le_bytes());
        refresh_checksum(&mut bytes);
        assert!(matches!(
            AirportDatabase::from_bytes(&bytes),
            Err(AirportDatabaseError::InvalidV3 {
                message: "unknown required section",
                ..
            })
        ));
    }

    #[test]
    fn v3_rejects_checksum_bounds_reserved_utf8_and_orphan_strings() {
        let original = sample_v3(false).to_bytes().expect("encode v3");

        let mut checksum = original.clone();
        *checksum.last_mut().expect("nonempty") ^= 1;
        assert!(matches!(
            AirportDatabase::from_bytes(&checksum),
            Err(AirportDatabaseError::ChecksumMismatch { .. })
        ));

        let mut bounds = original.clone();
        let light_directory = directory(&bounds, GROUND_LIGHTS);
        bounds[light_directory + 16..light_directory + 24].copy_from_slice(&0u64.to_le_bytes());
        refresh_checksum(&mut bounds);
        assert!(matches!(
            AirportDatabase::from_bytes(&bounds),
            Err(AirportDatabaseError::InvalidV3 { .. })
        ));

        let mut reserved = original.clone();
        let light = section_offset(&reserved, GROUND_LIGHTS);
        reserved[light + 10] = 1;
        refresh_checksum(&mut reserved);
        assert!(matches!(
            AirportDatabase::from_bytes(&reserved),
            Err(AirportDatabaseError::InvalidV3 {
                section_kind: GROUND_LIGHTS,
                ..
            })
        ));

        let mut flags = original.clone();
        let apron_directory = directory(&flags, APRON_TRIANGLES);
        flags[apron_directory + 4..apron_directory + 8].copy_from_slice(&0u32.to_le_bytes());
        refresh_checksum(&mut flags);
        assert!(matches!(
            AirportDatabase::from_bytes(&flags),
            Err(AirportDatabaseError::InvalidV3 {
                message: "known section has non-canonical flags",
                ..
            })
        ));

        let mut utf8 = original.clone();
        let strings = section_offset(&utf8, STRING_BYTES);
        utf8[strings] = 0xff;
        refresh_checksum(&mut utf8);
        assert!(matches!(
            AirportDatabase::from_bytes(&utf8),
            Err(AirportDatabaseError::InvalidV3 {
                section_kind: STRING_BYTES,
                ..
            })
        ));

        let mut orphan = original;
        let attributes = section_offset(&orphan, TAXIWAY_ATTRIBUTES);
        orphan[attributes + 8..attributes + 12].copy_from_slice(&NONE_STRING.to_le_bytes());
        refresh_checksum(&mut orphan);
        assert!(matches!(
            AirportDatabase::from_bytes(&orphan),
            Err(AirportDatabaseError::InvalidV3 {
                section_kind: STRING_INDEX,
                ..
            })
        ));
    }

    #[test]
    fn duplicate_feature_ids_and_orphan_related_taxiways_are_rejected() {
        assert!(matches!(
            AirportDatabase::with_ground_features(
                vec![runway(5, 35.0), runway(5, 36.0)],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new()
            ),
            Err(AirportDatabaseError::DuplicateRunwayWayId { source_way_id: 5 })
        ));
        assert!(matches!(
            AirportDatabase::with_ground_features(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec![light(1), light(1)]
            ),
            Err(AirportDatabaseError::DuplicateGroundFeature {
                section_kind: GROUND_LIGHTS,
                ..
            })
        ));
        assert!(matches!(
            AirportDatabase::with_ground_features(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec![holding(1, 999)],
                Vec::new()
            ),
            Err(AirportDatabaseError::OrphanTaxiwayReference { source_way_id: 999 })
        ));
    }

    #[test]
    fn expanded_reference_bytes_are_bounded_on_construct_and_encode() {
        let half_limit = "x".repeat(MAX_STRING_BYTES / 2);
        let database = AirportDatabase::with_ground_features(
            Vec::new(),
            vec![
                taxiway_with_reference(1, half_limit.clone()),
                taxiway_with_reference(2, half_limit),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("the exact expanded-string limit is valid");
        let mut expanded_input = database
            .to_bytes()
            .expect("the encoder accepts the exact expanded-string limit");
        let string_index = section_offset(&expanded_input, STRING_INDEX);
        let string_bytes_directory = directory(&expanded_input, STRING_BYTES);
        let expanded_length = u32::try_from(MAX_STRING_BYTES / 2 + 1).expect("limit fits u32");
        expanded_input[string_index + 4..string_index + 8]
            .copy_from_slice(&expanded_length.to_le_bytes());
        expanded_input[string_bytes_directory + 12..string_bytes_directory + 16]
            .copy_from_slice(&expanded_length.to_le_bytes());
        expanded_input[string_bytes_directory + 24..string_bytes_directory + 32]
            .copy_from_slice(&u64::from(expanded_length).to_le_bytes());
        expanded_input.push(b'x');
        refresh_checksum(&mut expanded_input);
        assert!(matches!(
            AirportDatabase::from_bytes(&expanded_input),
            Err(AirportDatabaseError::StringBytesExceedLimit {
                found,
                maximum: MAX_STRING_BYTES,
            }) if found == MAX_STRING_BYTES + 2
        ));

        let over_half = "x".repeat(MAX_STRING_BYTES / 2 + 1);
        assert!(matches!(
            AirportDatabase::with_ground_features(
                Vec::new(),
                vec![
                    taxiway_with_reference(1, over_half.clone()),
                    taxiway_with_reference(2, over_half),
                ],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            Err(AirportDatabaseError::StringBytesExceedLimit {
                found,
                maximum: MAX_STRING_BYTES,
            }) if found == MAX_STRING_BYTES + 2
        ));
    }

    #[test]
    fn one_apron_with_many_triangles_decodes_as_one_group() {
        let mut triangles = Vec::new();
        for index in 0..1_024 {
            let offset = f64::from(index) * 0.000_01;
            triangles.push([
                Geodetic::from_degrees(35.0 + offset, 139.0, 0.0),
                Geodetic::from_degrees(35.0 + offset, 139.000_001, 0.0),
                Geodetic::from_degrees(35.000_001 + offset, 139.0, 0.0),
            ]);
        }
        let apron = AirportApron::new(
            AirportSourceKind::Way,
            99,
            AirportSurface::Concrete,
            triangles,
        )
        .expect("many valid triangles");
        let database = AirportDatabase::with_ground_features(
            Vec::new(),
            Vec::new(),
            vec![apron],
            Vec::new(),
            Vec::new(),
        )
        .expect("one apron group");
        let restored = AirportDatabase::from_bytes(&database.to_bytes().expect("encode"))
            .expect("decode one large apron group");
        assert_eq!(restored.aprons().len(), 1);
        assert_eq!(restored.aprons()[0].triangles().len(), 1_024);
    }
}
