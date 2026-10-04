//! Bounded, explicitly model-aware replay v4. Old ReplayFile/Current* remain v3.
//! See docs/replay-v4.md for independently encoded field order and semantics.
use crate::model_identity::ModelIdentity;
use crate::model_simulation::{
    JET_FIXED_DT, JET_SIMULATION_REVISION, JetAdvance, JetEnvironment, JetPresentationSnapshot,
    JetSimulation, JetTerminalEvent, JetTerrain,
};
use crate::replay::current::{
    read_environment, read_header, read_weather, weather_length, write_environment, write_weather,
};
use crate::replay::{self, Keyframe, MAGIC, MAX_FRAMES, MAX_NAME_BYTES, ReplayError, ReplayFile};
use crate::replay::{
    read_f64, read_u16, read_u32, read_u64, require_valid, validate_frame_values,
    validate_visual_time, write_f64,
};
use crate::weather::WeatherSelection;
use flightsim_core::{Ecef, Meters, Seconds};
use flightsim_fdm::subsonic::{
    AxisStatus, JetAircraftConfig, JetConditions, JetDomainStatus, JetFailureReason,
    JetInvalidInput, JetStage, JetStepError, MachNumber, PressureRatio, TemperatureRatio,
};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use glam::{DQuat, DVec3};
use std::io::{Cursor, Read, Write};

pub const MODEL_FORMAT_VERSION: u16 = 4;
pub const MAX_MODEL_CONDITIONS_BYTES: u32 = 4096;
pub const MAX_MODEL_CHECKPOINTS: u32 = 8334;
pub const MAX_REPLAY_WORK: u32 = 240;
const CONDITIONS_FIXED_BYTES: u32 = 255;
const CHECKPOINT_INTERVAL: u32 = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct JetConditionsRecord {
    pub aircraft_name: String,
    pub identity: ModelIdentity,
    pub simulation_revision: u32,
    pub environment: JetEnvironment,
    pub initial_state: RigidBodyState,
}
impl JetConditionsRecord {
    fn validate(&self) -> Result<(), ReplayError> {
        bound("aircraft name", self.aircraft_name.len(), MAX_NAME_BYTES)?;
        require_valid(
            self.identity.supported(),
            "model identity",
            None,
            "requires algorithm1/schema2/kind2/law1",
        )?;
        require_valid(
            self.simulation_revision == JET_SIMULATION_REVISION,
            "simulation law",
            None,
            "requires jet simulation law1",
        )?;
        self.environment.validate()?;
        validate_model_state(&self.initial_state, 0)?;
        require_valid(
            self.environment
                .conditions
                .start
                .to_ecef()
                .as_vec()
                .distance(self.initial_state.position.as_vec())
                <= 1e-6,
            "initial start position",
            None,
            "must denote actual CG within one micrometre ECEF",
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JetRecording {
    conditions: JetConditionsRecord,
    controls: Vec<ControlInputs>,
    checkpoints: Vec<Keyframe>,
    terminal: Option<JetTerminalEvent>,
}
impl JetRecording {
    #[must_use]
    pub const fn conditions(&self) -> &JetConditionsRecord {
        &self.conditions
    }
    #[must_use]
    pub fn controls(&self) -> &[ControlInputs] {
        &self.controls
    }
    #[must_use]
    pub fn checkpoints(&self) -> &[Keyframe] {
        &self.checkpoints
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<JetTerminalEvent> {
        self.terminal
    }
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.controls
            .iter()
            .fold(Seconds::ZERO, |sum, _| sum + JET_FIXED_DT)
    }
    #[must_use]
    pub fn final_state(&self) -> &RigidBodyState {
        &self
            .checkpoints
            .last()
            .expect("validated mandatory final checkpoint")
            .state
    }
    fn count(&self) -> u32 {
        u32::try_from(self.controls.len()).expect("bounded controls")
    }
    /// # Errors
    /// Unsupported metadata, bounds, invalid scalars or inconsistent checkpoints.
    pub fn validate(&self) -> Result<(), ReplayError> {
        self.conditions.validate()?;
        bound("committed controls", self.controls.len(), MAX_FRAMES)?;
        bound("checkpoints", self.checkpoints.len(), MAX_MODEL_CHECKPOINTS)?;
        let n = self.count();
        for (i, control) in self.controls.iter().enumerate() {
            validate_controls(*control, u32::try_from(i).expect("bounded"))?;
        }
        let expected = n.div_ceil(CHECKPOINT_INTERVAL).max(1);
        require_valid(
            self.checkpoints.len() == expected as usize,
            "checkpoint count",
            None,
            "requires periodic checkpoints and exactly one final",
        )?;
        for (i, key) in self.checkpoints.iter().enumerate() {
            let expected_cursor =
                ((u32::try_from(i).expect("bounded") + 1) * CHECKPOINT_INTERVAL).min(n);
            require_valid(
                key.frame == expected_cursor,
                "checkpoint cursor",
                Some(key.frame),
                "must be periodic or final without duplicates",
            )?;
            validate_model_state(&key.state, key.frame)?;
        }
        if n == 0 {
            require_valid(
                state_bits_equal(self.final_state(), &self.conditions.initial_state),
                "zero-frame final state",
                None,
                "must match initial state bits",
            )?;
        }
        if let Some(event) = self.terminal {
            require_valid(
                event.cursor == n,
                "terminal cursor",
                None,
                "must equal successful input count",
            )?;
            validate_controls(event.controls, n)?;
            validate_failure(event.failure)?;
        }
        validate_visual_time(
            self.conditions.environment.conditions.start_epoch,
            self.conditions.environment.conditions.time_rate,
            self.duration().get(),
        )
    }
    /// # Errors
    /// Validation or output I/O errors. Validation finishes before writing.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        self.validate()?;
        writer.write_all(&MAGIC)?;
        writer.write_all(&MODEL_FORMAT_VERSION.to_le_bytes())?;
        let c = &self.conditions;
        let name_len = u32::try_from(c.aircraft_name.len()).expect("bounded");
        let w = weather_length(c.environment.weather);
        writer.write_all(&(CONDITIONS_FIXED_BYTES + name_len + w).to_le_bytes())?;
        writer.write_all(&name_len.to_le_bytes())?;
        writer.write_all(c.aircraft_name.as_bytes())?;
        let id = c.identity;
        for value in [id.algorithm, id.schema, id.kind] {
            writer.write_all(&value.to_le_bytes())?;
        }
        writer.write_all(&id.law_revision.to_le_bytes())?;
        writer.write_all(&id.fingerprint.to_le_bytes())?;
        writer.write_all(&c.simulation_revision.to_le_bytes())?;
        write_environment(writer, &c.environment.conditions)?;
        match c.environment.terrain {
            JetTerrain::Flat { elevation } => {
                writer.write_all(&[0])?;
                write_f64(writer, elevation.get())?;
            }
            JetTerrain::BundledGlobal => {
                writer.write_all(&[1])?;
                write_f64(writer, 0.0)?;
            }
        }
        writer.write_all(&w.to_le_bytes())?;
        if let WeatherSelection::Modeled(scenario) = c.environment.weather {
            write_weather(writer, scenario)?;
        }
        write_state(writer, &c.initial_state)?;
        writer.write_all(&self.count().to_le_bytes())?;
        for control in &self.controls {
            write_controls(writer, *control)?;
        }
        writer.write_all(
            &u32::try_from(self.checkpoints.len())
                .expect("bounded")
                .to_le_bytes(),
        )?;
        for key in &self.checkpoints {
            writer.write_all(&key.frame.to_le_bytes())?;
            write_state(writer, &key.state)?;
        }
        if let Some(event) = self.terminal {
            writer.write_all(
                &(if event.failure.query.is_some() {
                    85_u32
                } else {
                    61_u32
                })
                .to_le_bytes(),
            )?;
            writer.write_all(&event.cursor.to_le_bytes())?;
            write_controls(writer, event.controls)?;
            write_failure(writer, event.failure)?;
        } else {
            writer.write_all(&0_u32.to_le_bytes())?;
        }
        Ok(())
    }
    /// # Errors
    /// Invalid/unsupported bounded data or input I/O. Outer trailing bytes remain unread.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let version = read_header(reader)?;
        if version != MODEL_FORMAT_VERSION {
            return Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: MODEL_FORMAT_VERSION,
            });
        }
        Self::read_body(reader)
    }
    fn read_body<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let length = read_u32(reader)?;
        bound(
            "model conditions bytes",
            length as usize,
            MAX_MODEL_CONDITIONS_BYTES,
        )?;
        require_valid(
            length >= CONDITIONS_FIXED_BYTES,
            "model conditions length",
            None,
            "too short",
        )?;
        let mut block = reader.take(u64::from(length));
        let name_len = read_u32(&mut block)?;
        bound("aircraft name", name_len as usize, MAX_NAME_BYTES)?;
        require_valid(
            length >= CONDITIONS_FIXED_BYTES + name_len,
            "model conditions length",
            None,
            "inconsistent name length",
        )?;
        let mut name = vec![0; name_len as usize];
        block.read_exact(&mut name)?;
        let aircraft_name = String::from_utf8(name).map_err(|_| ReplayError::InvalidName)?;
        let identity = ModelIdentity {
            algorithm: read_u16(&mut block)?,
            schema: read_u16(&mut block)?,
            kind: read_u16(&mut block)?,
            law_revision: read_u32(&mut block)?,
            fingerprint: read_u64(&mut block)?,
        };
        require_valid(
            identity.supported(),
            "model identity",
            None,
            "requires supported schema2 jet identity",
        )?;
        let simulation_revision = read_u32(&mut block)?;
        require_valid(
            simulation_revision == JET_SIMULATION_REVISION,
            "simulation law",
            None,
            "unsupported",
        )?;
        let conditions = read_environment(&mut block)?;
        let terrain_kind = read_u8(&mut block)?;
        let elevation = read_f64(&mut block)?;
        let terrain = match terrain_kind {
            0 => JetTerrain::Flat {
                elevation: Meters(elevation),
            },
            1 if elevation.to_bits() == 0 => JetTerrain::BundledGlobal,
            _ => {
                return Err(invalid(
                    "terrain kind/elevation",
                    "requires flat0 or global1 with positive zero elevation",
                ));
            }
        };
        let w = read_u32(&mut block)?;
        bound("weather bytes", w as usize, replay::MAX_WEATHER_BYTES)?;
        require_valid(
            length == CONDITIONS_FIXED_BYTES + name_len + w,
            "model conditions length",
            None,
            "must match exact name/weather lengths",
        )?;
        let weather = if w == 0 {
            WeatherSelection::Legacy
        } else {
            let mut weather_block = (&mut block).take(u64::from(w));
            let result = read_weather(&mut weather_block, w)?;
            require_valid(
                weather_block.limit() == 0,
                "weather length",
                None,
                "must consume exact block",
            )?;
            WeatherSelection::Modeled(result)
        };
        let initial_state = read_state(&mut block)?;
        require_valid(
            block.limit() == 0,
            "conditions length",
            None,
            "must consume exact block",
        )?;
        let conditions = JetConditionsRecord {
            aircraft_name,
            identity,
            simulation_revision,
            environment: JetEnvironment {
                conditions,
                terrain,
                weather,
            },
            initial_state,
        };
        conditions.validate()?;
        let n = read_u32(reader)?;
        bound("committed controls", n as usize, MAX_FRAMES)?;
        let mut controls = reserve(n, "committed controls")?;
        for cursor in 0..n {
            controls.push(read_controls(reader, cursor)?);
        }
        let k = read_u32(reader)?;
        bound("checkpoints", k as usize, MAX_MODEL_CHECKPOINTS)?;
        require_valid(
            k == n.div_ceil(CHECKPOINT_INTERVAL).max(1),
            "checkpoint count",
            None,
            "must match exact required count",
        )?;
        let mut checkpoints = reserve(k, "checkpoints")?;
        for i in 0..k {
            let frame = read_u32(reader)?;
            require_valid(
                frame == ((i + 1) * CHECKPOINT_INTERVAL).min(n),
                "checkpoint cursor",
                Some(frame),
                "must be periodic or final",
            )?;
            let state = read_state(reader)?;
            validate_model_state(&state, frame)?;
            checkpoints.push(Keyframe { frame, state });
        }
        let terminal_length = read_u32(reader)?;
        let terminal = if terminal_length == 0 {
            None
        } else {
            require_valid(
                matches!(terminal_length, 61 | 85),
                "terminal length",
                None,
                "must be0/61/85",
            )?;
            let mut event_block = reader.take(u64::from(terminal_length));
            let cursor = read_u32(&mut event_block)?;
            require_valid(
                cursor == n,
                "terminal cursor",
                None,
                "must equal successful count",
            )?;
            let controls = read_controls(&mut event_block, cursor)?;
            let failure = read_failure(&mut event_block, terminal_length)?;
            require_valid(
                event_block.limit() == 0,
                "terminal length",
                None,
                "must consume exact block",
            )?;
            Some(JetTerminalEvent {
                cursor,
                controls,
                failure,
            })
        };
        let recording = Self {
            conditions,
            controls,
            checkpoints,
            terminal,
        };
        recording.validate()?;
        Ok(recording)
    }
}

/// The existing wrapper is retained as a distinct variant, never converted.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Explicit owned format dispatch retains the existing public wrapper; neither variant is copied per step"
)]
pub enum ModelReplayFile {
    Existing(ReplayFile),
    V4(JetRecording),
}
impl ModelReplayFile {
    /// # Errors
    /// Underlying bounded format validation or I/O.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let version = read_header(reader)?;
        if version == MODEL_FORMAT_VERSION {
            return JetRecording::read_body(reader).map(Self::V4);
        }
        if !(1..=3).contains(&version) {
            return Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: MODEL_FORMAT_VERSION,
            });
        }
        let mut prefix = MAGIC.to_vec();
        prefix.extend(version.to_le_bytes());
        ReplayFile::read_from(&mut Cursor::new(prefix).chain(reader)).map(Self::Existing)
    }
    /// # Errors
    /// Underlying format validation or I/O.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), ReplayError> {
        match self {
            Self::Existing(value) => value.write_to(writer),
            Self::V4(value) => value.write_to(writer),
        }
    }
}

/// Captures only successful reports and their one possible terminal attempt.
/// A recording error permanently closes the recorder at its prior authentic state.
#[derive(Debug)]
pub struct JetRecorder {
    recording: JetRecording,
    last: RigidBodyState,
    closed: bool,
}
impl JetRecorder {
    /// # Errors
    /// A recording must begin with a pristine, nonterminal simulation.
    pub fn new(sim: &JetSimulation) -> Result<Self, ReplayError> {
        let snapshot = sim.snapshot();
        require_valid(
            snapshot.committed_steps == 0
                && snapshot.elapsed.get().to_bits() == 0
                && sim.accumulated().get().to_bits() == 0
                && sim.terminal().is_none(),
            "recorder start",
            None,
            "requires pristine nonterminal simulation",
        )?;
        let mut environment = sim.environment();
        environment.conditions.start = sim.state().geodetic();
        environment.conditions.heading = sim.state().attitude().yaw;
        let conditions = JetConditionsRecord {
            aircraft_name: sim.config().airframe().name().to_owned(),
            identity: ModelIdentity::for_jet(sim.config()),
            simulation_revision: JET_SIMULATION_REVISION,
            environment,
            initial_state: *sim.state(),
        };
        conditions.validate()?;
        Ok(Self {
            recording: JetRecording {
                conditions,
                controls: Vec::new(),
                checkpoints: Vec::new(),
                terminal: None,
            },
            last: *sim.state(),
            closed: false,
        })
    }
    #[must_use]
    pub const fn closed(&self) -> bool {
        self.closed
    }
    /// # Errors
    /// Closed/capped recorder or a noncontiguous report. The recorder stays frozen.
    pub fn record(&mut self, report: &JetAdvance) -> Result<(), ReplayError> {
        let result = self.validate_report(report);
        if let Err(error) = result {
            self.closed = true;
            return Err(error);
        }
        for step in &report.committed {
            self.recording.controls.push(step.controls);
            self.last = step.after;
            if step.cursor % CHECKPOINT_INTERVAL == 0 {
                self.recording.checkpoints.push(Keyframe {
                    frame: step.cursor,
                    state: step.after,
                });
            }
        }
        if let Some(event) = report.terminal {
            self.recording.terminal = Some(event);
            self.closed = true;
        }
        Ok(())
    }
    fn validate_report(&self, report: &JetAdvance) -> Result<(), ReplayError> {
        require_valid(!self.closed, "recorder", None, "recording is closed")?;
        bound(
            "committed controls",
            self.recording
                .controls
                .len()
                .saturating_add(report.committed.len()),
            MAX_FRAMES,
        )?;
        let mut cursor = self.recording.count();
        let mut state = self.last;
        for step in &report.committed {
            cursor += 1;
            require_valid(
                step.cursor == cursor && state_bits_equal(&step.before, &state),
                "recording report",
                Some(cursor),
                "must continue the exact previous state/cursor",
            )?;
            state = step.after;
        }
        if let Some(event) = report.terminal {
            require_valid(
                event.cursor == cursor,
                "terminal cursor",
                None,
                "must follow committed report",
            )?;
            validate_failure(event.failure)?;
        }
        Ok(())
    }
    /// Export an independently owned, valid recording through the most recent
    /// accepted report. Does not close the recorder or add temporary checkpoints
    /// to ongoing recording. Empty and frame-zero terminal exports are valid.
    #[must_use]
    pub fn export(&self) -> JetRecording {
        let mut recording = self.recording.clone();
        Self::append_final_checkpoint(&mut recording, self.last);
        recording
    }
    #[must_use]
    pub fn finish(mut self) -> JetRecording {
        Self::append_final_checkpoint(&mut self.recording, self.last);
        self.recording
    }
    fn append_final_checkpoint(recording: &mut JetRecording, last: RigidBodyState) {
        let n = recording.count();
        if recording
            .checkpoints
            .last()
            .is_none_or(|key| key.frame != n)
        {
            recording.checkpoints.push(Keyframe {
                frame: n,
                state: last,
            });
        }
    }
}

fn bound(what: &'static str, value: usize, max: u32) -> Result<(), ReplayError> {
    if value > max as usize {
        Err(ReplayError::TooLarge {
            what,
            declared: u64::try_from(value).unwrap_or(u64::MAX),
            maximum: u64::from(max),
        })
    } else {
        Ok(())
    }
}
fn reserve<T>(count: u32, what: &'static str) -> Result<Vec<T>, ReplayError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count as usize)
        .map_err(|_| ReplayError::OutOfMemory {
            what,
            count: count as usize,
        })?;
    Ok(result)
}
fn invalid(field: &'static str, requirement: &'static str) -> ReplayError {
    ReplayError::InvalidValue {
        field,
        frame: None,
        requirement,
    }
}
fn mismatch(detail: impl Into<String>) -> ReplayError {
    ReplayError::ConditionsMismatch {
        detail: detail.into(),
    }
}
fn read_u8<R: Read>(r: &mut R) -> Result<u8, ReplayError> {
    let mut byte = [0];
    r.read_exact(&mut byte)?;
    Ok(byte[0])
}
fn controls_values(c: ControlInputs) -> [f64; 6] {
    [
        c.aileron(),
        c.elevator(),
        c.rudder(),
        c.throttle(),
        c.flaps(),
        c.brakes(),
    ]
}
fn validate_controls(c: ControlInputs, cursor: u32) -> Result<(), ReplayError> {
    let mut values = [0.; 7];
    values[1..].copy_from_slice(&controls_values(c));
    validate_frame_values(&values, cursor)
}
fn write_controls<W: Write>(w: &mut W, c: ControlInputs) -> Result<(), ReplayError> {
    for value in controls_values(c) {
        write_f64(w, value)?;
    }
    Ok(())
}
fn read_controls<R: Read>(r: &mut R, cursor: u32) -> Result<ControlInputs, ReplayError> {
    let mut value = [0.; 7];
    for slot in &mut value[1..] {
        *slot = read_f64(r)?;
    }
    validate_frame_values(&value, cursor)?;
    Ok(ControlInputs::new(value[1], value[2], value[3], value[4], value[5]).with_brakes(value[6]))
}
fn state_values(s: &RigidBodyState) -> [f64; 13] {
    let p = s.position.as_vec();
    [
        p.x,
        p.y,
        p.z,
        s.velocity.x,
        s.velocity.y,
        s.velocity.z,
        s.orientation.x,
        s.orientation.y,
        s.orientation.z,
        s.orientation.w,
        s.angular_velocity.x,
        s.angular_velocity.y,
        s.angular_velocity.z,
    ]
}
#[must_use]
pub fn state_bits_equal(a: &RigidBodyState, b: &RigidBodyState) -> bool {
    state_values(a).map(f64::to_bits) == state_values(b).map(f64::to_bits)
}
fn write_state<W: Write>(w: &mut W, s: &RigidBodyState) -> Result<(), ReplayError> {
    for value in state_values(s) {
        write_f64(w, value)?;
    }
    Ok(())
}
fn read_state<R: Read>(r: &mut R) -> Result<RigidBodyState, ReplayError> {
    let mut v = [0.; 13];
    for slot in &mut v {
        *slot = read_f64(r)?;
    }
    Ok(RigidBodyState {
        position: Ecef::from_vec(DVec3::new(v[0], v[1], v[2])),
        velocity: DVec3::new(v[3], v[4], v[5]),
        orientation: DQuat::from_xyzw(v[6], v[7], v[8], v[9]),
        angular_velocity: DVec3::new(v[10], v[11], v[12]),
    })
}

fn axis_code(axis: AxisStatus) -> u16 {
    match axis {
        AxisStatus::Below => 0,
        AxisStatus::Within => 1,
        AxisStatus::Above => 2,
    }
}
fn decode_axis(code: u16) -> Result<AxisStatus, ReplayError> {
    match code {
        0 => Ok(AxisStatus::Below),
        1 => Ok(AxisStatus::Within),
        2 => Ok(AxisStatus::Above),
        _ => Err(invalid("terminal axis", "requires0/1/2")),
    }
}
fn domain_code(d: JetDomainStatus) -> u16 {
    axis_code(d.pressure) | (axis_code(d.temperature) << 2) | (axis_code(d.mach) << 4)
}
fn decode_domain(code: u16) -> Result<JetDomainStatus, ReplayError> {
    require_valid(
        code & !63 == 0,
        "terminal domain",
        None,
        "reserved bits must be zero",
    )?;
    let result = JetDomainStatus {
        pressure: decode_axis(code & 3)?,
        temperature: decode_axis((code >> 2) & 3)?,
        mach: decode_axis((code >> 4) & 3)?,
    };
    require_valid(
        !result.is_supported(),
        "terminal domain",
        None,
        "outside reason needs an outside axis",
    )?;
    Ok(result)
}
fn invalid_code(code: u16) -> Result<JetInvalidInput, ReplayError> {
    Ok(match code {
        1 => JetInvalidInput::TimeStep,
        2 => JetInvalidInput::State,
        3 => JetInvalidInput::Quaternion,
        4 => JetInvalidInput::Position,
        5 => JetInvalidInput::AtmosphereOffset,
        6 => JetInvalidInput::Ground,
        7 => JetInvalidInput::Wind,
        8 => JetInvalidInput::RelativeVelocity,
        9 => JetInvalidInput::AtmosphereTemperature,
        10 => JetInvalidInput::AtmosphereDensity,
        11 => JetInvalidInput::ComponentQuery,
        12 => JetInvalidInput::Derivative,
        13 => JetInvalidInput::IntermediateState,
        14 => JetInvalidInput::AngularRate,
        _ => {
            return Err(invalid(
                "terminal invalid-input code",
                "requires a defined1..14 code",
            ));
        }
    })
}
fn reason_codes(reason: JetFailureReason) -> (u8, u16) {
    match reason {
        JetFailureReason::InvalidInput(code) => (1, code as u16),
        JetFailureReason::OutsideAtmosphereAltitude(axis) => (2, axis_code(axis)),
        JetFailureReason::OutsideOperatingEnvelope(d) => (3, domain_code(d)),
        JetFailureReason::OutsideJetDomain(d) => (4, domain_code(d)),
        JetFailureReason::OutsideMachDomain(axis) => (5, axis_code(axis)),
        JetFailureReason::SubstepBudgetExceeded => (6, 0),
    }
}
fn decode_reason(tag: u8, detail: u16) -> Result<JetFailureReason, ReplayError> {
    Ok(match tag {
        1 => JetFailureReason::InvalidInput(invalid_code(detail)?),
        2 | 5 => {
            let axis = decode_axis(detail)?;
            require_valid(
                axis != AxisStatus::Within,
                "terminal axis",
                None,
                "outside reason cannot be Within",
            )?;
            if tag == 2 {
                JetFailureReason::OutsideAtmosphereAltitude(axis)
            } else {
                JetFailureReason::OutsideMachDomain(axis)
            }
        }
        3 => JetFailureReason::OutsideOperatingEnvelope(decode_domain(detail)?),
        4 => JetFailureReason::OutsideJetDomain(decode_domain(detail)?),
        6 if detail == 0 => JetFailureReason::SubstepBudgetExceeded,
        _ => {
            return Err(invalid(
                "terminal reason/detail",
                "unknown or inconsistent reason/detail",
            ));
        }
    })
}
fn validate_failure(error: JetStepError) -> Result<(), ReplayError> {
    require_valid(
        error.substep < 8 && (error.stage != JetStage::Initial || error.substep == 0),
        "terminal substep",
        None,
        "requires0..7 and Initial substep0",
    )?;
    let (tag, detail) = reason_codes(error.reason);
    decode_reason(tag, detail)?;
    if let Some(query) = error.query {
        require_valid(
            query.pressure_ratio.0.is_finite()
                && query.pressure_ratio.0 >= 0.0
                && query.temperature_ratio.0.is_finite()
                && query.temperature_ratio.0 > 0.0
                && query.mach.0.is_finite()
                && query.mach.0 >= 0.0,
            "terminal query",
            None,
            "requires finite physical query signs",
        )?;
    }
    if matches!(
        error.reason,
        JetFailureReason::OutsideOperatingEnvelope(_)
            | JetFailureReason::OutsideJetDomain(_)
            | JetFailureReason::OutsideMachDomain(_)
    ) {
        require_valid(
            error.query.is_some(),
            "terminal query",
            None,
            "component/envelope domain failure requires query",
        )?;
    }
    if matches!(error.reason, JetFailureReason::OutsideAtmosphereAltitude(_)) {
        require_valid(
            error.query.is_none(),
            "terminal query",
            None,
            "altitude failure precedes query",
        )?;
    }
    Ok(())
}
fn write_failure<W: Write>(w: &mut W, e: JetStepError) -> Result<(), ReplayError> {
    let (tag, detail) = reason_codes(e.reason);
    w.write_all(&[tag])?;
    w.write_all(&detail.to_le_bytes())?;
    w.write_all(&[e.stage as u8])?;
    w.write_all(&e.substep.to_le_bytes())?;
    w.write_all(&[u8::from(e.query.is_some())])?;
    if let Some(q) = e.query {
        for v in [q.pressure_ratio.0, q.temperature_ratio.0, q.mach.0] {
            write_f64(w, v)?;
        }
    }
    Ok(())
}
fn read_failure<R: Read>(r: &mut R, length: u32) -> Result<JetStepError, ReplayError> {
    let tag = read_u8(r)?;
    let detail = read_u16(r)?;
    let reason = decode_reason(tag, detail)?;
    let stage = match read_u8(r)? {
        0 => JetStage::Initial,
        1 => JetStage::K1,
        2 => JetStage::K2,
        3 => JetStage::K3,
        4 => JetStage::K4,
        5 => JetStage::Endpoint,
        _ => return Err(invalid("terminal stage", "unknown stage")),
    };
    let substep = read_u32(r)?;
    let flag = read_u8(r)?;
    require_valid(
        (flag == 0 && length == 61) || (flag == 1 && length == 85),
        "terminal length/query",
        None,
        "length must match query flag",
    )?;
    let query = if flag == 1 {
        Some(JetConditions {
            pressure_ratio: PressureRatio(read_f64(r)?),
            temperature_ratio: TemperatureRatio(read_f64(r)?),
            mach: MachNumber(read_f64(r)?),
        })
    } else {
        None
    };
    let error = JetStepError {
        reason,
        substep,
        stage,
        query,
    };
    validate_failure(error)?;
    Ok(error)
}
fn failure_bits_equal(a: JetStepError, b: JetStepError) -> bool {
    let mut left = Vec::new();
    let mut right = Vec::new();
    write_failure(&mut left, a).expect("Vec write");
    write_failure(&mut right, b).expect("Vec write");
    left == right
}

fn validate_model_state(state: &RigidBodyState, cursor: u32) -> Result<(), ReplayError> {
    require_valid(
        state.is_finite()
            && state.position.as_vec().length().is_finite()
            && state.position.as_vec().length_squared() != 0.0
            && state.velocity.length().is_finite()
            && state.angular_velocity.length().is_finite(),
        "model state",
        Some(cursor),
        "requires finite nonzero position and finite vector norms",
    )?;
    require_valid(
        (state.orientation.length() - 1.0).abs() <= 1e-9,
        "model quaternion",
        Some(cursor),
        "must satisfy jet law unit tolerance1e-9 without normalization",
    )?;
    let position = state.geodetic();
    require_valid(
        position.latitude.is_finite()
            && position.longitude.is_finite()
            && position.altitude.is_finite()
            && (-5000.0..=86000.0).contains(&position.altitude.get()),
        "model altitude",
        Some(cursor),
        "must satisfy jet law geometric domain",
    )
}

/// Owns reconstruction so cursor/time/state cannot be advanced independently.
#[derive(Debug)]
pub struct JetReplayPlayer {
    recording: JetRecording,
    simulation: JetSimulation,
    cursor: u32,
    paused: bool,
    speed: f64,
    accumulator: Seconds,
    seek_target: Option<u32>,
    finished: bool,
    faulted: bool,
    interpolate: bool,
}
impl JetReplayPlayer {
    /// Validate complete identity before any reproduction. Terminal-at-zero is
    /// verified immediately, without advancing time or admitting live inputs.
    /// # Errors
    /// Invalid/mismatched identity, initial state, terminal evidence or environment.
    pub fn new(config: JetAircraftConfig, recording: JetRecording) -> Result<Self, ReplayError> {
        recording.validate()?;
        require_valid(
            recording.conditions.identity == ModelIdentity::for_jet(&config),
            "jet model identity",
            None,
            "must exactly match complete recorded identity",
        )?;
        let simulation = JetSimulation::from_state(
            config,
            recording.conditions.initial_state,
            recording.conditions.environment,
        )
        .map_err(|e| mismatch(e.to_string()))?;
        let mut player = Self {
            recording,
            simulation,
            cursor: 0,
            paused: false,
            speed: 1.0,
            accumulator: Seconds::ZERO,
            seek_target: None,
            finished: false,
            faulted: false,
            interpolate: false,
        };
        if player.recording.count() == 0 {
            player.settle_end()?;
        }
        Ok(player)
    }
    #[must_use]
    pub const fn simulation(&self) -> &JetSimulation {
        &self.simulation
    }
    #[must_use]
    pub const fn recording(&self) -> &JetRecording {
        &self.recording
    }
    /// Last committed effective input. Frame zero has no committed input,
    /// including a rejected frame-zero terminal, so it returns neutral controls.
    #[must_use]
    pub fn last_controls(&self) -> ControlInputs {
        self.cursor
            .checked_sub(1)
            .and_then(|cursor| self.recording.controls.get(cursor as usize))
            .copied()
            .unwrap_or_else(ControlInputs::neutral)
    }
    #[must_use]
    pub const fn speed(&self) -> f64 {
        self.speed
    }
    /// Dimensionless playback rate, clamped to the existing replay range.
    /// NaN resets to 1x; infinities select their corresponding bound.
    pub const fn set_speed(&mut self, speed: f64) {
        self.speed = if speed.is_nan() {
            1.0
        } else {
            speed.clamp(replay::MIN_SPEED, replay::MAX_SPEED)
        };
    }
    #[must_use]
    pub const fn seek_target(&self) -> Option<u32> {
        self.seek_target
    }
    /// Interpolation comes from playback's own fraction, never the unused live
    /// fixed-step accumulator. Stopped/seek poses show the last committed state.
    #[must_use]
    pub fn interpolated(&self) -> crate::InterpolatedState {
        let alpha = if !self.interpolate
            || self.paused
            || self.finished
            || self.faulted
            || self.seeking()
            || self.cursor == self.recording.count()
        {
            1.0
        } else {
            (self.accumulator.get() / JET_FIXED_DT.get()).clamp(0.0, 1.0)
        };
        self.simulation.interpolated_with_alpha(alpha)
    }
    #[must_use]
    pub fn presentation(&self) -> JetPresentationSnapshot {
        self.simulation.presentation_with_pose(self.interpolated())
    }
    #[must_use]
    pub const fn cursor(&self) -> u32 {
        self.cursor
    }
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.finished
    }
    #[must_use]
    pub const fn faulted(&self) -> bool {
        self.faulted
    }
    #[must_use]
    pub const fn seeking(&self) -> bool {
        self.seek_target.is_some()
    }
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused
    }
    pub const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.interpolate = false;
        }
    }
    /// At most 240 actual attempts per call. Nonfinite/negative time is rejected;
    /// one call admits at most 0.25 real seconds, scaled by bounded speed, and
    /// never accumulates paused/final time. Terminal work shares the same cap.
    /// # Errors
    /// Divergent checkpoints, unexpected success/rejection or invalid time.
    pub fn advance(&mut self, frame_time: Seconds) -> Result<u32, ReplayError> {
        if self.faulted {
            return Err(mismatch(
                "replay is faulted; restart or seek to reconstruct",
            ));
        }
        if self.paused || self.finished {
            return Ok(0);
        }
        if self.seek_target.is_some() {
            return self.continue_seek();
        }
        require_valid(
            frame_time.is_finite() && frame_time.get() >= 0.0,
            "playback frame time",
            None,
            "must be finite and nonnegative",
        )?;
        self.accumulator += Seconds(frame_time.get().min(0.25) * self.speed);
        let result = self.advance_due();
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn advance_due(&mut self) -> Result<u32, ReplayError> {
        let mut work = 0;
        while work < MAX_REPLAY_WORK && !self.finished {
            if self.cursor == self.recording.count() {
                self.settle_end()?;
                work += u32::from(self.recording.terminal.is_some());
                break;
            }
            let ratio = self.accumulator.get() / JET_FIXED_DT.get();
            if ratio + 64.0 * f64::EPSILON * ratio.abs().max(1.0) < 1.0 {
                break;
            }
            self.step_success()?;
            self.interpolate = true;
            self.accumulator = Seconds((self.accumulator.get() - JET_FIXED_DT.get()).max(0.0));
            work += 1;
        }
        Ok(work)
    }
    fn step_success(&mut self) -> Result<(), ReplayError> {
        let control = self.recording.controls[self.cursor as usize];
        self.simulation.attempt_step(control).map_err(|event| {
            mismatch(format!(
                "unexpected jet rejection at {}: {}",
                self.cursor, event.failure
            ))
        })?;
        self.cursor += 1;
        self.verify_checkpoint()
    }
    fn verify_checkpoint(&self) -> Result<(), ReplayError> {
        let key = if self.cursor == self.recording.count() {
            self.recording.checkpoints.last()
        } else if self.cursor > 0 && self.cursor % CHECKPOINT_INTERVAL == 0 {
            self.recording
                .checkpoints
                .get((self.cursor / CHECKPOINT_INTERVAL - 1) as usize)
        } else {
            None
        };
        if let Some(key) = key {
            require_valid(
                state_bits_equal(&key.state, self.simulation.state()),
                "jet replay state drift",
                Some(self.cursor),
                "all 104 state bytes must equal the checkpoint",
            )?;
        }
        Ok(())
    }
    fn settle_end(&mut self) -> Result<(), ReplayError> {
        self.verify_checkpoint()?;
        if let Some(expected) = self.recording.terminal {
            let before = self.simulation.snapshot();
            let actual = self
                .simulation
                .attempt_expected_rejection(expected.controls);
            let Err(actual) = actual else {
                return Err(mismatch("recorded terminal attempt unexpectedly succeeded"));
            };
            require_valid(
                actual.cursor == expected.cursor
                    && failure_bits_equal(actual.failure, expected.failure),
                "jet terminal mismatch",
                Some(self.cursor),
                "reason/substep/stage/query bits must exactly match",
            )?;
            require_valid(
                snapshot_bits_equal(&before, &self.simulation.snapshot()),
                "jet rollback",
                Some(self.cursor),
                "rejection must preserve all committed state",
            )?;
        }
        self.finished = true;
        self.accumulator = Seconds::ZERO;
        Ok(())
    }
    /// Reconstruct from zero, preserving the pause preference. Explicit seeking
    /// performs work even when paused; continue_seek resumes a bounded seek.
    /// # Errors
    /// Invalid target or replay reproduction failure.
    pub fn seek_to(&mut self, target: u32) -> Result<u32, ReplayError> {
        require_valid(
            target <= self.recording.count(),
            "seek target",
            None,
            "must be within committed cursor range",
        )?;
        self.simulation = JetSimulation::from_state(
            self.simulation.config().clone(),
            self.recording.conditions.initial_state,
            self.recording.conditions.environment,
        )
        .map_err(|e| mismatch(e.to_string()))?;
        self.cursor = 0;
        self.accumulator = Seconds::ZERO;
        self.finished = false;
        self.faulted = false;
        self.interpolate = false;
        self.seek_target = Some(target);
        self.continue_seek()
    }
    /// # Errors
    /// Reproduction failure. At most 240 attempts, including a terminal, per call.
    pub fn continue_seek(&mut self) -> Result<u32, ReplayError> {
        if self.faulted {
            return Err(mismatch("replay is faulted; restart seek"));
        }
        let result = self.seek_work();
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn seek_work(&mut self) -> Result<u32, ReplayError> {
        let Some(target) = self.seek_target else {
            return Ok(0);
        };
        let mut work = 0;
        while self.cursor < target && work < MAX_REPLAY_WORK {
            self.step_success()?;
            work += 1;
        }
        if self.cursor == target {
            if target == self.recording.count() {
                if self.recording.terminal.is_some() && work == MAX_REPLAY_WORK {
                    return Ok(work);
                }
                self.settle_end()?;
                work += u32::from(self.recording.terminal.is_some());
            }
            self.seek_target = None;
        }
        Ok(work)
    }
    /// # Errors
    /// Initial/terminal-at-zero reproduction failure.
    pub fn restart(&mut self) -> Result<(), ReplayError> {
        self.seek_to(0).map(|_| ())
    }
}

/// Exact scalar-bit comparison, excluding the intentional terminal latch and
/// discarded frame fraction. Includes both interpolation endpoints and history.
fn snapshot_bits_equal(
    a: &crate::model_simulation::JetSnapshot,
    b: &crate::model_simulation::JetSnapshot,
) -> bool {
    fn scalars(s: &crate::model_simulation::JetSnapshot) -> Vec<u64> {
        let mut values = Vec::from(state_values(&s.state));
        values.extend(state_values(&s.previous));
        values.extend(s.gear_clearances.map(Meters::get));
        values.extend([
            s.elapsed.get(),
            s.ground.reference.latitude.get(),
            s.ground.reference.longitude.get(),
            s.ground.reference.altitude.get(),
            s.ground.elevation.get(),
            s.ground.slope.north(),
            s.ground.slope.east(),
            s.log.airborne_time.get(),
            s.log.distance.get(),
            s.log.peak_agl.get(),
            s.log.peak_airspeed.get(),
        ]);
        if let Some(t) = s.last_touchdown {
            values.extend([
                t.position.latitude.get(),
                t.position.longitude.get(),
                t.position.altitude.get(),
                t.sink_rate.get(),
                t.ground_speed.get(),
                t.bank.get(),
                t.heading.get(),
                t.elapsed.get(),
            ]);
        }
        values.into_iter().map(f64::to_bits).collect()
    }
    a.ground.from_terrain == b.ground.from_terrain
        && a.log.landings == b.log.landings
        && a.airborne == b.airborne
        && a.last_touchdown.is_some() == b.last_touchdown.is_some()
        && a.touchdown_count == b.touchdown_count
        && a.committed_steps == b.committed_steps
        && scalars(a) == scalars(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn zero_recording() -> JetRecording {
        JetRecording::read_from(
            &mut include_bytes!("../tests/fixtures/v4_zero.fsreplay").as_slice(),
        )
        .unwrap()
    }
    #[test]
    fn recorder_capacity_closes_before_any_append_or_later_terminal() {
        let mut recording = zero_recording();
        let state = recording.conditions.initial_state;
        recording.controls = vec![ControlInputs::neutral(); MAX_FRAMES as usize];
        recording.checkpoints.clear();
        let mut recorder = JetRecorder {
            recording,
            last: state,
            closed: false,
        };
        let report = JetAdvance {
            committed: vec![crate::model_simulation::JetCommittedStep {
                cursor: MAX_FRAMES + 1,
                controls: ControlInputs::neutral(),
                before: state,
                after: state,
            }],
            terminal: None,
        };
        assert!(recorder.record(&report).is_err());
        assert!(recorder.closed());
        assert_eq!(recorder.recording.count(), MAX_FRAMES);
        assert!(state_bits_equal(&recorder.last, &state));
        let terminal = JetAdvance {
            committed: Vec::new(),
            terminal: Some(JetTerminalEvent {
                cursor: MAX_FRAMES,
                controls: ControlInputs::neutral(),
                failure: JetStepError {
                    reason: JetFailureReason::SubstepBudgetExceeded,
                    substep: 0,
                    stage: JetStage::Initial,
                    query: None,
                },
            }),
        };
        assert!(recorder.record(&terminal).is_err());
        assert!(recorder.recording.terminal.is_none());
    }
    #[test]
    fn every_gear_clearance_signed_zero_is_in_exact_snapshot_projection() {
        let profile = crate::aircraft_profile::AircraftProfileV2::parse(include_str!(
            "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
        ))
        .unwrap();
        let sim = JetSimulation::from_state(
            profile.configuration().clone(),
            zero_recording().conditions.initial_state,
            JetEnvironment::default(),
        )
        .unwrap();
        let mut before = sim.snapshot();
        before.gear_clearances = [Meters::ZERO; 3];
        for index in 0..3 {
            let mut changed = before;
            changed.gear_clearances[index] = Meters(-0.0);
            assert!(
                !snapshot_bits_equal(&before, &changed),
                "omitted clearance {index}"
            );
        }
    }
    #[test]
    fn unknown_terminal_codes_and_reserved_bits_reject() {
        let base = include_bytes!("../tests/fixtures/v4_terminal_zero.fsreplay");
        let start = base.len() - 85;
        for (offset, value) in [
            (52, 0),
            (52, 7),
            (53, 0xff),
            (54, 0x80),
            (55, 6),
            (56, 8),
            (60, 2),
        ] {
            let mut bytes = base.to_vec();
            bytes[start + offset] = value;
            assert!(
                JetRecording::read_from(&mut bytes.as_slice()).is_err(),
                "offset {offset}"
            );
        }
        for code in 1..=14 {
            assert!(invalid_code(code).is_ok());
        }
        for code in [0, 15, u16::MAX] {
            assert!(invalid_code(code).is_err());
        }
    }
}
