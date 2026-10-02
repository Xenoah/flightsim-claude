//! Compact, offline-baked, complete-planet terrain beneath regional DEM tiles.
//!
//! The bundled atlas contains real orthometric relief, an independent land mask,
//! and the matching EGM2008 geoid. The sample contract is WGS84 ellipsoidal metres:
//! `h = H + N`. Ocean samples use `H = 0`, not the bathymetric sea floor. Below-sea-
//! level land is retained. Coast cells blend the land and ocean surface smoothly.
//!
//! This is a low-resolution navigation/visual baseline, not a substitute for
//! surveyed airport terrain. It has no network, credentials, GeoTIFF parser, or
//! unbounded cache. Every generated tile has exactly 33 × 33 samples. Callers must
//! charge generation against their existing per-frame tile/mesh budget.

use crate::{DemTile, HeightGrid, TerrainError, TileId, TileSource};
use flightsim_core::{Degrees, Geodetic, Meters, Radians};
use std::sync::{Arc, OnceLock};

include!("global_metadata.rs");
/// Fixed work and allocation bound for a generated terrain tile.
pub const GLOBAL_TILE_GRID_SIZE: u32 = 33;
/// Finer meshes preserve Earth curvature without claiming finer source relief.
pub const MAX_GLOBAL_TILE_LEVEL: u8 = 13;
/// Header length of the little-endian FSGT v2 format.
pub const GLOBAL_HEADER_BYTES: usize = 64;
/// Bound checked before interpreting any dimensions or allocating atlas storage.
pub const MAX_GLOBAL_ATLAS_BYTES: usize = 40 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"FSGT";
const VERSION: u16 = 2;
const MAX_HEIGHT: u32 = 2_048;
const ORTHOMETRIC_LIMIT: i16 = 12_000;
const GEOID_LIMIT_CM: i16 = 20_000;
const BUNDLED_BYTES: &[u8] = include_bytes!("../data/global-terrain.fsgt");

/// A malformed or unsupported atlas. Corrupt data never silently becomes ocean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDataError(pub &'static str);

impl core::fmt::Display for GlobalDataError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for GlobalDataError {}

/// A continuous sample of the low-resolution world surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalTerrainSample {
    /// WGS84 ellipsoidal surface height (`elevation_msl + geoid_undulation`).
    pub surface_height: Meters,
    /// EGM2008 orthometric surface height. Ocean interiors are zero; coast cells
    /// blend land with sea level. Inland waters retain their corrected lake
    /// levels. Negative dry-land and lake surface elevations remain negative.
    pub elevation_msl: Meters,
    /// Matching geoid height above the WGS84 ellipsoid.
    pub geoid_undulation: Meters,
    /// Independent geographic dry-land mask, never inferred from height sign.
    /// Both inland water and ocean are false.
    pub is_land: bool,
    /// Independently mapped inland water (lake/reservoir), retaining its actual
    /// surface height rather than being forced to ocean sea level.
    pub is_inland_water: bool,
    /// Bilinearly interpolated mask in `[0, 1]`, useful for shoreline colour.
    pub land_fraction: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Channels {
    surface_msl: f64,
    geoid: f64,
    land: f64,
    inland_water: f64,
}

impl Channels {
    fn blend(self, other: Self, amount: f64) -> Self {
        Self {
            surface_msl: self.surface_msl + (other.surface_msl - self.surface_msl) * amount,
            geoid: self.geoid + (other.geoid - self.geoid) * amount,
            land: self.land + (other.land - self.land) * amount,
            inland_water: self.inland_water + (other.inland_water - self.inland_water) * amount,
        }
    }

    fn sample(self) -> GlobalTerrainSample {
        GlobalTerrainSample {
            surface_height: Meters(self.surface_msl + self.geoid),
            elevation_msl: Meters(self.surface_msl),
            geoid_undulation: Meters(self.geoid),
            is_land: self.land >= 0.5,
            is_inland_water: self.land < 0.5 && self.inland_water >= 0.5,
            land_fraction: self.land.clamp(0.0, 1.0),
        }
    }
}

/// Validated compact atlas. Cloning shares immutable bytes, including for a
/// renderer, map and ground sampler running independently.
#[derive(Debug, Clone)]
pub struct GlobalTerrain {
    bytes: Arc<[u8]>,
    width: u32,
    height: u32,
    longitude_origin: Radians,
    north_latitude: Radians,
    fingerprint: u64,
    north_pole: Channels,
    south_pole: Channels,
}

impl GlobalTerrain {
    /// Decode and validate the bundled atlas once, then cheaply share it.
    ///
    /// # Errors
    /// A corrupt or incompatible bundled asset is reported, never replaced by a
    /// synthetic globe. An altered bake requires an updated fingerprint.
    pub fn bundled() -> Result<Self, GlobalDataError> {
        static ATLAS: OnceLock<Result<GlobalTerrain, GlobalDataError>> = OnceLock::new();
        ATLAS
            .get_or_init(|| {
                let atlas = Self::from_bytes(BUNDLED_BYTES.to_vec())?;
                if atlas.fingerprint != GLOBAL_TERRAIN_FINGERPRINT {
                    return Err(GlobalDataError(
                        "bundled global terrain fingerprint mismatch",
                    ));
                }
                Ok(atlas)
            })
            .clone()
    }

    /// Validate one complete FSGT v2 byte stream, including header semantics,
    /// arithmetic bounds, checksum, all samples and unused mask bits.
    ///
    /// # Errors
    /// Malformed, truncated, oversized, unsupported or trailing data is rejected.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, GlobalDataError> {
        let invalid = GlobalDataError;
        if bytes.len() < GLOBAL_HEADER_BYTES || bytes.len() > MAX_GLOBAL_ATLAS_BYTES {
            return Err(invalid("global terrain byte length outside limits"));
        }
        if &bytes[..4] != MAGIC || u16_at(&bytes, 4) != VERSION {
            return Err(invalid("unsupported global terrain magic or version"));
        }
        if u16_at(&bytes, 6) != 0 || bytes[48..64].iter().any(|byte| *byte != 0) {
            return Err(invalid("nonzero global terrain flags or reserved bytes"));
        }
        // Horizontal WGS84, source EGM2008 height, target WGS84 ellipsoid,
        // explicit periodic sample origin. Other datums must get a new format.
        if u16_at(&bytes, 16) != 4326
            || u16_at(&bytes, 18) != 3855
            || u16_at(&bytes, 20) != 4979
            || u16_at(&bytes, 22) != 1
        {
            return Err(invalid(
                "unsupported global terrain CRS, datum or registration",
            ));
        }
        let width = u32_at(&bytes, 8);
        let height = u32_at(&bytes, 12);
        if !(2..=MAX_HEIGHT).contains(&height) || width != height * 2 {
            return Err(invalid(
                "global terrain requires width = 2 * height, height 2..=2048",
            ));
        }
        let count = width as usize * height as usize;
        let mask_bytes = count.div_ceil(8);
        let mask_offset = GLOBAL_HEADER_BYTES + count * 4;
        let expected = mask_offset + mask_bytes * 2;
        if bytes.len() != expected {
            return Err(invalid(
                "global terrain payload length mismatch or trailing bytes",
            ));
        }
        let lon = f64_at(&bytes, 32);
        let north = f64_at(&bytes, 40);
        let latitude_step = 180.0 / f64::from(height);
        if !lon.is_finite()
            || !(-180.0..180.0).contains(&lon)
            || !north.is_finite()
            || north >= 90.0
            || north <= 90.0 - latitude_step
        {
            return Err(invalid("invalid global terrain sample origin"));
        }
        let checksum = u64_at(&bytes, 24);
        if global_atlas_fingerprint(&bytes) != checksum {
            return Err(invalid("global terrain checksum mismatch"));
        }
        for index in 0..count {
            let offset = GLOBAL_HEADER_BYTES + index * 4;
            let height = i16_at(&bytes, offset);
            let geoid = i16_at(&bytes, offset + 2);
            let land = (bytes[mask_offset + index / 8] >> (index % 8)) & 1;
            let inland_water = (bytes[mask_offset + mask_bytes + index / 8] >> (index % 8)) & 1;
            if land == 1 && inland_water == 1 {
                return Err(invalid("global dry-land and inland-water masks overlap"));
            }
            if !(-ORTHOMETRIC_LIMIT..=ORTHOMETRIC_LIMIT).contains(&height)
                || !(-GEOID_LIMIT_CM..=GEOID_LIMIT_CM).contains(&geoid)
                || ((land == 1 || inland_water == 1) && height < -500)
            {
                return Err(invalid(
                    "global terrain sample outside physical surface bounds",
                ));
            }
        }
        let remainder = count % 8;
        if remainder != 0
            && (bytes[mask_offset + mask_bytes - 1] >> remainder != 0
                || bytes[expected - 1] >> remainder != 0)
        {
            return Err(invalid("nonzero global terrain unused mask bits"));
        }
        let mut atlas = Self {
            bytes: bytes.into(),
            width,
            height,
            longitude_origin: Degrees(lon).to_radians(),
            north_latitude: Degrees(north).to_radians(),
            fingerprint: checksum,
            north_pole: Channels::default(),
            south_pole: Channels::default(),
        };
        // The polar endpoints must be single points, independent of longitude.
        // Averaging the nearest row is an explicit low-resolution polar policy.
        atlas.north_pole = atlas.row_mean(0);
        atlas.south_pole = atlas.row_mean(height - 1);
        Ok(atlas)
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Sample origin and positive eastward/southward angular spacing.
    #[must_use]
    pub fn grid_geometry(&self) -> (Radians, Radians, Radians) {
        (
            self.longitude_origin,
            self.north_latitude,
            Radians(core::f64::consts::PI / f64::from(self.height)),
        )
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[must_use]
    pub fn memory_footprint(&self) -> usize {
        self.bytes.len()
    }

    /// Constant-work bilinear sample with periodic longitude and continuous pole
    /// caps. Longitude may wrap any finite number of turns. Latitude must be in
    /// `[-90°, 90°]`; altitude is intentionally ignored.
    #[must_use]
    pub fn sample(&self, position: Geodetic) -> Option<GlobalTerrainSample> {
        use core::f64::consts::{FRAC_PI_2, PI, TAU};
        let latitude = position.latitude.get();
        let longitude = position.longitude.get();
        if !latitude.is_finite()
            || !longitude.is_finite()
            || !(-FRAC_PI_2..=FRAC_PI_2).contains(&latitude)
        {
            return None;
        }
        let step = PI / f64::from(self.height);
        // Reduce before subtraction so even very large finite angles cannot
        // overflow. The final remainder maps the last column to the first.
        let x = (longitude.rem_euclid(TAU) - self.longitude_origin.get()).rem_euclid(TAU) / step;
        #[allow(clippy::cast_possible_truncation)]
        let column = (x.floor() as u32).min(self.width - 1);
        let fraction_x = (x - f64::from(column)).clamp(0.0, 1.0);
        let row_sample = |row| {
            self.channels(column, row)
                .blend(self.channels((column + 1) % self.width, row), fraction_x)
        };
        let north = self.north_latitude.get();
        let south = north - step * f64::from(self.height - 1);
        let channels = if latitude > north {
            row_sample(0).blend(
                self.north_pole,
                ((latitude - north) / (FRAC_PI_2 - north)).clamp(0.0, 1.0),
            )
        } else if latitude < south {
            row_sample(self.height - 1).blend(
                self.south_pole,
                ((south - latitude) / (south + FRAC_PI_2)).clamp(0.0, 1.0),
            )
        } else {
            let y = (north - latitude) / step;
            #[allow(clippy::cast_possible_truncation)]
            let row = (y.floor() as u32).min(self.height - 2);
            row_sample(row).blend(row_sample(row + 1), (y - f64::from(row)).clamp(0.0, 1.0))
        };
        Some(channels.sample())
    }

    /// Generate exactly one bounded 33 × 33 DEM, using only this resident atlas.
    /// This method has no disk IO. Invalid IDs and levels above 13 return None.
    #[must_use]
    pub fn tile(&self, id: TileId) -> Option<DemTile> {
        if id.level > MAX_GLOBAL_TILE_LEVEL
            || id.x >= TileId::columns(id.level)
            || id.y >= TileId::rows(id.level)
        {
            return None;
        }
        let bounds = id.bounds();
        let intervals = GLOBAL_TILE_GRID_SIZE - 1;
        let mut values =
            Vec::with_capacity((GLOBAL_TILE_GRID_SIZE * GLOBAL_TILE_GRID_SIZE) as usize);
        for row in 0..GLOBAL_TILE_GRID_SIZE {
            let latitude =
                bounds.north.get() - bounds.height().get() * f64::from(row) / f64::from(intervals);
            for column in 0..GLOBAL_TILE_GRID_SIZE {
                let longitude = bounds.west.get()
                    + bounds.width().get() * f64::from(column) / f64::from(intervals);
                let sample = self.sample(Geodetic::new(
                    // Roundoff in polar tile subtraction can exceed ±π/2 by an
                    // ulp. TileId geometry guarantees the intended domain.
                    Radians(
                        latitude.clamp(-core::f64::consts::FRAC_PI_2, core::f64::consts::FRAC_PI_2),
                    ),
                    Radians(longitude),
                    Meters::ZERO,
                ))?;
                #[allow(clippy::cast_possible_truncation)]
                values.push(sample.surface_height.get() as f32);
            }
        }
        Some(DemTile::new(
            bounds,
            HeightGrid::new(GLOBAL_TILE_GRID_SIZE, GLOBAL_TILE_GRID_SIZE, values),
        ))
    }

    fn channels(&self, column: u32, row: u32) -> Channels {
        let index = row as usize * self.width as usize + column as usize;
        let offset = GLOBAL_HEADER_BYTES + index * 4;
        let mask_offset = GLOBAL_HEADER_BYTES + self.width as usize * self.height as usize * 4;
        let mask_bytes = (self.width as usize * self.height as usize).div_ceil(8);
        let land = (self.bytes[mask_offset + index / 8] >> (index % 8)) & 1;
        let inland_water = (self.bytes[mask_offset + mask_bytes + index / 8] >> (index % 8)) & 1;
        Channels {
            surface_msl: if land == 1 || inland_water == 1 {
                f64::from(i16_at(&self.bytes, offset))
            } else {
                0.0
            },
            geoid: f64::from(i16_at(&self.bytes, offset + 2)) / 100.0,
            land: f64::from(land),
            inland_water: f64::from(inland_water),
        }
    }

    fn row_mean(&self, row: u32) -> Channels {
        let mut result = Channels::default();
        for column in 0..self.width {
            let value = self.channels(column, row);
            result.surface_msl += value.surface_msl;
            result.geoid += value.geoid;
            result.land += value.land;
            result.inland_water += value.inland_water;
        }
        let count = f64::from(self.width);
        result.surface_msl /= count;
        result.geoid /= count;
        result.land /= count;
        result.inland_water /= count;
        result
    }
}

/// Regional DEMs remain primary at *all* levels. The global layer is a separate
/// fallback pass, preventing a generated fine tile from hiding a real ancestor.
#[derive(Debug, Clone)]
pub struct GlobalTileSource<S> {
    primary: S,
    global: GlobalTerrain,
}

impl<S> GlobalTileSource<S> {
    #[must_use]
    pub const fn new(primary: S, global: GlobalTerrain) -> Self {
        Self { primary, global }
    }

    #[must_use]
    pub const fn primary(&self) -> &S {
        &self.primary
    }

    #[must_use]
    pub const fn global(&self) -> &GlobalTerrain {
        &self.global
    }
}

impl<S: TileSource> TileSource for GlobalTileSource<S> {
    fn load(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        self.primary.load(id)
    }

    fn primary_reads_possible(&self) -> bool {
        self.primary.primary_reads_possible()
    }

    fn has_fallback(&self) -> bool {
        true
    }

    fn load_fallback(&self, id: TileId) -> Result<Option<DemTile>, TerrainError> {
        Ok(self.global.tile(id))
    }

    fn fallback_elevation_at(&self, position: Geodetic) -> Option<Meters> {
        self.global
            .sample(position)
            .map(|sample| sample.surface_height)
    }
}

/// FNV-1a content identity over all bytes except the checksum slot itself.
/// The header's geometry and datum are protected as well as the payload.
#[must_use]
pub fn global_atlas_fingerprint(bytes: &[u8]) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for (index, byte) in bytes.iter().enumerate() {
        if !(24..32).contains(&index) {
            value ^= u64::from(*byte);
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    value
}

fn u16_at(bytes: &[u8], index: usize) -> u16 {
    u16::from_le_bytes([bytes[index], bytes[index + 1]])
}
fn i16_at(bytes: &[u8], index: usize) -> i16 {
    i16::from_le_bytes([bytes[index], bytes[index + 1]])
}
fn u32_at(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(
        bytes[index..index + 4]
            .try_into()
            .expect("validated field range"),
    )
}
fn u64_at(bytes: &[u8], index: usize) -> u64 {
    u64::from_le_bytes(
        bytes[index..index + 8]
            .try_into()
            .expect("validated field range"),
    )
}
fn f64_at(bytes: &[u8], index: usize) -> f64 {
    f64::from_le_bytes(
        bytes[index..index + 8]
            .try_into()
            .expect("validated field range"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryTileSource, Terrain};
    use std::cell::Cell;

    fn encode_fixture(height: u32, origin: (f64, f64), values: &[(i16, i16, bool)]) -> Vec<u8> {
        let width = height * 2;
        assert_eq!(values.len(), width as usize * height as usize);
        let mut bytes =
            vec![0_u8; GLOBAL_HEADER_BYTES + values.len() * 4 + values.len().div_ceil(8) * 2];
        bytes[..4].copy_from_slice(MAGIC);
        bytes[4..6].copy_from_slice(&VERSION.to_le_bytes());
        bytes[8..12].copy_from_slice(&width.to_le_bytes());
        bytes[12..16].copy_from_slice(&height.to_le_bytes());
        for (offset, value) in [(16, 4326_u16), (18, 3855), (20, 4979), (22, 1)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes[32..40].copy_from_slice(&origin.0.to_le_bytes());
        bytes[40..48].copy_from_slice(&origin.1.to_le_bytes());
        let mask_offset = GLOBAL_HEADER_BYTES + values.len() * 4;
        for (index, &(elevation, geoid, land)) in values.iter().enumerate() {
            let offset = GLOBAL_HEADER_BYTES + index * 4;
            bytes[offset..offset + 2].copy_from_slice(&elevation.to_le_bytes());
            bytes[offset + 2..offset + 4].copy_from_slice(&geoid.to_le_bytes());
            if land {
                bytes[mask_offset + index / 8] |= 1 << (index % 8);
            }
        }
        reseal(&mut bytes);
        bytes
    }

    fn reseal(bytes: &mut [u8]) {
        let hash = global_atlas_fingerprint(bytes);
        bytes[24..32].copy_from_slice(&hash.to_le_bytes());
    }

    fn varying() -> GlobalTerrain {
        let values: Vec<_> = (0_i16..32)
            .map(|index| (index * 100 - 500, index * 10 - 1_000, index % 3 != 0))
            .collect();
        GlobalTerrain::from_bytes(encode_fixture(4, (-157.5, 67.5), &values)).unwrap()
    }

    #[test]
    fn orthometric_plus_geoid_is_ellipsoidal_and_negative_land_is_retained() {
        let mut values = vec![(1_000, 3_250, true); 8];
        values[0] = (-420, 3_250, true);
        values[1] = (-5_000, -2_000, false);
        let atlas = GlobalTerrain::from_bytes(encode_fixture(2, (-135.0, 45.0), &values)).unwrap();
        let depression = atlas
            .sample(Geodetic::from_degrees(45.0, -135.0, 0.0))
            .unwrap();
        assert!(depression.is_land);
        assert_eq!(depression.elevation_msl, Meters(-420.0));
        assert_eq!(depression.surface_height, Meters(-387.5));
        let sea = atlas
            .sample(Geodetic::from_degrees(45.0, -45.0, 0.0))
            .unwrap();
        assert!(!sea.is_land);
        assert_eq!(sea.elevation_msl, Meters::ZERO);
        assert_eq!(sea.surface_height, Meters(-20.0));
    }

    #[test]
    fn inland_water_keeps_lake_level_and_is_not_dry_land_or_ocean() {
        let mut bytes = encode_fixture(2, (-135.0, 45.0), &[(455, 3_000, false); 8]);
        let lake_mask_offset = GLOBAL_HEADER_BYTES + 8 * 4 + 1;
        bytes[lake_mask_offset] = 0xff;
        reseal(&mut bytes);
        let atlas = GlobalTerrain::from_bytes(bytes.clone()).unwrap();
        let sample = atlas
            .sample(Geodetic::from_degrees(45.0, -135.0, 0.0))
            .unwrap();
        assert!(!sample.is_land && sample.is_inland_water);
        assert_eq!(sample.elevation_msl, Meters(455.0));
        assert_eq!(sample.surface_height, Meters(485.0));
        bytes[GLOBAL_HEADER_BYTES + 8 * 4] = 1;
        reseal(&mut bytes);
        assert!(
            GlobalTerrain::from_bytes(bytes).is_err(),
            "overlapping water/land masks must fail"
        );
        let mut submerged_land = encode_fixture(2, (-135.0, 45.0), &[(-501, 0, true); 8]);
        reseal(&mut submerged_land);
        assert!(
            GlobalTerrain::from_bytes(submerged_land).is_err(),
            "unexplained deep bathymetry must not pass as land"
        );
    }

    #[test]
    fn explicit_nonstandard_origin_is_not_assumed_to_be_a_cell_center() {
        let values: Vec<_> = (0_i16..8).map(|index| (index * 100, 0, true)).collect();
        let atlas = GlobalTerrain::from_bytes(encode_fixture(2, (0.5, 89.5), &values)).unwrap();
        for row in 0..2_u32 {
            for column in 0..4_u32 {
                let position = Geodetic::from_degrees(
                    89.5 - f64::from(row) * 90.0,
                    0.5 + f64::from(column) * 90.0,
                    0.0,
                );
                let actual = atlas.sample(position).unwrap().elevation_msl.get();
                assert!((actual - f64::from(row * 4 + column) * 100.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn longitude_seam_and_multiple_wraps_match() {
        let atlas = varying();
        for latitude in [-90.0, -89.9, -45.0, 0.0, 67.0, 89.9, 90.0] {
            let west = atlas
                .sample(Geodetic::from_degrees(latitude, -180.0, 0.0))
                .unwrap();
            for longitude in [180.0, 540.0, -540.0] {
                let east = atlas
                    .sample(Geodetic::from_degrees(latitude, longitude, 0.0))
                    .unwrap();
                assert!((west.surface_height.get() - east.surface_height.get()).abs() < 1e-9);
                assert!((west.land_fraction - east.land_fraction).abs() < 1e-12);
            }
            let a = atlas
                .sample(Geodetic::from_degrees(latitude, -180.0 + 1e-8, 0.0))
                .unwrap();
            let b = atlas
                .sample(Geodetic::from_degrees(latitude, 180.0 - 1e-8, 0.0))
                .unwrap();
            assert!((a.surface_height.get() - b.surface_height.get()).abs() < 1e-4);
        }
    }

    #[test]
    fn poles_are_single_points_and_caps_converge_continuously() {
        let atlas = varying();
        for pole in [-90.0, 90.0] {
            let reference = atlas
                .sample(Geodetic::from_degrees(pole, 0.0, 0.0))
                .unwrap();
            for lon in [-180.0, -137.2, 40.0, 90.0, 180.0] {
                let at_pole = atlas
                    .sample(Geodetic::from_degrees(pole, lon, 0.0))
                    .unwrap();
                assert!(
                    (at_pole.surface_height.get() - reference.surface_height.get()).abs() < 1e-10
                );
                let nearby = atlas
                    .sample(Geodetic::from_degrees(pole * (1.0 - 1e-10), lon, 0.0))
                    .unwrap();
                assert!(
                    (nearby.surface_height.get() - reference.surface_height.get()).abs() < 1e-5
                );
            }
        }
    }

    #[test]
    fn nonfinite_or_out_of_domain_coordinates_are_rejected() {
        let atlas = varying();
        for position in [
            Geodetic::new(Radians(f64::NAN), Radians(0.0), Meters::ZERO),
            Geodetic::new(Radians(0.0), Radians(f64::INFINITY), Meters::ZERO),
            Geodetic::from_degrees(90.001, 0.0, 0.0),
            Geodetic::from_degrees(-90.001, 0.0, 0.0),
        ] {
            assert!(atlas.sample(position).is_none());
        }
        assert!(
            atlas
                .sample(Geodetic::new(Radians(0.0), Radians(f64::MAX), Meters::ZERO))
                .is_some()
        );
    }

    #[test]
    fn generated_tile_edges_match_including_antimeridian_and_poles() {
        let atlas = varying();
        for level in [0, 3, 8, 13] {
            let west_id = TileId::new(level, 0, 0);
            let east_id = TileId::new(level, TileId::columns(level) - 1, 0);
            let west = atlas.tile(west_id).unwrap();
            let east = atlas.tile(east_id).unwrap();
            for row in 0..GLOBAL_TILE_GRID_SIZE {
                assert_eq!(
                    west.grid().sample_at(0, row),
                    east.grid().sample_at(GLOBAL_TILE_GRID_SIZE - 1, row)
                );
            }
            let neighbour = atlas.tile(TileId::new(level, 1, 0)).unwrap();
            for row in 0..GLOBAL_TILE_GRID_SIZE {
                assert_eq!(
                    west.grid().sample_at(GLOBAL_TILE_GRID_SIZE - 1, row),
                    neighbour.grid().sample_at(0, row)
                );
            }
            let pole = west.grid().sample_at(0, 0);
            for column in 0..GLOBAL_TILE_GRID_SIZE {
                assert_eq!(west.grid().sample_at(column, 0), pole);
            }
            assert!(west.geometric_error().get().is_finite());
            assert_eq!(west.grid().width(), GLOBAL_TILE_GRID_SIZE);
        }
        assert!(atlas.tile(TileId::new(14, 0, 0)).is_none());
        assert!(
            atlas
                .tile(TileId {
                    level: 255,
                    x: u32::MAX,
                    y: u32::MAX
                })
                .is_none()
        );
    }

    #[test]
    fn explicit_global_only_sampling_is_bit_exact_and_preserves_source_capabilities() {
        let atlas = GlobalTerrain::bundled().unwrap();
        let mut legacy = Terrain::new(
            GlobalTileSource::new(MemoryTileSource::new(), atlas.clone()),
            100_000,
            0..=13,
        );
        let explicit: Box<dyn TileSource> =
            Box::new(GlobalTileSource::new(crate::EmptyTileSource, atlas));
        assert!(!explicit.primary_reads_possible());
        assert!(explicit.has_fallback());
        assert!(legacy.source().primary_reads_possible());
        assert!(legacy.source().has_fallback());
        let mut explicit = Terrain::new(&explicit, 100_000, 0..=13);
        assert!(!explicit.source().primary_reads_possible());
        // Include both poles, both sides of the dateline, below-sea-level land,
        // high terrain, and repeated cold/warm-cache geographic samples.
        for _ in 0..2 {
            for lat in [-90.0, -89.999, -45.0, 0.0, 27.99, 31.5, 35.55, 89.999, 90.0] {
                for lon in [
                    -180.0, -179.999, -70.0, 0.0, 35.5, 86.93, 139.78, 179.999, 180.0,
                ] {
                    let position = Geodetic::from_degrees(lat, lon, 1234.0);
                    let expected = legacy.elevation_at(position).unwrap();
                    let actual = explicit.elevation_at(position).unwrap();
                    assert_eq!(
                        actual.get().to_bits(),
                        expected.get().to_bits(),
                        "{lat}, {lon}"
                    );
                    assert_eq!(
                        actual,
                        explicit.source().fallback_elevation_at(position).unwrap()
                    );
                }
            }
        }
        assert!(explicit.cache().is_empty());
        assert!(explicit.load_failures().is_empty());
    }

    #[test]
    fn real_primary_ancestor_wins_over_generated_fine_tiles() {
        let atlas = varying();
        let location = Geodetic::from_degrees(35.0, 139.0, 0.0);
        let ancestor = TileId::containing(3, location);
        let mut primary = MemoryTileSource::new();
        primary.insert(
            ancestor,
            DemTile::new(ancestor.bounds(), HeightGrid::flat(3, 3, Meters(777.0))),
        );
        let source = GlobalTileSource::new(primary, atlas);
        assert!(
            source
                .load(TileId::containing(13, location))
                .unwrap()
                .is_none()
        );
        assert!(
            source
                .load_fallback(TileId::containing(13, location))
                .unwrap()
                .is_some()
        );
        let mut terrain = Terrain::new(source, 100_000, 0..=13);
        assert_eq!(terrain.elevation_at(location), Some(Meters(777.0)));
        assert_eq!(terrain.elevation_at(location), Some(Meters(777.0)));
        let distant = Geodetic::from_degrees(-45.0, -70.0, 0.0);
        assert_eq!(
            terrain.elevation_at(distant),
            terrain.source().fallback_elevation_at(distant)
        );
        assert!(terrain.cache().used_bytes() <= terrain.cache().capacity_bytes());
    }

    #[test]
    fn source_load_never_hides_nested_disk_attempts_and_forwarders_preserve_fallback() {
        #[derive(Debug)]
        struct Counted(Cell<usize>);
        impl TileSource for Counted {
            fn load(&self, _id: TileId) -> Result<Option<DemTile>, TerrainError> {
                self.0.set(self.0.get() + 1);
                Ok(None)
            }
        }
        let source = GlobalTileSource::new(Counted(Cell::new(0)), varying());
        source.load(TileId::new(13, 1, 1)).unwrap();
        assert_eq!(source.primary().0.get(), 1);
        source.load_fallback(TileId::new(13, 1, 1)).unwrap();
        assert_eq!(source.primary().0.get(), 1);
        let boxed: Box<dyn TileSource> = Box::new(source);
        assert!(boxed.primary_reads_possible());
        assert!(boxed.has_fallback());
        fn reference_has_fallback<S: TileSource>(source: S) -> bool {
            source.has_fallback()
        }
        assert!(reference_has_fallback(&boxed));
        assert!(
            boxed
                .fallback_elevation_at(Geodetic::from_degrees(0.0, 0.0, 0.0))
                .is_some()
        );
        assert!(
            boxed
                .load_fallback(TileId::new(13, 1, 1))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn malformed_headers_checksums_samples_and_lengths_are_rejected() {
        let valid = encode_fixture(2, (-135.0, 45.0), &[(10, 100, true); 8]);
        for length in [0, 1, 63, valid.len() - 1] {
            assert!(GlobalTerrain::from_bytes(valid[..length].to_vec()).is_err());
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(GlobalTerrain::from_bytes(trailing).is_err());
        for index in [0, 4, 6, 8, 12, 16, 18, 20, 22, 24, 32, 40, 48, 64] {
            let mut corrupt = valid.clone();
            corrupt[index] ^= 1;
            assert!(
                GlobalTerrain::from_bytes(corrupt).is_err(),
                "accepted damaged field at {index}"
            );
        }
        for (offset, value) in [(32, f64::NAN), (32, 180.0), (40, 90.0), (40, 0.0)] {
            let mut malformed = valid.clone();
            malformed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            reseal(&mut malformed);
            assert!(GlobalTerrain::from_bytes(malformed).is_err());
        }
        for (offset, value) in [(64, i16::MIN), (66, i16::MAX)] {
            let mut malformed = valid.clone();
            malformed[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            reseal(&mut malformed);
            assert!(GlobalTerrain::from_bytes(malformed).is_err());
        }
        let mut padding = encode_fixture(3, (-150.0, 60.0), &[(0, 0, false); 18]);
        *padding.last_mut().unwrap() |= 0b1000_0000;
        reseal(&mut padding);
        assert!(GlobalTerrain::from_bytes(padding).is_err());
    }

    #[test]
    fn canonical_source_breakpoints_and_pole_caps_match_fine_dem_heights() {
        let atlas = GlobalTerrain::bundled().unwrap();
        assert_eq!(atlas.dimensions(), (2_048, 1_024));
        let (longitude, north, step) = atlas.grid_geometry();
        let mut probes = vec![
            // Worst original unaligned source-node probe; formerly >8 m at L13.
            Geodetic::from_degrees(-8.825, 147.508_333_333_333_33, 0.0),
            Geodetic::from_degrees(90.0, -180.0, 0.0),
            Geodetic::from_degrees(-90.0, 180.0, 0.0),
            Geodetic::from_degrees(89.99, 40.0, 0.0),
            Geodetic::from_degrees(-89.99, -123.0, 0.0),
            Geodetic::from_degrees(15.0, 180.0, 0.0),
            Geodetic::from_degrees(15.0, -180.0, 0.0),
        ];
        for index in 0..64_u32 {
            let row = (index * 131 + 17) % 1_024;
            let column = (index * 547 + 37) % 2_048;
            // Source nodes are exactly the breakpoints that an unaligned DEM
            // grid used to smooth over. Also probe the polar source rows.
            for row in [row, 0, 1_023] {
                probes.push(Geodetic::new(
                    Radians(north.get() - f64::from(row) * step.get()),
                    Radians(longitude.get() + f64::from(column) * step.get()),
                    Meters::ZERO,
                ));
            }
        }
        for level in [6, 8, 13] {
            for position in &probes {
                let direct = atlas.sample(*position).unwrap().surface_height.get();
                let tile = atlas.tile(TileId::containing(level, *position)).unwrap();
                let resampled = tile.elevation_at(*position).get();
                assert!(
                    (direct - resampled).abs() < 0.002,
                    "canonical interpolation changed by {} m at level {level}, {position:?}",
                    (direct - resampled).abs()
                );
            }
        }
    }

    #[test]
    fn bundled_data_is_real_complete_and_identifiable() {
        let atlas = GlobalTerrain::bundled().expect("the distributed world asset must validate");
        assert_eq!(atlas.fingerprint(), GLOBAL_TERRAIN_FINGERPRINT);
        assert_eq!(atlas.dimensions(), (2_048, 1_024));
        assert!(atlas.memory_footprint() < 10 * 1024 * 1024);
        // Broad, externally known regions rather than exact grid-derived values:
        // Himalaya and Andes are elevated land, central Pacific is ocean.
        for (lat, lon, minimum) in [
            (28.0, 86.8, 3_500.0),
            (-22.0, -67.0, 2_500.0),
            (72.0, -42.0, 1_500.0),
        ] {
            let sample = atlas.sample(Geodetic::from_degrees(lat, lon, 0.0)).unwrap();
            assert!(
                sample.is_land,
                "known continental/ice-covered land misclassified at {lat},{lon}"
            );
            assert!(
                sample.elevation_msl.get() > minimum,
                "major mountain/ice region absent at {lat},{lon}: {sample:?}"
            );
        }
        // Named depression represented at the pooled canonical node. This is
        // not a surveyed prediction at the exact Badwater Basin coordinate.
        let depression = atlas
            .sample(Geodetic::from_degrees(
                36.123_046_875,
                -116.806_640_625,
                0.0,
            ))
            .unwrap();
        assert!(
            depression.is_land && depression.elevation_msl.get() < 0.0,
            "Death Valley below-sea-level land must not be replaced by ocean"
        );
        let dead_sea = atlas
            .sample(Geodetic::from_degrees(31.728_515_625, 35.595_703_125, 0.0))
            .unwrap();
        assert!(dead_sea.is_inland_water && !dead_sea.is_land);
        assert!((-450.0..-400.0).contains(&dead_sea.elevation_msl.get()));
        for (latitude, longitude, low, high) in [
            (53.341_666_666_7, 108.175, 440.0, 470.0),
            (42.0, 51.0, -40.0, -20.0),
            (47.7, -87.5, 175.0, 190.0),
        ] {
            let lake = atlas
                .sample(Geodetic::from_degrees(latitude, longitude, 0.0))
                .unwrap();
            assert!(lake.is_inland_water && !lake.is_land);
            assert!((low..high).contains(&lake.elevation_msl.get()));
        }
        let former_bathymetry = atlas
            .sample(Geodetic::from_degrees(-15.491_666_666_7, 168.175, 0.0))
            .unwrap();
        assert!(
            former_bathymetry.elevation_msl.get() > -100.0,
            "the Vanuatu coastal seabed must not be exposed as land"
        );
        let pacific = atlas
            .sample(Geodetic::from_degrees(0.0, -140.0, 0.0))
            .unwrap();
        assert!(!pacific.is_land);
        assert_eq!(pacific.elevation_msl, Meters::ZERO);
        assert!((pacific.surface_height.get() - pacific.geoid_undulation.get()).abs() < 1e-12);
        for lat in [-90.0, -89.999, -30.0, 0.0, 35.0, 89.999, 90.0] {
            for lon in [-180.0, -120.0, 0.0, 139.0, 180.0] {
                assert!(
                    atlas
                        .sample(Geodetic::from_degrees(lat, lon, 0.0))
                        .unwrap()
                        .surface_height
                        .get()
                        .is_finite()
                );
            }
        }
    }
}
