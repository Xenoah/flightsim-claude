//! Monthly, regional climate from NOAA NCEP/NCAR Reanalysis 1 (1991–2020).
//!
//! This is **climatology, not live weather or a forecast**. The small bundled
//! atlas retains the source's 94 Gaussian latitude rows and 192 longitudes.
//! Temperature, water-equivalent precipitation and total cloud cover are real
//! reanalysis long-term means. Spatial/monthly interpolation, the 6.5 K/km
//! altitude adjustment, broad climate labels and snow indicator are models.
//! They are not station observations, a sounding, a Köppen map or snow depth.
//!
//! Runtime reads only a bounded, pre-baked binary, never NetCDF or HDF5.
//! [`GlobalClimate::bundled`] validates the atlas before sharing immutable data
//! with `Arc`. Sampling is deterministic and has no clock, RNG or network.
//! See `docs/data/global-climate.md` and `scripts/bake_climate.py` for sources,
//! data processing, vertical datum approximations and reproducibility.

use std::sync::{Arc, OnceLock};

use flightsim_core::{Geodetic, Kelvin, Meters, MetersPerSecond, Seconds};

include!("climate_metadata.rs");

/// Source of the monthly fields, as opposed to their derived visual cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClimateSource {
    /// NOAA-provided NCEP/NCAR Reanalysis 1 monthly 1991–2020 long-term means.
    NcepNcarReanalysis1991To2020,
}

impl ClimateSource {
    /// Explicit user-facing provenance. Never label these means live weather.
    #[must_use]
    pub const fn label(self) -> &'static str {
        "NCEP/NCAR 1991-2020 reanalysis climatology"
    }
}

/// Broad, derived regional labels; these are deliberately not Köppen codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClimateZone {
    Tropical,
    Dry,
    Temperate,
    Continental,
    Polar,
    Highland,
}

impl ClimateZone {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tropical => "Tropical",
            Self::Dry => "Dry / desert",
            Self::Temperate => "Temperate",
            Self::Continental => "Continental / boreal",
            Self::Polar => "Polar",
            Self::Highland => "Highland",
        }
    }
}

// Non-leap reference calendar, in days since January 1 midnight. Monthly
// means are placed at calendar-month centres; interpolation closes the year.
const MONTH_LENGTHS: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_STARTS: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
const MONTH_CENTRES: [f64; 12] = [
    15.5, 45.0, 74.5, 105.0, 135.5, 166.0, 196.5, 227.5, 258.0, 288.5, 319.0, 349.5,
];

/// A validated position in a climatological year, independent of wall time.
///
/// The representation is a finite, dimensionless annual phase in `[0, 1)`.
/// Gregorian dates map each actual month onto the corresponding reference
/// month, so February 29 is valid in leap years and joins March continuously.
/// The year does not select a historic weather record: all years use the same
/// 1991–2020 monthly means. A simulation keeps this date fixed for one flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateDate {
    annual_phase: f64,
}

impl ClimateDate {
    /// The exact centre of a monthly mean, with `month` in `1..=12`.
    #[must_use]
    pub fn from_month(month: u8) -> Option<Self> {
        let index = usize::from(month.checked_sub(1)?);
        let day = *MONTH_CENTRES.get(index)?;
        Some(Self {
            annual_phase: day / 365.0,
        })
    }

    /// A date at midnight in the non-leap reference calendar.
    /// February 29 is rejected; use [`Self::from_gregorian`] for a leap year.
    #[must_use]
    pub fn from_month_day(month: u8, day: u8) -> Option<Self> {
        Self::from_gregorian(2001, month, day, Seconds::ZERO)
    }

    /// A Gregorian date and seconds since midnight (`0 <= t < 86400`).
    ///
    /// Year numbering is astronomical (year zero is allowed). Invalid month,
    /// day, nonfinite time or an out-of-range time is rejected, not normalized.
    #[must_use]
    pub fn from_gregorian(year: i32, month: u8, day: u8, time_of_day: Seconds) -> Option<Self> {
        let index = usize::from(month.checked_sub(1)?);
        let reference_length = *MONTH_LENGTHS.get(index)?;
        let leap =
            year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
        let actual_length = reference_length + u16::from(index == 1 && leap);
        if day == 0
            || u16::from(day) > actual_length
            || !time_of_day.is_finite()
            || !(0.0..86_400.0).contains(&time_of_day.get())
        {
            return None;
        }
        let fraction =
            (f64::from(day - 1) + time_of_day.get() / 86_400.0) / f64::from(actual_length);
        let phase =
            (f64::from(MONTH_STARTS[index]) + fraction * f64::from(reference_length)) / 365.0;
        // A valid instant immediately before New Year can round up to 1.
        // Keep it in the last representable instant, without wrapping its year.
        Self::from_annual_phase(phase.min(f64::from_bits(1.0_f64.to_bits() - 1)))
    }

    /// Restore the exact dimensionless phase stored in a replay.
    #[must_use]
    pub fn from_annual_phase(annual_phase: f64) -> Option<Self> {
        if annual_phase.is_finite() && (0.0..1.0).contains(&annual_phase) {
            Some(Self { annual_phase })
        } else {
            None
        }
    }

    #[must_use]
    pub const fn annual_phase(self) -> f64 {
        self.annual_phase
    }

    /// Calendar month containing this phase, in `1..=12`.
    #[must_use]
    pub fn month(self) -> u8 {
        let count =
            MONTH_STARTS.partition_point(|&start| f64::from(start) / 365.0 <= self.annual_phase);
        u8::try_from(count.clamp(1, 12)).unwrap_or(1)
    }

    fn month_weights(self) -> (usize, usize, f64) {
        let day = self.annual_phase * 365.0;
        let right = MONTH_CENTRES.partition_point(|&centre| centre <= day);
        if right == 0 {
            let left_day = MONTH_CENTRES[11] - 365.0;
            (11, 0, (day - left_day) / (MONTH_CENTRES[0] - left_day))
        } else if right == 12 {
            let right_day = MONTH_CENTRES[0] + 365.0;
            (
                11,
                0,
                (day - MONTH_CENTRES[11]) / (right_day - MONTH_CENTRES[11]),
            )
        } else {
            (
                right - 1,
                right,
                (day - MONTH_CENTRES[right - 1])
                    / (MONTH_CENTRES[right] - MONTH_CENTRES[right - 1]),
            )
        }
    }
}

/// Regional monthly climate and explicitly derived altitude/snow cues.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateSample {
    /// Temperature at queried ellipsoidal height, using the modeled lapse.
    /// The lapse is capped at 11 km MSL; this is not an upper-air sounding.
    pub temperature: Kelvin,
    /// The same gridded temperature reduced to MSL with the modeled lapse.
    pub sea_level_temperature: Kelvin,
    /// Equivalent ISA offset referred to ellipsoid altitude zero, in kelvin.
    /// Sim applies this **once** to its ISA atmosphere, retaining ISA pressure.
    pub isa_temperature_offset: Kelvin,
    /// Coarse NCEP surface geopotential height, treated as approximate MSL m.
    pub model_surface_elevation: Meters,
    /// EGM2008 undulation used for `MSL height = ellipsoidal height - N`.
    pub geoid_undulation: Meters,
    /// Monthly mean water-equivalent precipitation, not current rainfall.
    pub precipitation_rate: MetersPerSecond,
    /// Interpolated monthly total cloud cover from reanalysis, in `[0, 1]`.
    pub cloud_cover: f64,
    /// Derived cold-phase / snow visual indicator, in `[0, 1]`.
    /// This does not assert measured snow cover, snow depth or active snowfall.
    pub snow_fraction: f64,
    /// Broad class of the source-grid near-surface annual climate.
    /// Aircraft altitude does not turn an oceanic region into a mountain.
    pub zone: ClimateZone,
    pub source: ClimateSource,
}

impl ClimateSample {
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.temperature.is_finite()
            && self.sea_level_temperature.is_finite()
            && self.isa_temperature_offset.is_finite()
            && self.model_surface_elevation.is_finite()
            && self.geoid_undulation.is_finite()
            && self.precipitation_rate.is_finite()
            && self.cloud_cover.is_finite()
            && self.snow_fraction.is_finite()
    }
}

/// Invalid pre-baked climate data. No repair of corrupt source bytes is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClimateDataError {
    reason: &'static str,
}

impl std::fmt::Display for ClimateDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid global climate atlas: {}", self.reason)
    }
}

impl std::error::Error for ClimateDataError {}

const fn invalid(reason: &'static str) -> ClimateDataError {
    ClimateDataError { reason }
}

const ROWS: usize = 96;
const COLUMNS: usize = 192;
const MONTHS: usize = 12;
const CELLS: usize = ROWS * COLUMNS;
const HEADER_BYTES: usize = 40;
const PAYLOAD_BYTES: usize = ROWS * 8 + CELLS * 4 + MONTHS * CELLS * 5;
const LAPSE_K_PER_M: f64 = 0.0065;
const BUNDLED: &[u8] = include_bytes!("../data/ncep-ncar-1991-2020.fsclim");
static BUNDLED_CLIMATE: OnceLock<Result<GlobalClimate, ClimateDataError>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct StaticCell {
    model_height: f64,
    geoid: f64,
}

#[derive(Debug, Clone, Copy)]
struct MonthlyCell {
    temperature: f64,
    precipitation: f64,
    cloud: f64,
}

#[derive(Debug)]
struct ClimateData {
    latitudes: [f64; ROWS],
    static_cells: Vec<StaticCell>,
    monthly_cells: Vec<MonthlyCell>,
    fingerprint: u64,
}

/// Immutable, validated global monthly atlas. Cloning shares its data.
#[derive(Debug, Clone)]
pub struct GlobalClimate {
    data: Arc<ClimateData>,
}

impl GlobalClimate {
    /// Load and validate the embedded atlas. There is no download at startup.
    ///
    /// # Errors
    /// Returns an error if its version, layout, checksum, physical values or
    /// polar closure are inconsistent with this build's bundled data identity.
    pub fn bundled() -> Result<Self, ClimateDataError> {
        BUNDLED_CLIMATE
            .get_or_init(|| Self::from_bytes(BUNDLED, GLOBAL_CLIMATE_FINGERPRINT))
            .clone()
    }

    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        self.data.fingerprint
    }

    /// Bilinear space / linear calendar-month interpolation of climate means.
    ///
    /// Longitude wraps at the dateline. Added polar zonal-mean rows make the
    /// same pole independent of longitude. Invalid numerical inputs are
    /// sanitized (nonfinite values to zero; latitude/altitude clamped), so this secondary
    /// field cannot spread NaN into the physical state. Sim must still reject
    /// a diverged aircraft. Query height is WGS84 ellipsoidal metres.
    #[must_use]
    pub fn sample(&self, position: Geodetic, date: ClimateDate) -> ClimateSample {
        let latitude = finite_or_zero(position.latitude.get())
            .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        let longitude = finite_or_zero(position.longitude.get()).rem_euclid(std::f64::consts::TAU);
        let altitude = finite_or_zero(position.altitude.get()).clamp(-1_200.0, 86_000.0);
        let weights = self.spatial_weights(latitude, longitude);
        let model_height = weights.interpolate(|i| self.data.static_cells[i].model_height);
        let geoid = weights.interpolate(|i| self.data.static_cells[i].geoid);
        let (month_a, month_b, fraction) = date.month_weights();
        let a = self.month_at(weights, month_a);
        let b = self.month_at(weights, month_b);
        let temperature = mix(a.temperature, b.temperature, fraction);
        let precipitation = mix(a.precipitation, b.precipitation, fraction).max(0.0);
        let cloud_cover = mix(a.cloud, b.cloud, fraction).clamp(0.0, 1.0);

        // NCEP air is 2 metres above its own model terrain. Normalize before
        // applying destination altitude so a real mountain isn't lapsed twice.
        // Geopotential surface height is used as approximate geometric MSL
        // height at this coarse scale. We do not relabel it ellipsoidal height.
        let sea_temperature = temperature + LAPSE_K_PER_M * (model_height + 2.0);
        let msl_altitude = (altitude - geoid).clamp(-1_200.0, 11_000.0);
        let query_temperature =
            (sea_temperature - LAPSE_K_PER_M * msl_altitude).clamp(150.0, 350.0);
        // The FDM's ISA profile uses ellipsoidal altitude. Its offset therefore
        // includes the N term, rather than silently treating h as MSL.
        let isa_offset = sea_temperature + LAPSE_K_PER_M * geoid - 288.15;
        let snow_fraction = (1.0 - smoothstep(270.15, 276.15, query_temperature)).clamp(0.0, 1.0);

        ClimateSample {
            temperature: Kelvin(query_temperature),
            sea_level_temperature: Kelvin(sea_temperature),
            isa_temperature_offset: Kelvin(isa_offset),
            model_surface_elevation: Meters(model_height),
            geoid_undulation: Meters(geoid),
            precipitation_rate: MetersPerSecond(precipitation),
            cloud_cover,
            snow_fraction,
            zone: self.zone_at(weights, model_height),
            source: ClimateSource::NcepNcarReanalysis1991To2020,
        }
    }

    fn from_bytes(bytes: &[u8], expected_fingerprint: u64) -> Result<Self, ClimateDataError> {
        if bytes.len() != HEADER_BYTES + PAYLOAD_BYTES {
            return Err(invalid("wrong byte count"));
        }
        if &bytes[..8] != b"FSCLIM01" {
            return Err(invalid("wrong magic"));
        }
        if read_u16(bytes, 8) != 1
            || usize::from(read_u16(bytes, 10)) != ROWS
            || usize::from(read_u16(bytes, 12)) != COLUMNS
            || usize::from(read_u16(bytes, 14)) != MONTHS
            || read_u16(bytes, 16) != 5
            || read_u16(bytes, 18) != 4
            || bytes[20..24] != [0; 4]
            || read_u64(bytes, 24) != u64::try_from(PAYLOAD_BYTES).unwrap_or(u64::MAX)
        {
            return Err(invalid("unsupported format, dimensions or flags"));
        }
        let payload = &bytes[HEADER_BYTES..];
        let fingerprint = read_u64(bytes, 32);
        if fingerprint != expected_fingerprint || checksum(payload) != fingerprint {
            return Err(invalid("data identity or checksum mismatch"));
        }

        let mut latitudes = [0.0; ROWS];
        for (row, latitude) in latitudes.iter_mut().enumerate() {
            *latitude = f64::from_bits(read_u64(payload, row * 8));
            if !latitude.is_finite()
                || !(-std::f64::consts::FRAC_PI_2..=std::f64::consts::FRAC_PI_2).contains(latitude)
            {
                return Err(invalid("invalid latitude"));
            }
        }
        if (latitudes[0] - std::f64::consts::FRAC_PI_2).abs() > 1e-12
            || (latitudes[ROWS - 1] + std::f64::consts::FRAC_PI_2).abs() > 1e-12
            || !latitudes.windows(2).all(|pair| pair[0] > pair[1])
        {
            return Err(invalid(
                "latitude grid is not ordered or does not close at poles",
            ));
        }

        let mut static_cells = Vec::with_capacity(CELLS);
        let static_start = ROWS * 8;
        for cell in 0..CELLS {
            let offset = static_start + cell * 4;
            let model_height = f64::from(read_i16(payload, offset));
            let geoid = f64::from(read_i16(payload, offset + 2)) * 0.1;
            if !(-1_200.0..=6_300.0).contains(&model_height) || !(-200.0..=200.0).contains(&geoid) {
                return Err(invalid("implausible terrain or geoid value"));
            }
            static_cells.push(StaticCell {
                model_height,
                geoid,
            });
        }
        let mut monthly_cells = Vec::with_capacity(MONTHS * CELLS);
        let monthly_start = static_start + CELLS * 4;
        for cell in 0..MONTHS * CELLS {
            let offset = monthly_start + cell * 5;
            let temperature = f64::from(read_u16(payload, offset)) * 0.01;
            let precipitation = f64::from(read_u16(payload, offset + 2)) * 1e-10;
            let cloud = f64::from(payload[offset + 4]) / 255.0;
            if !(150.0..=350.0).contains(&temperature) || precipitation > 1e-6 {
                return Err(invalid("implausible temperature or precipitation value"));
            }
            monthly_cells.push(MonthlyCell {
                temperature,
                precipitation,
                cloud,
            });
        }
        // Pole equality is a format invariant, not just a sampler assumption.
        // Compare original bytes exactly, including the height/geoid fields.
        for row in [0, ROWS - 1] {
            let first = static_start + row * COLUMNS * 4;
            for col in 1..COLUMNS {
                let other = first + col * 4;
                if payload[first..first + 4] != payload[other..other + 4] {
                    return Err(invalid("static pole row varies with longitude"));
                }
            }
            for month in 0..MONTHS {
                let first = monthly_start + (month * CELLS + row * COLUMNS) * 5;
                for col in 1..COLUMNS {
                    let other = first + col * 5;
                    if payload[first..first + 5] != payload[other..other + 5] {
                        return Err(invalid("monthly pole row varies with longitude"));
                    }
                }
            }
        }
        Ok(Self {
            data: Arc::new(ClimateData {
                latitudes,
                static_cells,
                monthly_cells,
                fingerprint,
            }),
        })
    }

    fn spatial_weights(&self, latitude: f64, longitude: f64) -> SpatialWeights {
        let south = self
            .data
            .latitudes
            .partition_point(|&lat| lat > latitude)
            .clamp(1, ROWS - 1);
        let north = south - 1;
        let latitude_fraction = ((self.data.latitudes[north] - latitude)
            / (self.data.latitudes[north] - self.data.latitudes[south]))
            .clamp(0.0, 1.0);
        let column = longitude / std::f64::consts::TAU * 192.0;
        // Longitude is finite, normalized and bounded. Rounding just below
        // TAU can still produce column 192; wrap it rather than indexing 192.
        #[allow(clippy::cast_possible_truncation)]
        let west = (column.floor() as usize).min(COLUMNS) % COLUMNS;
        let east = (west + 1) % COLUMNS;
        SpatialWeights {
            corners: [
                north * COLUMNS + west,
                north * COLUMNS + east,
                south * COLUMNS + west,
                south * COLUMNS + east,
            ],
            longitude_fraction: column - column.floor(),
            latitude_fraction,
        }
    }

    fn month_at(&self, weights: SpatialWeights, month: usize) -> MonthlyCell {
        let base = month * CELLS;
        MonthlyCell {
            temperature: weights.interpolate(|i| self.data.monthly_cells[base + i].temperature),
            precipitation: weights.interpolate(|i| self.data.monthly_cells[base + i].precipitation),
            cloud: weights.interpolate(|i| self.data.monthly_cells[base + i].cloud),
        }
    }

    fn zone_at(&self, weights: SpatialWeights, model_height: f64) -> ClimateZone {
        let mut warmest = f64::NEG_INFINITY;
        let mut coldest = f64::INFINITY;
        let mut annual_water = 0.0;
        for (month, days) in MONTH_LENGTHS.into_iter().enumerate() {
            let sample = self.month_at(weights, month);
            warmest = warmest.max(sample.temperature);
            coldest = coldest.min(sample.temperature);
            annual_water += sample.precipitation * f64::from(days) * 86_400.0;
        }
        if warmest < 283.15 {
            ClimateZone::Polar
        } else if model_height > 1_800.0 {
            ClimateZone::Highland
        } else if annual_water < 0.3 {
            ClimateZone::Dry
        } else if coldest >= 291.15 {
            ClimateZone::Tropical
        } else if coldest < 270.15 {
            ClimateZone::Continental
        } else {
            ClimateZone::Temperate
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SpatialWeights {
    corners: [usize; 4],
    longitude_fraction: f64,
    latitude_fraction: f64,
}

impl SpatialWeights {
    fn interpolate(self, field: impl Fn(usize) -> f64) -> f64 {
        let north = mix(
            field(self.corners[0]),
            field(self.corners[1]),
            self.longitude_fraction,
        );
        let south = mix(
            field(self.corners[2]),
            field(self.corners[3]),
            self.longitude_fraction,
        );
        mix(north, south, self.latitude_fraction)
    }
}

fn mix(a: f64, b: f64, fraction: f64) -> f64 {
    if fraction <= 0.0 {
        a
    } else if fraction >= 1.0 {
        b
    } else {
        a + (b - a) * fraction
    }
}

fn smoothstep(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_i16(bytes: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(lat: f64, lon: f64, alt: f64, month: u8) -> ClimateSample {
        GlobalClimate::bundled().unwrap().sample(
            Geodetic::from_degrees(lat, lon, alt),
            ClimateDate::from_month(month).unwrap(),
        )
    }

    #[test]
    fn calendar_validates_gregorian_boundaries_and_preserves_phase() {
        for month in 1..=12 {
            let date = ClimateDate::from_month(month).unwrap();
            assert_eq!(date.month(), month);
            assert_eq!(
                ClimateDate::from_annual_phase(date.annual_phase()),
                Some(date)
            );
            let first = ClimateDate::from_gregorian(2026, month, 1, Seconds::ZERO).unwrap();
            assert_eq!(first.month(), month);
            if month > 1 {
                let immediately_before = f64::from_bits(first.annual_phase().to_bits() - 1);
                assert_eq!(
                    ClimateDate::from_annual_phase(immediately_before)
                        .unwrap()
                        .month(),
                    month - 1
                );
            }
        }
        assert!(ClimateDate::from_month(0).is_none());
        assert!(ClimateDate::from_month(13).is_none());
        assert!(ClimateDate::from_month_day(2, 29).is_none());
        assert!(ClimateDate::from_gregorian(2000, 2, 29, Seconds::ZERO).is_some());
        assert!(ClimateDate::from_gregorian(1900, 2, 29, Seconds::ZERO).is_none());
        assert!(ClimateDate::from_gregorian(2024, 2, 29, Seconds(86_399.9)).is_some());
        for time in [-1.0, 86_400.0, f64::NAN, f64::INFINITY] {
            assert!(ClimateDate::from_gregorian(2026, 6, 21, Seconds(time)).is_none());
        }
        for phase in [-1.0, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(ClimateDate::from_annual_phase(phase).is_none());
        }
        let end = ClimateDate::from_gregorian(2024, 2, 29, Seconds(86_399.999)).unwrap();
        let start = ClimateDate::from_gregorian(2024, 3, 1, Seconds::ZERO).unwrap();
        assert!((end.annual_phase() - start.annual_phase()).abs() < 1e-9);
    }

    #[test]
    fn source_grid_known_noaa_values_survive_quantization() {
        // Independently inspected source NetCDF scalar: January air[0,0,0]
        // at 88.54199981689453 N, 0 E is 242.17156982421875 K. Its model
        // hgt is 31 m; sample at hgt + 2 m + geoid to remove our lapse.
        let climate = GlobalClimate::bundled().unwrap();
        let point = Geodetic::from_degrees(88.541_999_816_894_53, 0.0, 0.0);
        let date = ClimateDate::from_month(1).unwrap();
        let reference = climate.sample(point, date);
        let at_model = climate.sample(
            Geodetic {
                altitude: reference.model_surface_elevation
                    + reference.geoid_undulation
                    + Meters(2.0),
                ..point
            },
            date,
        );
        assert!((at_model.temperature.get() - 242.171_569_824_218_75).abs() <= 0.0051);
        // Source prate[0,0,0] = 3.110224291e-6 kg/m²/s. Water conversion
        // is /1000, not /86400; monthly averages already have per-second units.
        assert!((at_model.precipitation_rate.get() - 3.110_224_291e-9).abs() <= 5.1e-11);
        // Source total cloud cover tcdc[0,0,0] = 34.181339263916016 percent.
        assert!((at_model.cloud_cover - 0.341_813_392_639_160_16).abs() <= 0.5 / 255.0);
    }

    #[test]
    fn longitude_changes_regional_climate_at_similar_latitude() {
        // Sahara versus east-Asian monsoon; maritime western Europe versus
        // continental eastern Siberia. These are broad independent contrasts,
        // not a claim of accurate airport-local temperatures from a 2° grid.
        let sahara = sample(25.0, 10.0, 0.0, 7);
        let india = sample(25.0, 85.0, 0.0, 7);
        assert!(india.precipitation_rate > sahara.precipitation_rate * 5.0);
        assert!(india.cloud_cover > sahara.cloud_cover + 0.15);
        assert_eq!(sahara.zone, ClimateZone::Dry);
        let atlantic = sample(55.0, -10.0, 0.0, 1);
        let siberia = sample(55.0, 110.0, 0.0, 1);
        assert!(atlantic.temperature > siberia.temperature + Kelvin(15.0));
    }

    #[test]
    fn opposite_hemispheres_have_opposite_seasons() {
        let tokyo_jan = sample(35.7, 139.8, 0.0, 1);
        let tokyo_jul = sample(35.7, 139.8, 0.0, 7);
        let sydney_jan = sample(-33.9, 151.2, 0.0, 1);
        let sydney_jul = sample(-33.9, 151.2, 0.0, 7);
        assert!(tokyo_jul.temperature > tokyo_jan.temperature + Kelvin(10.0));
        assert!(sydney_jan.temperature > sydney_jul.temperature + Kelvin(5.0));
    }

    #[test]
    fn tropics_have_small_temperature_seasons_but_monsoon_rain_changes() {
        let singapore_jan = sample(1.3, 103.8, 0.0, 1);
        let singapore_jul = sample(1.3, 103.8, 0.0, 7);
        assert!((singapore_jan.temperature - singapore_jul.temperature).abs() < Kelvin(5.0));
        assert_eq!(singapore_jan.zone, ClimateZone::Tropical);
        let mumbai_jan = sample(19.0, 72.9, 0.0, 1);
        let mumbai_jul = sample(19.0, 72.9, 0.0, 7);
        assert!(mumbai_jul.precipitation_rate > mumbai_jan.precipitation_rate * 3.0);
    }

    #[test]
    fn altitude_lapse_is_applied_once_and_does_not_change_regional_zone() {
        let low = sample(35.0, 139.0, 0.0, 7);
        let high = sample(35.0, 139.0, 2000.0, 7);
        assert!((low.temperature.get() - high.temperature.get() - 13.0).abs() < 1e-10);
        assert_eq!(low.sea_level_temperature, high.sea_level_temperature);
        assert_eq!(low.isa_temperature_offset, high.isa_temperature_offset);
        assert_eq!(low.zone, high.zone);
        let cold = sample(65.0, 110.0, 1000.0, 1);
        let warm = sample(1.3, 103.8, 0.0, 1);
        assert!(cold.snow_fraction > 0.99);
        assert!(warm.snow_fraction < 0.01);
    }

    #[test]
    fn dateline_poles_and_year_join_continuously() {
        let climate = GlobalClimate::bundled().unwrap();
        for month in 1..=12 {
            let date = ClimateDate::from_month(month).unwrap();
            for latitude in [-90.0, -45.0, 0.0, 45.0, 90.0] {
                let west = climate.sample(Geodetic::from_degrees(latitude, -180.0, 0.0), date);
                let east = climate.sample(Geodetic::from_degrees(latitude, 180.0, 0.0), date);
                assert_eq!(west, east);
                let nearby =
                    climate.sample(Geodetic::from_degrees(latitude, 180.0 - 1e-8, 0.0), date);
                assert!((west.temperature - nearby.temperature).abs() < Kelvin(1e-6));
                assert!((west.cloud_cover - nearby.cloud_cover).abs() < 1e-6);
            }
            for latitude in [-90.0, 90.0] {
                let reference = climate.sample(Geodetic::from_degrees(latitude, 0.0, 0.0), date);
                for longitude in [-175.0, -89.0, 35.0, 178.0] {
                    assert_eq!(
                        reference,
                        climate.sample(Geodetic::from_degrees(latitude, longitude, 0.0), date)
                    );
                }
            }
        }
        let point = Geodetic::from_degrees(50.0, 110.0, 500.0);
        let december_end =
            climate.sample(point, ClimateDate::from_annual_phase(1.0 - 1e-9).unwrap());
        let january_start = climate.sample(point, ClimateDate::from_annual_phase(0.0).unwrap());
        assert!((december_end.temperature - january_start.temperature).abs() < Kelvin(1e-5));
        assert!((december_end.cloud_cover - january_start.cloud_cover).abs() < 1e-7);
    }

    #[test]
    fn global_samples_remain_bounded_finite_and_deterministic() {
        let climate = GlobalClimate::bundled().unwrap();
        for latitude in (-90..=90).step_by(9) {
            for longitude in (-180..=180).step_by(13) {
                for month in 1..=12 {
                    let point =
                        Geodetic::from_degrees(f64::from(latitude), f64::from(longitude), 3000.0);
                    let date = ClimateDate::from_month(month).unwrap();
                    let a = climate.sample(point, date);
                    assert_eq!(a, climate.sample(point, date));
                    assert!(a.is_finite());
                    assert!((150.0..=350.0).contains(&a.temperature.get()));
                    assert!((0.0..=1.0).contains(&a.cloud_cover));
                    assert!((0.0..=1.0).contains(&a.snow_fraction));
                    assert!((0.0..=1e-6).contains(&a.precipitation_rate.get()));
                }
            }
        }
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
        ] {
            assert!(
                climate
                    .sample(
                        Geodetic::from_degrees(value, value, value),
                        ClimateDate::from_month(1).unwrap()
                    )
                    .is_finite()
            );
        }
    }

    #[test]
    fn corrupt_or_truncated_atlas_is_rejected_before_sampling() {
        for length in [0, 8, 39, 40, BUNDLED.len() - 1] {
            assert!(
                GlobalClimate::from_bytes(&BUNDLED[..length], GLOBAL_CLIMATE_FINGERPRINT).is_err()
            );
        }
        for offset in [0, 8, 10, 12, 14, 16, 18, 20, 24, 32, 40, BUNDLED.len() - 1] {
            let mut bytes = BUNDLED.to_vec();
            bytes[offset] ^= 0x80;
            assert!(GlobalClimate::from_bytes(&bytes, GLOBAL_CLIMATE_FINGERPRINT).is_err());
        }
        let mut bytes = BUNDLED.to_vec();
        bytes.extend_from_slice(&[0]);
        assert!(GlobalClimate::from_bytes(&bytes, GLOBAL_CLIMATE_FINGERPRINT).is_err());
    }

    #[test]
    fn even_correctly_checksummed_implausible_fields_are_rejected() {
        let invalid_fields = [
            (HEADER_BYTES, f64::NAN.to_le_bytes().to_vec()),
            (HEADER_BYTES + ROWS * 8, i16::MAX.to_le_bytes().to_vec()),
            (HEADER_BYTES + ROWS * 8 + 2, i16::MAX.to_le_bytes().to_vec()),
            (
                HEADER_BYTES + ROWS * 8 + CELLS * 4,
                0_u16.to_le_bytes().to_vec(),
            ),
            (
                HEADER_BYTES + ROWS * 8 + CELLS * 4 + 2,
                u16::MAX.to_le_bytes().to_vec(),
            ),
        ];
        for (at, value) in invalid_fields {
            let mut bytes = BUNDLED.to_vec();
            bytes[at..at + value.len()].copy_from_slice(&value);
            let fingerprint = checksum(&bytes[HEADER_BYTES..]);
            bytes[32..40].copy_from_slice(&fingerprint.to_le_bytes());
            assert!(GlobalClimate::from_bytes(&bytes, fingerprint).is_err());
        }
    }
}
