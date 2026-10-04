//! Explicit replay v6 for all 16 near-static turboprop law-2 state scalars.
//! Existing v1-v5 codecs and supported model gates remain unchanged.
//! See docs/replay-v6.md for independently encoded field order and semantics.
mod terminal;
use terminal::{
    failure_bits_equal, read_failure, terminal_length, validate_failure, write_failure,
};

use crate::model_identity::ModelIdentity;
use crate::near_static_turboprop_simulation::{
    NEAR_STATIC_TURBOPROP_FIXED_DT, NEAR_STATIC_TURBOPROP_SIMULATION_REVISION,
    NearStaticTurbopropAdvance, NearStaticTurbopropEnvironment, NearStaticTurbopropReportOrigin,
    NearStaticTurbopropSimulation, NearStaticTurbopropTerminalEvent, NearStaticTurbopropTerrain,
};
use crate::replay::current::{
    read_environment, read_header, read_weather, weather_length, write_environment, write_weather,
};
use crate::replay::{self, MAGIC, MAX_FRAMES, MAX_NAME_BYTES, ReplayError};
use crate::replay::{
    read_f64, read_u16, read_u32, read_u64, require_valid, validate_frame_values,
    validate_visual_time, write_f64,
};
use crate::weather::WeatherSelection;
use flightsim_core::{Ecef, Meters, Radians, RadiansPerSecond, Seconds};
use flightsim_fdm::turboprop::near_static::{
    AdvanceRatio, AxisStatus, MachNumber, NearStaticDomainStatus, PowerDomainStatus, PressureRatio,
    PropellerDomainStatus, PropellerPowerBound, StaticPowerBound, TemperatureRatio,
    TurbineFraction, TurbopropAircraftConfig, TurbopropDiagnosticValues, TurbopropDiagnostics,
    TurbopropDomainStatus, TurbopropFailureReason, TurbopropInvalidInput, TurbopropStage,
    TurbopropState, TurbopropStepError,
};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use glam::{DQuat, DVec3};
use std::io::{Read, Write};

pub const NEAR_STATIC_TURBOPROP_FORMAT_VERSION: u16 = 6;
pub const MAX_NEAR_STATIC_TURBOPROP_CONDITIONS_BYTES: u32 = 4096;
pub const MAX_NEAR_STATIC_TURBOPROP_CHECKPOINTS: u32 = 8334;
pub const MAX_REPLAY_WORK: u32 = 240;
/// Wire bound with the current256-byte name and120-byte weather limits.
pub const MAX_NEAR_STATIC_TURBOPROP_RECORDING_BYTES: u64 = 49_100_927;
const CONDITIONS_FIXED_BYTES: u32 = 279;
const CHECKPOINT_INTERVAL: u32 = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct NearStaticTurbopropConditionsRecord {
    pub aircraft_name: String,
    pub identity: ModelIdentity,
    pub simulation_revision: u32,
    pub environment: NearStaticTurbopropEnvironment,
    pub initial_state: TurbopropState,
}
impl NearStaticTurbopropConditionsRecord {
    fn validate(&self) -> Result<(), ReplayError> {
        bound("aircraft name", self.aircraft_name.len(), MAX_NAME_BYTES)?;
        require_valid(
            identity_is_structural(self.identity),
            "model identity",
            None,
            "requires nonzero identity algorithm/schema/kind/law",
        )?;
        require_valid(
            self.simulation_revision == NEAR_STATIC_TURBOPROP_SIMULATION_REVISION,
            "simulation law",
            None,
            "requires near-static turboprop host revision 1",
        )?;
        self.environment.validate()?;
        validate_model_state(&self.initial_state, 0)?;
        require_valid(
            self.environment
                .conditions
                .start
                .to_ecef()
                .as_vec()
                .distance(self.initial_state.rigid_body.position.as_vec())
                <= 1e-6,
            "initial start position",
            None,
            "must denote actual CG within one micrometre ECEF",
        )
    }
}

/// Full physical checkpoint: evidence for deterministic reconstruction, not
/// a restorable clock/contact/controller snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearStaticTurbopropCheckpoint {
    pub frame: u32,
    pub state: TurbopropState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NearStaticTurbopropRecording {
    conditions: NearStaticTurbopropConditionsRecord,
    controls: Vec<ControlInputs>,
    checkpoints: Vec<NearStaticTurbopropCheckpoint>,
    terminal: Option<NearStaticTurbopropTerminalEvent>,
}
impl NearStaticTurbopropRecording {
    #[must_use]
    pub const fn conditions(&self) -> &NearStaticTurbopropConditionsRecord {
        &self.conditions
    }
    #[must_use]
    pub fn controls(&self) -> &[ControlInputs] {
        &self.controls
    }
    #[must_use]
    pub fn checkpoints(&self) -> &[NearStaticTurbopropCheckpoint] {
        &self.checkpoints
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<NearStaticTurbopropTerminalEvent> {
        self.terminal
    }
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.controls
            .iter()
            .fold(Seconds::ZERO, |sum, _| sum + NEAR_STATIC_TURBOPROP_FIXED_DT)
    }
    #[must_use]
    pub fn final_state(&self) -> &TurbopropState {
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
        bound(
            "checkpoints",
            self.checkpoints.len(),
            MAX_NEAR_STATIC_TURBOPROP_CHECKPOINTS,
        )?;
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
        writer.write_all(&NEAR_STATIC_TURBOPROP_FORMAT_VERSION.to_le_bytes())?;
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
            NearStaticTurbopropTerrain::Flat { elevation } => {
                writer.write_all(&[0])?;
                write_f64(writer, elevation.get())?;
            }
            NearStaticTurbopropTerrain::BundledGlobal => {
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
            writer.write_all(&terminal_length(event.failure).to_le_bytes())?;
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
        if version != NEAR_STATIC_TURBOPROP_FORMAT_VERSION {
            return Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: NEAR_STATIC_TURBOPROP_FORMAT_VERSION,
            });
        }
        Self::read_body(reader)
    }
    fn read_body<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let length = read_u32(reader)?;
        bound(
            "model conditions bytes",
            length as usize,
            MAX_NEAR_STATIC_TURBOPROP_CONDITIONS_BYTES,
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
            identity_is_structural(identity),
            "model identity",
            None,
            "requires nonzero identity algorithm/schema/kind/law",
        )?;
        let simulation_revision = read_u32(&mut block)?;
        require_valid(
            simulation_revision == NEAR_STATIC_TURBOPROP_SIMULATION_REVISION,
            "simulation law",
            None,
            "unsupported",
        )?;
        let conditions = read_environment(&mut block)?;
        let terrain_kind = read_u8(&mut block)?;
        let elevation = read_f64(&mut block)?;
        let terrain = match terrain_kind {
            0 => NearStaticTurbopropTerrain::Flat {
                elevation: Meters(elevation),
            },
            1 if elevation.to_bits() == 0 => NearStaticTurbopropTerrain::BundledGlobal,
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
        let conditions = NearStaticTurbopropConditionsRecord {
            aircraft_name,
            identity,
            simulation_revision,
            environment: NearStaticTurbopropEnvironment {
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
        bound(
            "checkpoints",
            k as usize,
            MAX_NEAR_STATIC_TURBOPROP_CHECKPOINTS,
        )?;
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
            checkpoints.push(NearStaticTurbopropCheckpoint { frame, state });
        }
        let terminal_length = read_u32(reader)?;
        let terminal = if terminal_length == 0 {
            None
        } else {
            require_valid(
                (62..=158).contains(&terminal_length) && (terminal_length - 62) % 8 == 0,
                "terminal length",
                None,
                "must be zero or 62+8*n for n in0..12",
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
            Some(NearStaticTurbopropTerminalEvent {
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

/// Captures only successful reports and their one possible terminal attempt.
/// A recording error permanently closes the recorder at its prior authentic state.
#[derive(Debug)]
pub struct NearStaticTurbopropRecorder {
    origin: NearStaticTurbopropReportOrigin,
    recording: NearStaticTurbopropRecording,
    last: TurbopropState,
    elapsed: Seconds,
    closed: bool,
}
impl NearStaticTurbopropRecorder {
    /// # Errors
    /// A recording must begin with a pristine, nonterminal simulation.
    pub fn new(sim: &NearStaticTurbopropSimulation) -> Result<Self, ReplayError> {
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
        environment.conditions.start = sim.state().rigid_body.geodetic();
        environment.conditions.heading = sim.state().rigid_body.attitude().yaw;
        let conditions = NearStaticTurbopropConditionsRecord {
            aircraft_name: sim.config().airframe().name().to_owned(),
            identity: ModelIdentity::for_near_static_turboprop(sim.config()),
            simulation_revision: NEAR_STATIC_TURBOPROP_SIMULATION_REVISION,
            environment,
            initial_state: *sim.state(),
        };
        conditions.validate()?;
        Ok(Self {
            origin: sim.report_origin().clone(),
            recording: NearStaticTurbopropRecording {
                conditions,
                controls: Vec::new(),
                checkpoints: Vec::new(),
                terminal: None,
            },
            last: *sim.state(),
            elapsed: Seconds::ZERO,
            closed: false,
        })
    }
    #[must_use]
    pub const fn closed(&self) -> bool {
        self.closed
    }
    /// # Errors
    /// Closed/capped recorder or a noncontiguous report. The recorder stays frozen.
    pub fn record(&mut self, report: &NearStaticTurbopropAdvance) -> Result<(), ReplayError> {
        let elapsed = match self.validate_report(report) {
            Ok(elapsed) => elapsed,
            Err(error) => {
                self.closed = true;
                return Err(error);
            }
        };
        for step in &report.committed {
            self.recording.controls.push(step.controls);
            self.last = step.after;
            if step.cursor % CHECKPOINT_INTERVAL == 0 {
                self.recording
                    .checkpoints
                    .push(NearStaticTurbopropCheckpoint {
                        frame: step.cursor,
                        state: step.after,
                    });
            }
        }
        self.elapsed = elapsed;
        if let Some(event) = report.terminal {
            self.recording.terminal = Some(event);
            self.closed = true;
        }
        Ok(())
    }
    fn validate_report(&self, report: &NearStaticTurbopropAdvance) -> Result<Seconds, ReplayError> {
        require_valid(!self.closed, "recorder", None, "recording is closed")?;
        require_valid(
            report.origin.as_ref() == Some(&self.origin),
            "report origin",
            None,
            "must match the exact physical model and complete recorded environment",
        )?;
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
        let mut elapsed = self.elapsed;
        for step in &report.committed {
            cursor += 1;
            require_valid(
                step.cursor == cursor && state_bits_equal(&step.before, &state),
                "recording report",
                Some(cursor),
                "must continue the exact previous state/cursor",
            )?;
            validate_controls(step.controls, cursor)?;
            validate_model_state(&step.after, cursor)?;
            state = step.after;
            elapsed += NEAR_STATIC_TURBOPROP_FIXED_DT;
        }
        if let Some(event) = report.terminal {
            require_valid(
                event.cursor == cursor
                    && report
                        .terminal_state
                        .is_some_and(|before| state_bits_equal(&before, &state)),
                "terminal continuity",
                None,
                "must follow the exact previous full state and cursor",
            )?;
            validate_controls(event.controls, event.cursor)?;
            validate_failure(event.failure)?;
        }
        validate_visual_time(
            self.recording.conditions.environment.conditions.start_epoch,
            self.recording.conditions.environment.conditions.time_rate,
            elapsed.get(),
        )?;
        Ok(elapsed)
    }
    /// Export an independently owned, valid recording through the most recent
    /// accepted report. Does not close the recorder or add temporary checkpoints
    /// to ongoing recording. Empty and frame-zero terminal exports are valid.
    #[must_use]
    pub fn export(&self) -> NearStaticTurbopropRecording {
        let mut recording = self.recording.clone();
        Self::append_final_checkpoint(&mut recording, self.last);
        recording
    }
    #[must_use]
    pub fn finish(mut self) -> NearStaticTurbopropRecording {
        Self::append_final_checkpoint(&mut self.recording, self.last);
        self.recording
    }
    fn append_final_checkpoint(recording: &mut NearStaticTurbopropRecording, last: TurbopropState) {
        let n = recording.count();
        if recording
            .checkpoints
            .last()
            .is_none_or(|key| key.frame != n)
        {
            recording.checkpoints.push(NearStaticTurbopropCheckpoint {
                frame: n,
                state: last,
            });
        }
    }
}

// Unknown nonzero allocations remain inspectable/exportable. Reproduction
// separately checks the exact supported tuple and complete configuration hash.
fn identity_is_structural(identity: ModelIdentity) -> bool {
    identity.algorithm != 0
        && identity.schema != 0
        && identity.kind != 0
        && identity.law_revision != 0
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
fn state_values(s: &TurbopropState) -> [f64; 16] {
    let r = &s.rigid_body;
    let p = r.position.as_vec();
    [
        p.x,
        p.y,
        p.z,
        r.velocity.x,
        r.velocity.y,
        r.velocity.z,
        r.orientation.x,
        r.orientation.y,
        r.orientation.z,
        r.orientation.w,
        r.angular_velocity.x,
        r.angular_velocity.y,
        r.angular_velocity.z,
        s.turbine_fraction.get(),
        s.shaft_rad_s.get(),
        s.blade_pitch_rad.get(),
    ]
}
#[must_use]
pub fn state_bits_equal(a: &TurbopropState, b: &TurbopropState) -> bool {
    state_values(a).map(f64::to_bits) == state_values(b).map(f64::to_bits)
}
fn write_state<W: Write>(w: &mut W, s: &TurbopropState) -> Result<(), ReplayError> {
    for value in state_values(s) {
        write_f64(w, value)?;
    }
    Ok(())
}
fn read_state<R: Read>(r: &mut R) -> Result<TurbopropState, ReplayError> {
    let mut v = [0.; 16];
    for slot in &mut v {
        *slot = read_f64(r)?;
    }
    Ok(TurbopropState {
        rigid_body: RigidBodyState {
            position: Ecef::from_vec(DVec3::new(v[0], v[1], v[2])),
            velocity: DVec3::new(v[3], v[4], v[5]),
            orientation: DQuat::from_xyzw(v[6], v[7], v[8], v[9]),
            angular_velocity: DVec3::new(v[10], v[11], v[12]),
        },
        turbine_fraction: TurbineFraction::new(v[13])
            .map_err(|_| invalid("turbine fraction", "requires finite0..1"))?,
        shaft_rad_s: RadiansPerSecond(v[14]),
        blade_pitch_rad: Radians(v[15]),
    })
}

fn validate_model_state(full: &TurbopropState, cursor: u32) -> Result<(), ReplayError> {
    let state = &full.rigid_body;
    require_valid(
        state.is_finite()
            && state.position.as_vec().length().is_finite()
            && state.position.as_vec().length_squared() != 0.0
            && state.velocity.length().is_finite()
            && state.angular_velocity.length().is_finite(),
        "turboprop state",
        Some(cursor),
        "requires finite nonzero position and finite vector norms",
    )?;
    require_valid(
        (state.orientation.length() - 1.0).abs() <= 1e-9,
        "turboprop quaternion",
        Some(cursor),
        "requires unit tolerance1e-9 without normalization",
    )?;
    let position = state.geodetic();
    require_valid(
        position.latitude.is_finite()
            && position.longitude.is_finite()
            && position.altitude.is_finite()
            && (-5000.0..=86000.0).contains(&position.altitude.get()),
        "turboprop altitude",
        Some(cursor),
        "requires geometric domain",
    )?;
    require_valid(
        full.turbine_fraction.get().is_finite()
            && (0.0..=1.0).contains(&full.turbine_fraction.get())
            && full.shaft_rad_s.is_finite()
            && (20.0..=1000.0).contains(&full.shaft_rad_s.get())
            && full.blade_pitch_rad.is_finite()
            && (0.0..=std::f64::consts::FRAC_PI_2).contains(&full.blade_pitch_rad.get()),
        "turboprop engine state",
        Some(cursor),
        "requires fraction0..1, shaft20..1000 rad/s and pitch0..pi/2 rad",
    )
}

/// Owns reconstruction so cursor/time/state cannot be advanced independently.
#[derive(Debug)]
pub struct NearStaticTurbopropReplayPlayer {
    recording: NearStaticTurbopropRecording,
    simulation: NearStaticTurbopropSimulation,
    cursor: u32,
    paused: bool,
    speed: f64,
    accumulator: Seconds,
    seek_target: Option<u32>,
    finished: bool,
    faulted: bool,
    interpolate: bool,
}
impl NearStaticTurbopropReplayPlayer {
    /// Validate complete identity before any reproduction. Terminal-at-zero is
    /// verified immediately, without advancing time or admitting live inputs.
    /// # Errors
    /// Invalid/mismatched identity, initial state, terminal evidence or environment.
    pub fn new(
        config: TurbopropAircraftConfig,
        recording: NearStaticTurbopropRecording,
    ) -> Result<Self, ReplayError> {
        recording.validate()?;
        require_valid(
            recording
                .conditions
                .identity
                .supported_near_static_turboprop()
                && recording.conditions.identity
                    == ModelIdentity::for_near_static_turboprop(&config),
            "turboprop model identity",
            None,
            "must exactly match complete recorded identity",
        )?;
        let mut simulation = NearStaticTurbopropSimulation::from_state(
            config,
            recording.conditions.initial_state,
            recording.conditions.environment,
        )
        .map_err(|e| mismatch(e.to_string()))?;
        if let Some(first) = recording.controls.first() {
            simulation
                .validate_next_attempt(*first)
                .map_err(|e| mismatch(e.to_string()))?;
        } else if recording.terminal.is_none() {
            simulation = NearStaticTurbopropSimulation::from_supported_state(
                simulation.config().clone(),
                recording.conditions.initial_state,
                recording.conditions.environment,
                ControlInputs::neutral(),
            )
            .map_err(|e| mismatch(e.to_string()))?;
        }
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
    pub const fn simulation(&self) -> &NearStaticTurbopropSimulation {
        &self.simulation
    }
    #[must_use]
    pub const fn recording(&self) -> &NearStaticTurbopropRecording {
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
            (self.accumulator.get() / NEAR_STATIC_TURBOPROP_FIXED_DT.get()).clamp(0.0, 1.0)
        };
        self.simulation.interpolated_with_alpha(alpha)
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
            self.accumulator = Seconds::ZERO;
            self.interpolate = false;
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
            let ratio = self.accumulator.get() / NEAR_STATIC_TURBOPROP_FIXED_DT.get();
            if ratio + 64.0 * f64::EPSILON * ratio.abs().max(1.0) < 1.0 {
                break;
            }
            self.step_success()?;
            self.interpolate = true;
            self.accumulator =
                Seconds((self.accumulator.get() - NEAR_STATIC_TURBOPROP_FIXED_DT.get()).max(0.0));
            work += 1;
        }
        Ok(work)
    }
    fn step_success(&mut self) -> Result<(), ReplayError> {
        let control = self.recording.controls[self.cursor as usize];
        let before = self.simulation.snapshot();
        self.simulation.attempt_step(control).map_err(|failure| {
            mismatch(format!(
                "unexpected turboprop rejection at {}: {}",
                self.cursor, failure
            ))
        })?;
        self.cursor += 1;
        if let Err(error) = self.verify_checkpoint() {
            self.cursor -= 1;
            self.simulation.restore_committed(before);
            return Err(error);
        }
        Ok(())
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
                "turboprop replay state drift",
                Some(self.cursor),
                "all 128 state bytes must equal the checkpoint",
            )?;
        }
        Ok(())
    }
    fn settle_end(&mut self) -> Result<(), ReplayError> {
        self.verify_checkpoint()?;
        if let Some(expected) = self.recording.terminal {
            let before = self.simulation.snapshot();
            let actual = self.simulation.probe_rejection(expected.controls);
            let Err(failure) = actual else {
                return Err(mismatch("recorded terminal attempt unexpectedly succeeded"));
            };
            require_valid(
                before.committed_steps == expected.cursor
                    && failure_bits_equal(failure, expected.failure),
                "turboprop terminal mismatch",
                Some(self.cursor),
                "reason/substep/stage/query bits must exactly match",
            )?;
            require_valid(
                snapshot_bits_equal(&before, &self.simulation.snapshot()),
                "turboprop rollback",
                Some(self.cursor),
                "rejection must preserve all committed state",
            )?;
            self.simulation
                .latch_terminal(NearStaticTurbopropTerminalEvent {
                    cursor: self.cursor,
                    controls: expected.controls,
                    failure,
                });
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
        self.simulation = NearStaticTurbopropSimulation::from_state(
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
            self.accumulator = Seconds::ZERO;
            self.interpolate = false;
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
#[must_use]
pub fn snapshot_bits_equal(
    a: &crate::near_static_turboprop_simulation::NearStaticTurbopropSnapshot,
    b: &crate::near_static_turboprop_simulation::NearStaticTurbopropSnapshot,
) -> bool {
    fn scalars(
        s: &crate::near_static_turboprop_simulation::NearStaticTurbopropSnapshot,
    ) -> Vec<u64> {
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
    fn zero() -> NearStaticTurbopropRecording {
        NearStaticTurbopropRecording::read_from(
            &mut include_bytes!("../../tests/fixtures/v6_zero.fsreplay").as_slice(),
        )
        .unwrap()
    }
    #[test]
    fn every_engine_scalar_is_in_continuity_and_final_zero_checkpoint() {
        for field in 0..3 {
            let r = zero();
            let state = r.conditions.initial_state;
            let mut recorder = NearStaticTurbopropRecorder {
                origin: NearStaticTurbopropReportOrigin(std::sync::Arc::from([1_u8, 2, 3])),
                recording: NearStaticTurbopropRecording {
                    checkpoints: Vec::new(),
                    ..r
                },
                last: state,
                elapsed: Seconds::ZERO,
                closed: false,
            };
            let mut changed = state;
            match field {
                0 => changed.turbine_fraction = TurbineFraction::new(0.).unwrap(),
                1 => {
                    changed.shaft_rad_s =
                        RadiansPerSecond(f64::from_bits(state.shaft_rad_s.get().to_bits() + 1))
                }
                _ => {
                    changed.blade_pitch_rad =
                        Radians(f64::from_bits(state.blade_pitch_rad.get().to_bits() + 1))
                }
            }
            let report = NearStaticTurbopropAdvance {
                origin: Some(recorder.origin.clone()),
                committed: vec![
                    crate::near_static_turboprop_simulation::NearStaticTurbopropCommittedStep {
                        cursor: 1,
                        controls: ControlInputs::neutral(),
                        before: changed,
                        after: state,
                    },
                ],
                terminal: None,
                terminal_state: None,
            };
            assert!(recorder.record(&report).is_err());
            assert!(recorder.closed());
            assert!(recorder.recording.controls.is_empty());
            assert!(state_bits_equal(&state, &recorder.last));
            let mut invalid = zero();
            invalid.checkpoints[0].state = changed;
            assert!(invalid.validate().is_err());
        }
    }
    #[test]
    fn capped_or_invalid_whole_report_never_appends_partial_controls() {
        let r = zero();
        let state = r.conditions.initial_state;
        let mut recorder = NearStaticTurbopropRecorder {
            origin: NearStaticTurbopropReportOrigin(std::sync::Arc::from([1_u8, 2, 3])),
            recording: r,
            last: state,
            elapsed: Seconds::ZERO,
            closed: false,
        };
        recorder.recording.controls = vec![ControlInputs::neutral(); MAX_FRAMES as usize];
        let report = NearStaticTurbopropAdvance {
            origin: Some(recorder.origin.clone()),
            committed: vec![
                crate::near_static_turboprop_simulation::NearStaticTurbopropCommittedStep {
                    cursor: MAX_FRAMES + 1,
                    controls: ControlInputs::neutral(),
                    before: state,
                    after: state,
                },
            ],
            terminal: None,
            terminal_state: None,
        };
        assert!(recorder.record(&report).is_err());
        assert_eq!(recorder.recording.controls.len(), MAX_FRAMES as usize);
        assert!(recorder.closed());
        assert!(state_bits_equal(&state, &recorder.last));
    }
}
