//! Bounded, explicitly model-aware replay v5 for complete running-turboprop state. All v1-v4 paths remain unchanged.
//! See docs/replay-v5.md for independently encoded field order and semantics.
use crate::model_identity::ModelIdentity;
use crate::replay::current::{
    read_environment, read_header, read_weather, weather_length, write_environment, write_weather,
};
use crate::replay::{self, MAGIC, MAX_FRAMES, MAX_NAME_BYTES, ReplayError};
use crate::replay::{
    read_f64, read_u16, read_u32, read_u64, require_valid, validate_frame_values,
    validate_visual_time, write_f64,
};
use crate::turboprop_simulation::{
    TURBOPROP_FIXED_DT, TURBOPROP_SIMULATION_REVISION, TurbopropAdvance, TurbopropEnvironment,
    TurbopropReportOrigin, TurbopropSimulation, TurbopropTerminalEvent, TurbopropTerrain,
};
use crate::weather::WeatherSelection;
use flightsim_core::{Ecef, Meters, Radians, RadiansPerSecond, Seconds};
use flightsim_fdm::turboprop::{
    AdvanceRatio, AxisStatus, MachNumber, PowerDomainStatus, PressureRatio, PropellerDomainStatus,
    PropellerPowerBound, TemperatureRatio, TurbineFraction, TurbopropAircraftConfig,
    TurbopropDiagnosticValues, TurbopropDiagnostics, TurbopropDomainStatus, TurbopropFailureReason,
    TurbopropInvalidInput, TurbopropStage, TurbopropState, TurbopropStepError,
};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use glam::{DQuat, DVec3};
use std::io::{Read, Write};

pub const TURBOPROP_FORMAT_VERSION: u16 = 5;
pub const MAX_TURBOPROP_CONDITIONS_BYTES: u32 = 4096;
pub const MAX_TURBOPROP_CHECKPOINTS: u32 = 8334;
pub const MAX_REPLAY_WORK: u32 = 240;
/// Wire bound with the current256-byte name and120-byte weather limits.
pub const MAX_TURBOPROP_RECORDING_BYTES: u64 = 49_100_903;
const CONDITIONS_FIXED_BYTES: u32 = 279;
const CHECKPOINT_INTERVAL: u32 = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct TurbopropConditionsRecord {
    pub aircraft_name: String,
    pub identity: ModelIdentity,
    pub simulation_revision: u32,
    pub environment: TurbopropEnvironment,
    pub initial_state: TurbopropState,
}
impl TurbopropConditionsRecord {
    fn validate(&self) -> Result<(), ReplayError> {
        bound("aircraft name", self.aircraft_name.len(), MAX_NAME_BYTES)?;
        require_valid(
            self.identity.supported_turboprop(),
            "model identity",
            None,
            "requires algorithm1/schema3/kind3/law1",
        )?;
        require_valid(
            self.simulation_revision == TURBOPROP_SIMULATION_REVISION,
            "simulation law",
            None,
            "requires turboprop simulation law1",
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
pub struct TurbopropCheckpoint {
    pub frame: u32,
    pub state: TurbopropState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurbopropRecording {
    conditions: TurbopropConditionsRecord,
    controls: Vec<ControlInputs>,
    checkpoints: Vec<TurbopropCheckpoint>,
    terminal: Option<TurbopropTerminalEvent>,
}
impl TurbopropRecording {
    #[must_use]
    pub const fn conditions(&self) -> &TurbopropConditionsRecord {
        &self.conditions
    }
    #[must_use]
    pub fn controls(&self) -> &[ControlInputs] {
        &self.controls
    }
    #[must_use]
    pub fn checkpoints(&self) -> &[TurbopropCheckpoint] {
        &self.checkpoints
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<TurbopropTerminalEvent> {
        self.terminal
    }
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.controls
            .iter()
            .fold(Seconds::ZERO, |sum, _| sum + TURBOPROP_FIXED_DT)
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
            MAX_TURBOPROP_CHECKPOINTS,
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
        writer.write_all(&TURBOPROP_FORMAT_VERSION.to_le_bytes())?;
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
            TurbopropTerrain::Flat { elevation } => {
                writer.write_all(&[0])?;
                write_f64(writer, elevation.get())?;
            }
            TurbopropTerrain::BundledGlobal => {
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
        if version != TURBOPROP_FORMAT_VERSION {
            return Err(ReplayError::UnsupportedVersion {
                found: version,
                expected: TURBOPROP_FORMAT_VERSION,
            });
        }
        Self::read_body(reader)
    }
    fn read_body<R: Read>(reader: &mut R) -> Result<Self, ReplayError> {
        let length = read_u32(reader)?;
        bound(
            "model conditions bytes",
            length as usize,
            MAX_TURBOPROP_CONDITIONS_BYTES,
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
            identity.supported_turboprop(),
            "model identity",
            None,
            "requires supported schema3 turboprop identity",
        )?;
        let simulation_revision = read_u32(&mut block)?;
        require_valid(
            simulation_revision == TURBOPROP_SIMULATION_REVISION,
            "simulation law",
            None,
            "unsupported",
        )?;
        let conditions = read_environment(&mut block)?;
        let terrain_kind = read_u8(&mut block)?;
        let elevation = read_f64(&mut block)?;
        let terrain = match terrain_kind {
            0 => TurbopropTerrain::Flat {
                elevation: Meters(elevation),
            },
            1 if elevation.to_bits() == 0 => TurbopropTerrain::BundledGlobal,
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
        let conditions = TurbopropConditionsRecord {
            aircraft_name,
            identity,
            simulation_revision,
            environment: TurbopropEnvironment {
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
        bound("checkpoints", k as usize, MAX_TURBOPROP_CHECKPOINTS)?;
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
            checkpoints.push(TurbopropCheckpoint { frame, state });
        }
        let terminal_length = read_u32(reader)?;
        let terminal = if terminal_length == 0 {
            None
        } else {
            require_valid(
                (62..=134).contains(&terminal_length) && (terminal_length - 62) % 8 == 0,
                "terminal length",
                None,
                "must be zero or 62+8*n for n in0..9",
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
            Some(TurbopropTerminalEvent {
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
pub struct TurbopropRecorder {
    origin: TurbopropReportOrigin,
    recording: TurbopropRecording,
    last: TurbopropState,
    elapsed: Seconds,
    closed: bool,
}
impl TurbopropRecorder {
    /// # Errors
    /// A recording must begin with a pristine, nonterminal simulation.
    pub fn new(sim: &TurbopropSimulation) -> Result<Self, ReplayError> {
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
        let conditions = TurbopropConditionsRecord {
            aircraft_name: sim.config().airframe().name().to_owned(),
            identity: ModelIdentity::for_turboprop(sim.config()),
            simulation_revision: TURBOPROP_SIMULATION_REVISION,
            environment,
            initial_state: *sim.state(),
        };
        conditions.validate()?;
        Ok(Self {
            origin: sim.report_origin().clone(),
            recording: TurbopropRecording {
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
    pub fn record(&mut self, report: &TurbopropAdvance) -> Result<(), ReplayError> {
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
                self.recording.checkpoints.push(TurbopropCheckpoint {
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
    fn validate_report(&self, report: &TurbopropAdvance) -> Result<Seconds, ReplayError> {
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
            elapsed += TURBOPROP_FIXED_DT;
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
    pub fn export(&self) -> TurbopropRecording {
        let mut recording = self.recording.clone();
        Self::append_final_checkpoint(&mut recording, self.last);
        recording
    }
    #[must_use]
    pub fn finish(mut self) -> TurbopropRecording {
        Self::append_final_checkpoint(&mut self.recording, self.last);
        self.recording
    }
    fn append_final_checkpoint(recording: &mut TurbopropRecording, last: TurbopropState) {
        let n = recording.count();
        if recording
            .checkpoints
            .last()
            .is_none_or(|key| key.frame != n)
        {
            recording.checkpoints.push(TurbopropCheckpoint {
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
fn axes_code(axes: &[AxisStatus]) -> u16 {
    axes.iter()
        .enumerate()
        .fold(0, |bits, (i, axis)| bits | (axis_code(*axis) << (2 * i)))
}
fn decode_axes<const N: usize>(code: u16) -> Result<[AxisStatus; N], ReplayError> {
    require_valid(
        code >> (2 * N) == 0,
        "terminal domain",
        None,
        "reserved bits must be zero",
    )?;
    let mut axes = [AxisStatus::Within; N];
    for (i, axis) in axes.iter_mut().enumerate() {
        *axis = decode_axis((code >> (2 * i)) & 3)?;
    }
    require_valid(
        axes.iter().any(|v| *v != AxisStatus::Within),
        "terminal domain",
        None,
        "requires an outside axis",
    )?;
    Ok(axes)
}
fn invalid_code(code: u16) -> Result<TurbopropInvalidInput, ReplayError> {
    use TurbopropInvalidInput as I;
    Ok(match code {
        1 => I::TimeStep,
        2 => I::State,
        3 => I::Quaternion,
        4 => I::Position,
        5 => I::AtmosphereOffset,
        6 => I::Ground,
        7 => I::Wind,
        8 => I::RelativeVelocity,
        9 => I::AtmosphereTemperature,
        10 => I::AtmosphereDensity,
        11 => I::TurbineFraction,
        12 => I::ShaftRate,
        13 => I::BladePitch,
        14 => I::ComponentQuery,
        15 => I::Derivative,
        16 => I::IntermediateState,
        _ => {
            return Err(invalid(
                "terminal invalid-input code",
                "requires a defined1..16 code",
            ));
        }
    })
}
fn reason_codes(reason: TurbopropFailureReason) -> (u8, u16) {
    use TurbopropFailureReason as R;
    match reason {
        R::InvalidInput(code) => (1, code as u16),
        R::OutsideAtmosphereAltitude(axis) => (2, axis_code(axis)),
        R::OutsideOperatingEnvelope(d) => (
            3,
            axes_code(&[
                d.pressure,
                d.temperature,
                d.mach,
                d.relative_shaft,
                d.absolute_spin,
                d.tip_mach,
                d.crossflow,
            ]),
        ),
        R::OutsidePowerDomain(d) => (4, axes_code(&[d.pressure, d.temperature])),
        R::OutsidePropellerDomain(d) => (5, axes_code(&[d.advance_ratio, d.blade_pitch])),
        R::OutsideMachDomain(axis) => (6, axis_code(axis)),
        R::SubstepBudgetExceeded => (7, 0),
        R::PropellerPowerBound(detail) => (8, detail as u16),
    }
}
fn decode_reason(tag: u8, detail: u16) -> Result<TurbopropFailureReason, ReplayError> {
    use TurbopropFailureReason as R;
    Ok(match tag {
        1 => R::InvalidInput(invalid_code(detail)?),
        2 | 6 => {
            let [axis] = decode_axes(detail)?;
            if tag == 2 {
                R::OutsideAtmosphereAltitude(axis)
            } else {
                R::OutsideMachDomain(axis)
            }
        }
        3 => {
            let [
                pressure,
                temperature,
                mach,
                relative_shaft,
                absolute_spin,
                tip_mach,
                crossflow,
            ] = decode_axes(detail)?;
            require_valid(
                tip_mach != AxisStatus::Below && crossflow != AxisStatus::Below,
                "terminal upper-only domain",
                None,
                "tip/crossflow lower bound is inclusive zero",
            )?;
            R::OutsideOperatingEnvelope(TurbopropDomainStatus {
                pressure,
                temperature,
                mach,
                relative_shaft,
                absolute_spin,
                tip_mach,
                crossflow,
            })
        }
        4 => {
            let [pressure, temperature] = decode_axes(detail)?;
            R::OutsidePowerDomain(PowerDomainStatus {
                pressure,
                temperature,
            })
        }
        5 => {
            let [advance_ratio, blade_pitch] = decode_axes(detail)?;
            R::OutsidePropellerDomain(PropellerDomainStatus {
                advance_ratio,
                blade_pitch,
            })
        }
        7 if detail == 0 => R::SubstepBudgetExceeded,
        8 => R::PropellerPowerBound(match detail {
            1 => PropellerPowerBound::NonpositivePower,
            2 => PropellerPowerBound::BelowIdealDisk,
            3 => PropellerPowerBound::InvalidDerivedBound,
            _ => return Err(invalid("terminal power-bound detail", "requires1..3")),
        }),
        _ => {
            return Err(invalid(
                "terminal reason/detail",
                "unknown or inconsistent reason/detail",
            ));
        }
    })
}
fn diagnostic_values(d: TurbopropDiagnostics) -> [Option<f64>; 9] {
    let d = d.values();
    [
        d.pressure_ratio.map(|v| v.0),
        d.temperature_ratio.map(|v| v.0),
        d.mach.map(|v| v.0),
        d.advance_ratio.map(|v| v.0),
        d.blade_pitch.map(Radians::get),
        d.relative_shaft.map(RadiansPerSecond::get),
        d.absolute_spin.map(RadiansPerSecond::get),
        d.tip_mach.map(|v| v.0),
        d.crossflow_ratio,
    ]
}
fn diagnostic_mask(d: TurbopropDiagnostics) -> u16 {
    diagnostic_values(d)
        .iter()
        .enumerate()
        .fold(0, |mask, (i, v)| mask | (u16::from(v.is_some()) << i))
}
fn terminal_length(error: TurbopropStepError) -> u32 {
    62 + 8 * diagnostic_mask(error.diagnostics).count_ones()
}
fn validate_failure(error: TurbopropStepError) -> Result<(), ReplayError> {
    use TurbopropFailureReason as R;
    use TurbopropInvalidInput as I;
    require_valid(
        error.substep < 8 && (error.stage != TurbopropStage::Initial || error.substep == 0),
        "terminal substep",
        None,
        "requires0..7 and Initial substep0",
    )?;
    let (tag, detail) = reason_codes(error.reason);
    decode_reason(tag, detail)?;
    let mask = diagnostic_mask(error.diagnostics);
    // These are the only finite diagnostic groups established by FDM law1.
    let valid_mask = match error.reason {
        R::OutsideAtmosphereAltitude(_) => mask == 0,
        R::OutsideOperatingEnvelope(d) => {
            let base_outside = [
                d.pressure,
                d.temperature,
                d.mach,
                d.relative_shaft,
                d.absolute_spin,
            ]
            .iter()
            .any(|a| *a != AxisStatus::Within);
            if base_outside {
                mask == 0x77
                    && d.tip_mach == AxisStatus::Within
                    && d.crossflow == AxisStatus::Within
            } else {
                mask == 0x1f7
            }
        }
        R::OutsidePowerDomain(_) => mask == 0x1f7,
        R::OutsidePropellerDomain(_) | R::OutsideMachDomain(_) | R::PropellerPowerBound(_) => {
            mask == 0x1ff
        }
        R::SubstepBudgetExceeded => {
            mask == 0x1ff || (mask == 0 && error.stage == TurbopropStage::Initial)
        }
        R::InvalidInput(code) => match code {
            I::TimeStep
            | I::State
            | I::Quaternion
            | I::Position
            | I::TurbineFraction
            | I::ShaftRate
            | I::BladePitch
            | I::IntermediateState => mask == 0,
            I::AtmosphereOffset
            | I::Wind
            | I::RelativeVelocity
            | I::AtmosphereTemperature
            | I::AtmosphereDensity => mask == 0x30,
            I::Ground => mask == 0x30 || (mask == 0x1ff && error.stage == TurbopropStage::Endpoint),
            I::ComponentQuery => matches!(mask, 0 | 0x30 | 0x77 | 0x1f7 | 0x1ff),
            I::Derivative => mask == 0x1ff,
        },
    };
    require_valid(
        valid_mask,
        "terminal diagnostic mask",
        None,
        "must match the reason and law1 evaluation order",
    )?;
    for (i, value) in diagnostic_values(error.diagnostics).into_iter().enumerate() {
        if let Some(value) = value {
            let sign_valid = match i {
                0 | 2 | 7 | 8 => value >= 0.0,
                1 | 5 => value > 0.0,
                4 => (0.0..=std::f64::consts::FRAC_PI_2).contains(&value),
                _ => true, // J may be reverse-flow; absolute spin may be nonpositive.
            };
            require_valid(
                value.is_finite() && sign_valid,
                "terminal diagnostic",
                None,
                "requires finite validated physical values",
            )?;
        }
    }
    Ok(())
}
fn write_failure<W: Write>(w: &mut W, e: TurbopropStepError) -> Result<(), ReplayError> {
    let (tag, detail) = reason_codes(e.reason);
    w.write_all(&[tag])?;
    w.write_all(&detail.to_le_bytes())?;
    w.write_all(&[e.stage as u8])?;
    w.write_all(&e.substep.to_le_bytes())?;
    w.write_all(&diagnostic_mask(e.diagnostics).to_le_bytes())?;
    for value in diagnostic_values(e.diagnostics).into_iter().flatten() {
        write_f64(w, value)?;
    }
    Ok(())
}
fn read_failure<R: Read>(r: &mut R, length: u32) -> Result<TurbopropStepError, ReplayError> {
    let tag = read_u8(r)?;
    let detail = read_u16(r)?;
    let reason = decode_reason(tag, detail)?;
    let stage = match read_u8(r)? {
        0 => TurbopropStage::Initial,
        1 => TurbopropStage::K1,
        2 => TurbopropStage::K2,
        3 => TurbopropStage::K3,
        4 => TurbopropStage::K4,
        5 => TurbopropStage::Endpoint,
        _ => return Err(invalid("terminal stage", "unknown stage")),
    };
    let substep = read_u32(r)?;
    let mask = read_u16(r)?;
    require_valid(
        mask & !0x1ff == 0 && length == 62 + 8 * mask.count_ones(),
        "terminal length/mask",
        None,
        "length must equal62+8*popcount with only bits0..8",
    )?;
    let mut v = [None; 9];
    for (i, slot) in v.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *slot = Some(read_f64(r)?);
        }
    }
    let diagnostics = TurbopropDiagnostics::from_values(TurbopropDiagnosticValues {
        pressure_ratio: v[0].map(PressureRatio),
        temperature_ratio: v[1].map(TemperatureRatio),
        mach: v[2].map(MachNumber),
        advance_ratio: v[3].map(AdvanceRatio),
        blade_pitch: v[4].map(Radians),
        relative_shaft: v[5].map(RadiansPerSecond),
        absolute_spin: v[6].map(RadiansPerSecond),
        tip_mach: v[7].map(MachNumber),
        crossflow_ratio: v[8],
    })
    .map_err(|_| invalid("terminal diagnostic", "requires finite values"))?;
    let error = TurbopropStepError {
        reason,
        substep,
        stage,
        diagnostics,
    };
    validate_failure(error)?;
    Ok(error)
}
fn failure_bits_equal(a: TurbopropStepError, b: TurbopropStepError) -> bool {
    reason_codes(a.reason) == reason_codes(b.reason)
        && a.stage == b.stage
        && a.substep == b.substep
        && diagnostic_values(a.diagnostics).map(|v| v.map(f64::to_bits))
            == diagnostic_values(b.diagnostics).map(|v| v.map(f64::to_bits))
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
pub struct TurbopropReplayPlayer {
    recording: TurbopropRecording,
    simulation: TurbopropSimulation,
    cursor: u32,
    paused: bool,
    speed: f64,
    accumulator: Seconds,
    seek_target: Option<u32>,
    finished: bool,
    faulted: bool,
    interpolate: bool,
}
impl TurbopropReplayPlayer {
    /// Validate complete identity before any reproduction. Terminal-at-zero is
    /// verified immediately, without advancing time or admitting live inputs.
    /// # Errors
    /// Invalid/mismatched identity, initial state, terminal evidence or environment.
    pub fn new(
        config: TurbopropAircraftConfig,
        recording: TurbopropRecording,
    ) -> Result<Self, ReplayError> {
        recording.validate()?;
        require_valid(
            recording.conditions.identity == ModelIdentity::for_turboprop(&config),
            "turboprop model identity",
            None,
            "must exactly match complete recorded identity",
        )?;
        let mut simulation = TurbopropSimulation::from_state(
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
            simulation = TurbopropSimulation::from_supported_state(
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
    pub const fn simulation(&self) -> &TurbopropSimulation {
        &self.simulation
    }
    #[must_use]
    pub const fn recording(&self) -> &TurbopropRecording {
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
            (self.accumulator.get() / TURBOPROP_FIXED_DT.get()).clamp(0.0, 1.0)
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
            let ratio = self.accumulator.get() / TURBOPROP_FIXED_DT.get();
            if ratio + 64.0 * f64::EPSILON * ratio.abs().max(1.0) < 1.0 {
                break;
            }
            self.step_success()?;
            self.interpolate = true;
            self.accumulator =
                Seconds((self.accumulator.get() - TURBOPROP_FIXED_DT.get()).max(0.0));
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
            self.simulation.latch_terminal(TurbopropTerminalEvent {
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
        self.simulation = TurbopropSimulation::from_state(
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
    a: &crate::turboprop_simulation::TurbopropSnapshot,
    b: &crate::turboprop_simulation::TurbopropSnapshot,
) -> bool {
    fn scalars(s: &crate::turboprop_simulation::TurbopropSnapshot) -> Vec<u64> {
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
    fn zero() -> TurbopropRecording {
        TurbopropRecording::read_from(
            &mut include_bytes!("../tests/fixtures/v5_zero.fsreplay").as_slice(),
        )
        .unwrap()
    }
    #[test]
    fn every_engine_scalar_is_in_continuity_and_final_zero_checkpoint() {
        for field in 0..3 {
            let r = zero();
            let state = r.conditions.initial_state;
            let mut recorder = TurbopropRecorder {
                origin: TurbopropReportOrigin(std::sync::Arc::from([1_u8, 2, 3])),
                recording: TurbopropRecording {
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
            let report = TurbopropAdvance {
                origin: Some(recorder.origin.clone()),
                committed: vec![crate::turboprop_simulation::TurbopropCommittedStep {
                    cursor: 1,
                    controls: ControlInputs::neutral(),
                    before: changed,
                    after: state,
                }],
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
        let mut recorder = TurbopropRecorder {
            origin: TurbopropReportOrigin(std::sync::Arc::from([1_u8, 2, 3])),
            recording: r,
            last: state,
            elapsed: Seconds::ZERO,
            closed: false,
        };
        recorder.recording.controls = vec![ControlInputs::neutral(); MAX_FRAMES as usize];
        let report = TurbopropAdvance {
            origin: Some(recorder.origin.clone()),
            committed: vec![crate::turboprop_simulation::TurbopropCommittedStep {
                cursor: MAX_FRAMES + 1,
                controls: ControlInputs::neutral(),
                before: state,
                after: state,
            }],
            terminal: None,
            terminal_state: None,
        };
        assert!(recorder.record(&report).is_err());
        assert_eq!(recorder.recording.controls.len(), MAX_FRAMES as usize);
        assert!(recorder.closed());
        assert!(state_bits_equal(&state, &recorder.last));
    }
    #[test]
    fn invalid_input_codes_are_closed_and_all_roundtrip() {
        for code in 1..=16 {
            assert_eq!(
                reason_codes(TurbopropFailureReason::InvalidInput(
                    invalid_code(code).unwrap()
                )),
                (1, code)
            );
        }
        for code in [0, 17, u16::MAX] {
            assert!(invalid_code(code).is_err());
        }
        for (tag, detail) in [
            (2, 1),
            (3, 0x1555),
            (3, 0x9555),
            (4, 5),
            (4, 0x16),
            (5, 5),
            (5, 0x16),
            (6, 1),
            (7, 1),
            (8, 0),
            (8, 4),
            (9, 0),
        ] {
            assert!(decode_reason(tag, detail).is_err(), "{tag}/{detail}");
        }
    }
}
