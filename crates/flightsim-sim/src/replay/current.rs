//! Explicit file-format dispatch and the bounded complete-identity v3 codec.
//!
//! No type here converts a legacy recording into complete aircraft evidence.
//! Parsing does not authorize weather presentation or regional-package playback.

use std::io::{Cursor, Read, Write};

use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::{
    AircraftConfig, ControlInputs, FDM_MODEL_REVISION, RigidBodyState, Turbulence,
};
use flightsim_world::{
    ClimateDate, GLOBAL_CLIMATE_FINGERPRINT, global::GLOBAL_TERRAIN_FINGERPRINT,
};

use super::identity::{
    AIRCRAFT_IDENTITY_ALGORITHM, AIRCRAFT_IDENTITY_SCHEMA, AircraftCompatibility, AircraftIdentity,
    RecordedAircraftIdentity,
};
use super::{
    Frame, KEYFRAME_INTERVAL, Keyframe, LEGACY_FORMAT_VERSION, MAGIC, MAX_FRAMES, MAX_NAME_BYTES,
    Recording, ReplayError, WORLD_FORMAT_VERSION, read_f64, read_records, read_u16, read_u32,
    read_u64, require_valid, validate_environment, validate_records, write_f64, write_records,
};
use crate::Wind;
use crate::weather::{
    CloudMorphology, ModeledCloudLayer, ModeledFogLayer, PrecipitationKind, WEATHER_MODEL_REVISION,
    WEATHER_PARAMETER_SCHEMA, WaterEquivalentRate, WeatherError, WeatherParameters, WeatherPreset,
    WeatherScenario, WeatherSelection, WeatherSource,
};

/// Complete aircraft identity and an optional typed weather block.
pub const CURRENT_FORMAT_VERSION: u16 = 3;
/// Conservative envelope bound; schema 1 actually uses at most 512 bytes.
pub const MAX_CONDITIONS_BYTES: u32 = 4096;
/// Conservative weather bound, not permission to accept unknown payloads.
/// Schema 1 accepts only 62, 86, 96 or 120 bytes when present.
pub const MAX_WEATHER_BYTES: u32 = 512;
const FIXED_CONDITIONS_BYTES: u32 = 136;

/// Environmental values shared by legacy and current files, without aircraft
/// evidence. Defaults preserve the old wind/turbulence, world and ISA behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentConditions {
    pub start: Geodetic,
    pub heading: Radians,
    pub wind: Wind,
    pub turbulence: Turbulence,
    /// Julian date, or zero when unspecified. See [`super::MAX_VISUAL_EPOCH`].
    pub start_epoch: f64,
    /// Visual clock acceleration, finite and nonnegative.
    pub time_rate: f64,
    pub world_terrain: bool,
    pub terrain_fingerprint: u64,
    pub climate_date: Option<ClimateDate>,
    pub climate_fingerprint: u64,
}

impl Default for EnvironmentConditions {
    fn default() -> Self {
        Self {
            start: Geodetic::from_degrees(0.0, 0.0, 0.0),
            heading: Radians(0.0),
            wind: Wind::CALM,
            turbulence: Turbulence::CALM,
            start_epoch: 0.0,
            time_rate: 1.0,
            world_terrain: false,
            terrain_fingerprint: 0,
            climate_date: None,
            climate_fingerprint: 0,
        }
    }
}

impl From<&super::Conditions> for EnvironmentConditions {
    /// Copy only the recorded environmental values; this does not create or
    /// upgrade aircraft identity and is not a legacy-recording migration.
    fn from(value: &super::Conditions) -> Self {
        Self {
            start: value.start,
            heading: value.heading,
            wind: value.wind,
            turbulence: value.turbulence,
            start_epoch: value.start_epoch,
            time_rate: value.time_rate,
            world_terrain: value.world_terrain,
            terrain_fingerprint: value.terrain_fingerprint,
            climate_date: value.climate_date,
            climate_fingerprint: value.climate_fingerprint,
        }
    }
}

impl EnvironmentConditions {
    /// Select bundled world data for a new recording, before its first frame.
    #[must_use]
    pub const fn with_world_climate(
        mut self,
        world_terrain: bool,
        climate_date: Option<ClimateDate>,
    ) -> Self {
        self.world_terrain = world_terrain;
        self.terrain_fingerprint = if world_terrain {
            GLOBAL_TERRAIN_FINGERPRINT
        } else {
            0
        };
        self.climate_date = climate_date;
        self.climate_fingerprint = if climate_date.is_some() {
            GLOBAL_CLIMATE_FINGERPRINT
        } else {
            0
        };
        self
    }

    pub(crate) fn check_world(self) -> Result<(), ReplayError> {
        for (enabled, recorded, expected, detail) in [
            (
                self.world_terrain,
                self.terrain_fingerprint,
                GLOBAL_TERRAIN_FINGERPRINT,
                "the bundled global terrain dataset differs from the recording",
            ),
            (
                self.climate_date.is_some(),
                self.climate_fingerprint,
                GLOBAL_CLIMATE_FINGERPRINT,
                "the bundled climate dataset differs from the recording",
            ),
        ] {
            if enabled && recorded != expected {
                return Err(ReplayError::ConditionsMismatch {
                    detail: detail.to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// Conditions for a new complete-identity recording. There is deliberately no
/// default aircraft identity, legacy-fingerprint slot, or conversion from a
/// legacy recording. Recorded weather is immutable and validated independently.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentConditions {
    /// Informational only; never selects aircraft compatibility policy.
    pub aircraft_name: String,
    pub aircraft_identity: AircraftIdentity,
    pub environment: EnvironmentConditions,
    pub weather: WeatherSelection,
}

impl CurrentConditions {
    /// Start a new recording with complete aircraft identity and legacy weather
    /// semantics. External aircraft profiles must already have been validated.
    #[must_use]
    pub fn for_aircraft(config: &AircraftConfig, environment: EnvironmentConditions) -> Self {
        Self {
            aircraft_name: config.name.clone(),
            aircraft_identity: AircraftIdentity::for_config(config),
            environment,
            weather: WeatherSelection::Legacy,
        }
    }
}

/// Format-3 recording. Its sole aircraft identity is the complete descriptor.
///
/// ```
/// use flightsim_fdm::AircraftConfig;
/// use flightsim_sim::replay::{
///     CurrentConditions, CurrentRecorder, EnvironmentConditions, ReplayFile,
///     identity::AircraftCompatibility,
/// };
/// let aircraft = AircraftConfig::light_single();
/// let conditions = CurrentConditions::for_aircraft(&aircraft, EnvironmentConditions::default());
/// let recording = CurrentRecorder::new(conditions).finish();
/// let mut bytes = Vec::new();
/// recording.write_to(&mut bytes).unwrap();
/// let restored = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
/// assert_eq!(restored.format_version(), 3);
/// assert_eq!(restored.check_compatibility_with(&aircraft).unwrap(), AircraftCompatibility::CompleteMatch);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentRecording {
    conditions: CurrentConditions,
    frames: Vec<Frame>,
    keyframes: Vec<Keyframe>,
}

impl CurrentRecording {
    #[must_use]
    pub const fn conditions(&self) -> &CurrentConditions {
        &self.conditions
    }
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
    #[must_use]
    pub fn keyframes(&self) -> &[Keyframe] {
        &self.keyframes
    }
    #[must_use]
    pub fn duration(&self) -> Seconds {
        duration(&self.frames)
    }
    #[must_use]
    pub fn keyframe_at_or_before(&self, frame: u32) -> Option<Keyframe> {
        keyframe_at_or_before(&self.keyframes, frame)
    }
    #[must_use]
    pub fn keyframe_exactly_at(&self, frame: u32) -> Option<Keyframe> {
        keyframe_exactly_at(&self.keyframes, frame)
    }
    #[must_use]
    pub fn drift_at(&self, frame: u32, state: &RigidBodyState) -> Option<Meters> {
        drift_at(&self.keyframes, frame, state)
    }

    fn validate(&self) -> Result<(), ReplayError> {
        validate_identity(self.conditions.aircraft_identity)?;
        validate_records(
            &self.conditions.aircraft_name,
            &self.conditions.environment,
            &self.frames,
            &self.keyframes,
        )
    }

    /// Validate values/world data, then classify complete aircraft evidence.
    /// This is not playback permission: the caller must support recorded weather
    /// presentation, and regional-package replay remains unsupported.
    ///
    /// # Errors
    /// Invalid values, unsupported identity metadata, or mismatched world data.
    pub fn check_compatibility_with(
        &self,
        config: &AircraftConfig,
    ) -> Result<AircraftCompatibility, ReplayError> {
        self.validate()?;
        self.conditions.environment.check_world()?;
        Ok(RecordedAircraftIdentity::Complete(self.conditions.aircraft_identity).verify(config))
    }

    /// Write format 3, validating all values before writing any bytes.
    ///
    /// # Errors
    /// Invalid values or unsupported identity metadata; I/O can partially write.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        self.validate()?;
        let conditions = &self.conditions;
        let name = conditions.aircraft_name.as_bytes();
        // Validation bounds the name and the immutable weather type bounds W.
        let name_len = u32::try_from(name.len()).map_err(|_| ReplayError::TooLarge {
            what: "bytes of aircraft name",
            declared: u64::MAX,
            maximum: u64::from(MAX_NAME_BYTES),
        })?;
        let weather_len = weather_length(conditions.weather);
        let conditions_len = FIXED_CONDITIONS_BYTES + name_len + weather_len;
        writer.write_all(&MAGIC)?;
        writer.write_all(&CURRENT_FORMAT_VERSION.to_le_bytes())?;
        writer.write_all(&conditions_len.to_le_bytes())?;
        writer.write_all(&name_len.to_le_bytes())?;
        writer.write_all(name)?;
        let identity = conditions.aircraft_identity;
        writer.write_all(&identity.algorithm.to_le_bytes())?;
        writer.write_all(&identity.schema.to_le_bytes())?;
        writer.write_all(&identity.fdm_model_revision.to_le_bytes())?;
        writer.write_all(&identity.fingerprint.to_le_bytes())?;
        write_environment(writer, &conditions.environment)?;
        writer.write_all(&weather_len.to_le_bytes())?;
        if let WeatherSelection::Modeled(scenario) = conditions.weather {
            write_weather(writer, scenario)?;
        }
        write_records(writer, &self.frames, &self.keyframes)
    }

    /// Read exactly a v3 recording. For 1/2/3 dispatch use [`ReplayFile::read_from`].
    /// Like the legacy streaming reader, leaves any following outer-stream data
    /// unread; the bounded conditions and weather blocks must be exact.
    ///
    /// # Errors
    /// Unsupported versions/fields, invalid or excessive values, and I/O failure.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let version = read_header(reader)?;
        require_valid(
            version == CURRENT_FORMAT_VERSION,
            "current replay format",
            None,
            "CurrentRecording requires format 3; use ReplayFile for legacy input",
        )?;
        Self::read_body(reader)
    }

    fn read_body<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let conditions_len = read_u32(reader)?;
        bounded_length("bytes of conditions", conditions_len, MAX_CONDITIONS_BYTES)?;
        require_valid(
            conditions_len >= FIXED_CONDITIONS_BYTES,
            "conditions length",
            None,
            "must contain the entire fixed conditions envelope",
        )?;
        let conditions = {
            let mut bounded = reader.take(u64::from(conditions_len));
            let name_len = read_u32(&mut bounded)?;
            bounded_length("bytes of aircraft name", name_len, MAX_NAME_BYTES)?;
            require_valid(
                FIXED_CONDITIONS_BYTES + name_len <= conditions_len,
                "conditions length",
                None,
                "must contain the declared name and all fixed fields",
            )?;
            let mut name = vec![0; name_len as usize];
            bounded.read_exact(&mut name)?;
            let aircraft_name = String::from_utf8(name).map_err(|_| ReplayError::InvalidName)?;
            let aircraft_identity = AircraftIdentity {
                algorithm: read_u16(&mut bounded)?,
                schema: read_u16(&mut bounded)?,
                fdm_model_revision: read_u32(&mut bounded)?,
                fingerprint: read_u64(&mut bounded)?,
            };
            validate_identity(aircraft_identity)?;
            let environment = read_environment(&mut bounded)?;
            validate_environment(&environment)?;
            let weather_len = read_u32(&mut bounded)?;
            bounded_length("bytes of weather", weather_len, MAX_WEATHER_BYTES)?;
            require_valid(
                weather_len == 0 || weather_len >= 8,
                "weather length",
                None,
                "a present block must contain the complete eight-byte header",
            )?;
            require_valid(
                conditions_len == FIXED_CONDITIONS_BYTES + name_len + weather_len,
                "conditions length",
                None,
                "must equal 136 + name length + weather length",
            )?;
            let weather = if weather_len == 0 {
                WeatherSelection::Legacy
            } else {
                let mut block = (&mut bounded).take(u64::from(weather_len));
                let scenario = read_weather(&mut block, weather_len)?;
                require_valid(
                    block.limit() == 0,
                    "weather length",
                    None,
                    "the typed weather block must be consumed exactly",
                )?;
                WeatherSelection::Modeled(scenario)
            };
            require_valid(
                bounded.limit() == 0,
                "conditions length",
                None,
                "conditions must be consumed exactly before reading frame counts",
            )?;
            CurrentConditions {
                aircraft_name,
                aircraft_identity,
                environment,
                weather,
            }
        };
        let (frames, keyframes) = read_records(
            reader,
            conditions.environment.start_epoch,
            conditions.environment.time_rate,
        )?;
        Ok(Self {
            conditions,
            frames,
            keyframes,
        })
    }
}

/// New-recording API for complete identity, with the unchanged frame/keyframe cap.
/// Values are retained unchanged; write/check boundaries validate them.
#[derive(Debug, Clone)]
pub struct CurrentRecorder {
    recording: CurrentRecording,
}

impl CurrentRecorder {
    #[must_use]
    pub const fn new(conditions: CurrentConditions) -> Self {
        Self {
            recording: CurrentRecording {
                conditions,
                frames: Vec::new(),
                keyframes: Vec::new(),
            },
        }
    }
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        u32::try_from(self.recording.frames.len()).unwrap_or(u32::MAX)
    }
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.recording.frames.len() >= MAX_FRAMES as usize
    }
    /// Store one unchanged duration/effective-control pair. At the existing cap,
    /// further frames are discarded. State is pre-step, as in [`super::Recorder`].
    pub fn record(
        &mut self,
        frame_time: Seconds,
        controls: ControlInputs,
        state: Option<&RigidBodyState>,
    ) {
        if self.is_full() {
            return;
        }
        let frame = self.frame_count();
        if frame % KEYFRAME_INTERVAL == 0
            && let Some(state) = state
        {
            self.recording.keyframes.push(Keyframe {
                frame,
                state: *state,
            });
        }
        self.recording.frames.push(Frame {
            frame_time,
            controls,
        });
    }
    #[must_use]
    pub const fn recording(&self) -> &CurrentRecording {
        &self.recording
    }
    #[must_use]
    pub fn finish(self) -> CurrentRecording {
        self.recording
    }
}

/// Explicit on-disk format and aircraft evidence. Export preserves each variant,
/// including a v2 input with a disabled (all-zero) world/climate extension.
/// No implicit legacy-to-current conversion is provided.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayFile {
    V1(Recording),
    V2(Recording),
    V3(CurrentRecording),
}

impl ReplayFile {
    /// Dispatch only the explicitly supported versions; never infer identity
    /// algorithms by trying candidate hashes against an untagged legacy u64.
    ///
    /// # Errors
    /// Malformed/unsupported fields, excessive counts or lengths, and I/O failure.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let version = read_header(reader)?;
        match version {
            LEGACY_FORMAT_VERSION | WORLD_FORMAT_VERSION => {
                // Reuse the legacy parser verbatim from its original header.
                // This small prefix is bounded and does not copy the file.
                let mut header = [0_u8; 10];
                header[..8].copy_from_slice(&MAGIC);
                header[8..].copy_from_slice(&version.to_le_bytes());
                let recording = Recording::read_from(&mut Cursor::new(header).chain(reader))?;
                Ok(if version == LEGACY_FORMAT_VERSION {
                    Self::V1(recording)
                } else {
                    Self::V2(recording)
                })
            }
            CURRENT_FORMAT_VERSION => Ok(Self::V3(CurrentRecording::read_body(reader)?)),
            _ => Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: CURRENT_FORMAT_VERSION,
            }),
        }
    }
    /// Export the retained format, without upgrading legacy aircraft evidence.
    ///
    /// # Errors
    /// Invalid values, a V1 variant with enabled world/climate, or I/O failure.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        match self {
            Self::V1(recording) => recording.write_v1_to(writer),
            Self::V2(recording) => recording.write_v2_to(writer),
            Self::V3(recording) => recording.write_to(writer),
        }
    }
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        match self {
            Self::V1(_) => LEGACY_FORMAT_VERSION,
            Self::V2(_) => WORLD_FORMAT_VERSION,
            Self::V3(_) => CURRENT_FORMAT_VERSION,
        }
    }
    #[must_use]
    pub fn aircraft_name(&self) -> &str {
        match self {
            Self::V1(r) | Self::V2(r) => &r.conditions.aircraft_name,
            Self::V3(r) => &r.conditions.aircraft_name,
        }
    }
    #[must_use]
    pub const fn aircraft_identity(&self) -> RecordedAircraftIdentity {
        match self {
            Self::V1(r) | Self::V2(r) => r.conditions.aircraft_identity(),
            Self::V3(r) => RecordedAircraftIdentity::Complete(r.conditions.aircraft_identity),
        }
    }
    #[must_use]
    pub fn environment(&self) -> EnvironmentConditions {
        match self {
            Self::V1(r) | Self::V2(r) => EnvironmentConditions::from(&r.conditions),
            Self::V3(r) => r.conditions.environment,
        }
    }
    #[must_use]
    pub const fn weather(&self) -> WeatherSelection {
        match self {
            Self::V1(_) | Self::V2(_) => WeatherSelection::Legacy,
            Self::V3(r) => r.conditions.weather,
        }
    }
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        match self {
            Self::V1(r) | Self::V2(r) => r.frames(),
            Self::V3(r) => r.frames(),
        }
    }
    #[must_use]
    pub fn keyframes(&self) -> &[Keyframe] {
        match self {
            Self::V1(r) | Self::V2(r) => r.keyframes(),
            Self::V3(r) => r.keyframes(),
        }
    }
    #[must_use]
    pub fn duration(&self) -> Seconds {
        duration(self.frames())
    }
    #[must_use]
    pub fn keyframe_at_or_before(&self, frame: u32) -> Option<Keyframe> {
        keyframe_at_or_before(self.keyframes(), frame)
    }
    #[must_use]
    pub fn keyframe_exactly_at(&self, frame: u32) -> Option<Keyframe> {
        keyframe_exactly_at(self.keyframes(), frame)
    }
    #[must_use]
    pub fn drift_at(&self, frame: u32, state: &RigidBodyState) -> Option<Meters> {
        drift_at(self.keyframes(), frame, state)
    }
    /// Classify aircraft evidence after value/world validation, without granting
    /// playback permission or establishing legacy files' omitted coefficients.
    ///
    /// # Errors
    /// Invalid values, unsupported current metadata or mismatched world data.
    pub fn check_compatibility_with(
        &self,
        config: &AircraftConfig,
    ) -> Result<AircraftCompatibility, ReplayError> {
        if let Self::V1(recording) = self {
            require_valid(
                !recording.conditions.world_terrain && recording.conditions.climate_date.is_none(),
                "format 1 world/climate",
                None,
                "format 1 cannot represent enabled world or climate data",
            )?;
        }
        match self {
            Self::V1(r) | Self::V2(r) => r.check_compatibility_with(config),
            Self::V3(r) => r.check_compatibility_with(config),
        }
    }
}

pub(crate) fn read_header<R: Read>(reader: &mut R) -> Result<u16, ReplayError> {
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(ReplayError::NotAReplay { found: magic });
    }
    Ok(read_u16(reader)?)
}

fn bounded_length(what: &'static str, declared: u32, maximum: u32) -> Result<(), ReplayError> {
    if declared > maximum {
        return Err(ReplayError::TooLarge {
            what,
            declared: u64::from(declared),
            maximum: u64::from(maximum),
        });
    }
    Ok(())
}

fn validate_identity(identity: AircraftIdentity) -> Result<(), ReplayError> {
    for (valid, field, requirement) in [
        (
            identity.algorithm == AIRCRAFT_IDENTITY_ALGORITHM,
            "aircraft identity algorithm",
            "must be supported algorithm 1",
        ),
        (
            identity.schema == AIRCRAFT_IDENTITY_SCHEMA,
            "aircraft identity schema",
            "must be supported schema 1",
        ),
        (
            identity.fdm_model_revision == FDM_MODEL_REVISION,
            "aircraft FDM model revision",
            "must match the supported FDM model revision",
        ),
    ] {
        require_valid(valid, field, None, requirement)?;
    }
    Ok(())
}

pub(crate) fn write_environment<W: Write>(
    writer: &mut W,
    e: &EnvironmentConditions,
) -> Result<(), ReplayError> {
    for value in [
        e.start.latitude.get(),
        e.start.longitude.get(),
        e.start.altitude.get(),
        e.heading.get(),
        e.wind.from.get(),
        e.wind.speed.get(),
        e.turbulence.intensity.get(),
    ] {
        write_f64(writer, value)?;
    }
    writer.write_all(&e.turbulence.seed.to_le_bytes())?;
    write_f64(writer, e.start_epoch)?;
    write_f64(writer, e.time_rate)?;
    let flags = u64::from(e.world_terrain) | (u64::from(e.climate_date.is_some()) << 1);
    writer.write_all(&flags.to_le_bytes())?;
    writer.write_all(&e.terrain_fingerprint.to_le_bytes())?;
    write_f64(
        writer,
        e.climate_date.map_or(0.0, ClimateDate::annual_phase),
    )?;
    writer.write_all(&e.climate_fingerprint.to_le_bytes())?;
    Ok(())
}

pub(crate) fn read_environment<R: Read>(
    reader: &mut R,
) -> Result<EnvironmentConditions, ReplayError> {
    let start = Geodetic {
        latitude: Radians(read_f64(reader)?),
        longitude: Radians(read_f64(reader)?),
        altitude: Meters(read_f64(reader)?),
    };
    let heading = Radians(read_f64(reader)?);
    let wind = Wind {
        from: Radians(read_f64(reader)?),
        speed: MetersPerSecond(read_f64(reader)?),
    };
    let turbulence = Turbulence {
        intensity: MetersPerSecond(read_f64(reader)?),
        seed: read_u64(reader)?,
    };
    let start_epoch = read_f64(reader)?;
    let time_rate = read_f64(reader)?;
    let flags = read_u64(reader)?;
    require_valid(
        flags & !3 == 0,
        "world/climate flags",
        None,
        "only global terrain and climate flag bits are defined",
    )?;
    let terrain_fingerprint = read_u64(reader)?;
    let phase = read_f64(reader)?;
    let climate_fingerprint = read_u64(reader)?;
    let climate_date = if flags & 2 != 0 {
        Some(
            ClimateDate::from_annual_phase(phase).ok_or(ReplayError::InvalidValue {
                field: "climate annual phase",
                frame: None,
                requirement: "must be finite and in [0, 1)",
            })?,
        )
    } else {
        require_valid(
            phase.to_bits() == 0,
            "disabled climate annual phase",
            None,
            "must be positive zero when climate is disabled",
        )?;
        None
    };
    Ok(EnvironmentConditions {
        start,
        heading,
        wind,
        turbulence,
        start_epoch,
        time_rate,
        world_terrain: flags & 1 != 0,
        terrain_fingerprint,
        climate_date,
        climate_fingerprint,
    })
}

pub(crate) const fn weather_length(selection: WeatherSelection) -> u32 {
    match selection {
        WeatherSelection::Legacy => 0,
        WeatherSelection::Modeled(scenario) => {
            let p = scenario.parameters();
            62 + if p.cloud.is_some() { 34 } else { 0 } + if p.fog.is_some() { 24 } else { 0 }
        }
    }
}

pub(crate) fn write_weather<W: Write>(
    writer: &mut W,
    scenario: WeatherScenario,
) -> Result<(), ReplayError> {
    let p = scenario.parameters();
    writer.write_all(&p.parameter_schema.to_le_bytes())?;
    writer.write_all(&(p.source as u16).to_le_bytes())?;
    writer.write_all(&p.model_revision.to_le_bytes())?;
    writer.write_all(&(p.preset as u16).to_le_bytes())?;
    writer.write_all(&(p.precipitation_kind as u16).to_le_bytes())?;
    writer.write_all(&p.seed.to_le_bytes())?;
    for value in [
        p.departure_reference.latitude.get(),
        p.departure_reference.longitude.get(),
        p.departure_reference.altitude.get(),
        p.ambient_visibility.get(),
        p.precipitation_rate.0,
    ] {
        write_f64(writer, value)?;
    }
    let flags = u16::from(p.cloud.is_some()) | (u16::from(p.fog.is_some()) << 1);
    writer.write_all(&flags.to_le_bytes())?;
    if let Some(cloud) = p.cloud {
        writer.write_all(&(cloud.morphology as u16).to_le_bytes())?;
        for value in [
            cloud.base.get(),
            cloud.top.get(),
            cloud.coverage,
            cloud.visibility.get(),
        ] {
            write_f64(writer, value)?;
        }
    }
    if let Some(fog) = p.fog {
        for value in [fog.bottom.get(), fog.top.get(), fog.visibility.get()] {
            write_f64(writer, value)?;
        }
    }
    Ok(())
}

fn weather_error(error: WeatherError) -> ReplayError {
    // Preserve typed weather diagnostics without broadening the longstanding
    // ReplayError public variants (callers may exhaustively match them).
    ReplayError::ConditionsMismatch {
        detail: format!("invalid recorded weather: {error}"),
    }
}

pub(crate) fn read_weather<R: Read>(
    reader: &mut R,
    length: u32,
) -> Result<WeatherScenario, ReplayError> {
    require_valid(
        matches!(length, 62 | 86 | 96 | 120),
        "weather length",
        None,
        "schema 1 requires exactly 62, 86, 96 or 120 bytes",
    )?;
    let parameter_schema = read_u16(reader)?;
    let source = WeatherSource::try_from(read_u16(reader)?).map_err(weather_error)?;
    let model_revision = read_u32(reader)?;
    if parameter_schema != WEATHER_PARAMETER_SCHEMA {
        return Err(weather_error(WeatherError::UnsupportedSchema(
            parameter_schema,
        )));
    }
    if model_revision != WEATHER_MODEL_REVISION {
        return Err(weather_error(WeatherError::UnsupportedModelRevision(
            model_revision,
        )));
    }
    let preset = WeatherPreset::try_from(read_u16(reader)?).map_err(weather_error)?;
    let precipitation_kind =
        PrecipitationKind::try_from(read_u16(reader)?).map_err(weather_error)?;
    let seed = read_u64(reader)?;
    let departure_reference = Geodetic {
        latitude: Radians(read_f64(reader)?),
        longitude: Radians(read_f64(reader)?),
        altitude: Meters(read_f64(reader)?),
    };
    let ambient_visibility = Meters(read_f64(reader)?);
    let precipitation_rate = WaterEquivalentRate(read_f64(reader)?);
    let flags = read_u16(reader)?;
    require_valid(
        flags & !3 == 0,
        "weather layer flags",
        None,
        "only cloud and fog flag bits are defined",
    )?;
    let expected = 62 + if flags & 1 != 0 { 34 } else { 0 } + if flags & 2 != 0 { 24 } else { 0 };
    require_valid(
        length == expected,
        "weather length",
        None,
        "must match the exact layer flags before reading layers",
    )?;
    let cloud = if flags & 1 != 0 {
        Some(ModeledCloudLayer {
            morphology: CloudMorphology::try_from(read_u16(reader)?).map_err(weather_error)?,
            base: Meters(read_f64(reader)?),
            top: Meters(read_f64(reader)?),
            coverage: read_f64(reader)?,
            visibility: Meters(read_f64(reader)?),
        })
    } else {
        None
    };
    let fog = if flags & 2 != 0 {
        Some(ModeledFogLayer {
            bottom: Meters(read_f64(reader)?),
            top: Meters(read_f64(reader)?),
            visibility: Meters(read_f64(reader)?),
        })
    } else {
        None
    };
    WeatherScenario::try_from(WeatherParameters {
        parameter_schema,
        source,
        model_revision,
        preset,
        seed,
        departure_reference,
        ambient_visibility,
        precipitation_kind,
        precipitation_rate,
        cloud,
        fog,
    })
    .map_err(weather_error)
}

fn duration(frames: &[Frame]) -> Seconds {
    Seconds(
        frames
            .iter()
            .fold(0.0, |elapsed, frame| elapsed + frame.frame_time.get()),
    )
}
fn keyframe_at_or_before(keys: &[Keyframe], frame: u32) -> Option<Keyframe> {
    match keys.binary_search_by_key(&frame, |key| key.frame) {
        Ok(index) => Some(keys[index]),
        Err(0) => None,
        Err(index) => Some(keys[index - 1]),
    }
}
fn keyframe_exactly_at(keys: &[Keyframe], frame: u32) -> Option<Keyframe> {
    keys.binary_search_by_key(&frame, |key| key.frame)
        .ok()
        .map(|index| keys[index])
}
fn drift_at(keys: &[Keyframe], frame: u32, state: &RigidBodyState) -> Option<Meters> {
    keyframe_exactly_at(keys, frame)
        .map(|key| Meters((state.position.0 - key.state.position.0).length()))
}
