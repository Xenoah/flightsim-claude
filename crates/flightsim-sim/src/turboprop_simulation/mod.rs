//! Transactional simulation for the explicit turboprop model. Legacy simulation is untouched.
mod parked;

use crate::{
    FlightLog, GroundPlane, GroundSampler, Touchdown,
    replay::{EnvironmentConditions, ReplayError},
    weather::WeatherSelection,
};
use flightsim_core::{
    Attitude, Ecef, FixedStep, Geodetic, LocalFrame, Meters, MetersPerSecond, Seconds,
};
use flightsim_fdm::turboprop::{
    TurbopropAircraftConfig, TurbopropFlightDynamics, TurbopropState, TurbopropStepError,
};
use flightsim_fdm::{Atmosphere, ControlInputs, Environment, GroundSlope};
use flightsim_world::{
    GlobalClimate, MemoryTileSource, Terrain,
    global::{GlobalTerrain, GlobalTileSource},
};

/// Pins fixed time, prospective weather, terrain sampling and contact semantics.
pub const TURBOPROP_SIMULATION_REVISION: u32 = 1;
pub const TURBOPROP_FIXED_DT: Seconds = Seconds(1.0 / 120.0);

/// The only supported terrain sources; regional packages cannot enter this API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurbopropTerrain {
    Flat { elevation: Meters },
    BundledGlobal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropEnvironment {
    pub conditions: EnvironmentConditions,
    pub terrain: TurbopropTerrain,
    /// Complete authored scenario identity. Headless execution does not render it.
    pub weather: WeatherSelection,
}
impl Default for TurbopropEnvironment {
    fn default() -> Self {
        Self {
            conditions: EnvironmentConditions::default(),
            terrain: TurbopropTerrain::Flat {
                elevation: Meters::ZERO,
            },
            weather: WeatherSelection::Legacy,
        }
    }
}
impl TurbopropEnvironment {
    /// Validate structural settings. Dataset compatibility is checked separately
    /// when constructing a simulation, so unknown fingerprints remain inspectable.
    /// # Errors
    /// Invalid, inconsistent or unsupported conditions.
    pub fn validate(self) -> Result<(), ReplayError> {
        crate::replay::validate_environment(&self.conditions)?;
        let global = matches!(self.terrain, TurbopropTerrain::BundledGlobal);
        crate::replay::require_valid(
            global == self.conditions.world_terrain,
            "turboprop terrain kind",
            None,
            "must agree with global terrain flag",
        )?;
        if let TurbopropTerrain::Flat { elevation } = self.terrain {
            crate::replay::require_valid(
                elevation.is_finite() && (-5_000.0..=86_000.0).contains(&elevation.get()),
                "flat terrain elevation",
                None,
                "must be finite and in [-5000,86000] metres",
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum TurbopropSimulationError {
    Conditions(ReplayError),
    Initial(TurbopropStepError),
    World(String),
    Parked(&'static str),
}
impl std::fmt::Display for TurbopropSimulationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conditions(e) => e.fmt(f),
            Self::Initial(e) => e.fmt(f),
            Self::World(e) => f.write_str(e),
            Self::Parked(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for TurbopropSimulationError {}

/// Committed state only. A rejection may change the visible terminal latch and
/// discard fractional frame budget; every field of this snapshot stays exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropSnapshot {
    pub state: TurbopropState,
    pub previous: TurbopropState,
    pub elapsed: Seconds,
    pub ground: GroundPlane,
    /// Per-leg signed normal clearances using the same held plane as the FDM.
    pub gear_clearances: [Meters; 3],
    pub log: FlightLog,
    pub airborne: bool,
    pub last_touchdown: Option<Touchdown>,
    pub touchdown_count: u32,
    pub committed_steps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbopropTerminalEvent {
    pub cursor: u32,
    pub controls: ControlInputs,
    pub failure: TurbopropStepError,
}
#[derive(Debug, Clone, Copy)]
pub struct TurbopropCommittedStep {
    pub(crate) cursor: u32,
    pub(crate) controls: ControlInputs,
    pub(crate) before: TurbopropState,
    pub(crate) after: TurbopropState,
}
/// Exact immutable model/environment provenance. Shared by reports without
/// rebuilding canonical bytes or allocating them on every fixed step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurbopropReportOrigin(pub(crate) std::sync::Arc<[u8]>);
impl TurbopropReportOrigin {
    fn new(
        config: &TurbopropAircraftConfig,
        state: TurbopropState,
        mut environment: TurbopropEnvironment,
    ) -> Result<Self, ReplayError> {
        use crate::replay::current::{weather_length, write_environment, write_weather};
        let mut bytes = crate::turboprop_identity::canonical_turboprop_bytes(config);
        // Same canonical frame-zero metadata as the recorder header.
        environment.conditions.start = state.rigid_body.geodetic();
        environment.conditions.heading = state.rigid_body.attitude().yaw;
        write_environment(&mut bytes, &environment.conditions)?;
        match environment.terrain {
            TurbopropTerrain::Flat { elevation } => {
                bytes.push(0);
                bytes.extend(elevation.get().to_le_bytes());
            }
            TurbopropTerrain::BundledGlobal => {
                bytes.push(1);
                bytes.extend(0_f64.to_le_bytes());
            }
        }
        bytes.extend(weather_length(environment.weather).to_le_bytes());
        if let WeatherSelection::Modeled(weather) = environment.weather {
            write_weather(&mut bytes, weather)?;
        }
        Ok(Self(bytes.into()))
    }
}
#[derive(Debug, Default)]
pub struct TurbopropAdvance {
    pub(crate) origin: Option<TurbopropReportOrigin>,
    pub(crate) committed: Vec<TurbopropCommittedStep>,
    pub(crate) terminal: Option<TurbopropTerminalEvent>,
    /// Complete before-state of a rejected attempt, for recorder continuity.
    pub(crate) terminal_state: Option<TurbopropState>,
}
impl TurbopropAdvance {
    #[must_use]
    pub fn committed_steps(&self) -> usize {
        self.committed.len()
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<TurbopropTerminalEvent> {
        self.terminal
    }
    /// Successful calls plus this report's one possible rejected call.
    #[must_use]
    pub fn attempted_steps(&self) -> usize {
        self.committed.len() + usize::from(self.terminal.is_some())
    }
}

#[derive(Debug)]
pub struct TurbopropSimulation {
    origin: TurbopropReportOrigin,
    dynamics: TurbopropFlightDynamics,
    environment: TurbopropEnvironment,
    terrain: Option<Terrain<GlobalTileSource<MemoryTileSource>>>,
    climate: Option<GlobalClimate>,
    fixed: FixedStep,
    committed: TurbopropSnapshot,
    terminal: Option<TurbopropTerminalEvent>,
}
impl TurbopropSimulation {
    /// Initial CG state is authoritative. Contact initialization uses its gear
    /// clearance (>0.5 m is airborne), identically for live and replay starts.
    /// # Errors
    /// Invalid state/environment or unavailable bundled datasets.
    pub fn from_state(
        config: TurbopropAircraftConfig,
        state: TurbopropState,
        environment: TurbopropEnvironment,
    ) -> Result<Self, TurbopropSimulationError> {
        environment
            .validate()
            .map_err(TurbopropSimulationError::Conditions)?;
        environment
            .conditions
            .check_world()
            .map_err(TurbopropSimulationError::Conditions)?;
        let dynamics = TurbopropFlightDynamics::new(config, state)
            .map_err(TurbopropSimulationError::Initial)?;
        let terrain = if matches!(environment.terrain, TurbopropTerrain::BundledGlobal) {
            let global = GlobalTerrain::bundled()
                .map_err(|e| TurbopropSimulationError::World(e.to_string()))?;
            Some(Terrain::new(
                GlobalTileSource::new(MemoryTileSource::new(), global),
                8 * 1024 * 1024,
                8..=12,
            ))
        } else {
            None
        };
        let climate = environment
            .conditions
            .climate_date
            .map(|_| GlobalClimate::bundled())
            .transpose()
            .map_err(|e| TurbopropSimulationError::World(e.to_string()))?;
        let placeholder = GroundPlane {
            reference: state.rigid_body.geodetic(),
            elevation: Meters::ZERO,
            slope: GroundSlope::LEVEL,
            from_terrain: false,
        };
        let origin = TurbopropReportOrigin::new(dynamics.config(), state, environment)
            .map_err(TurbopropSimulationError::Conditions)?;
        let mut sim = Self {
            origin,
            dynamics,
            environment,
            terrain,
            climate,
            fixed: FixedStep::new(TURBOPROP_FIXED_DT),
            committed: TurbopropSnapshot {
                state,
                previous: state,
                elapsed: Seconds::ZERO,
                ground: placeholder,
                gear_clearances: [Meters::ZERO; 3],
                log: FlightLog::default(),
                airborne: false,
                last_touchdown: None,
                touchdown_count: 0,
                committed_steps: 0,
            },
            terminal: None,
        };
        sim.committed.ground = sim.sample_ground(state.rigid_body.geodetic());
        sim.committed.gear_clearances = sim
            .dynamics
            .gear_clearances(
                &state,
                &sim.environment_at(&state, sim.committed.ground, Seconds::ZERO),
            )
            .map_err(TurbopropSimulationError::Initial)?;
        sim.committed.airborne = sim.minimum_gear_clearance().get() > 0.5;
        Ok(sim)
    }
    /// Evaluate the actual start environment without advancing physical time.
    /// Use this for live flight staging; `from_state` also permits an authentic
    /// frame-zero terminal outside a narrower authored operating envelope.
    /// # Errors
    /// Invalid state/environment or an unsupported initial force query.
    pub fn from_supported_state(
        config: TurbopropAircraftConfig,
        state: TurbopropState,
        environment: TurbopropEnvironment,
        controls: ControlInputs,
    ) -> Result<Self, TurbopropSimulationError> {
        let mut sim = Self::from_state(config, state, environment)?;
        let env = sim.environment_at(&state, sim.committed.ground, Seconds::ZERO);
        sim.dynamics
            .step(Seconds::ZERO, controls, &env)
            .map_err(TurbopropSimulationError::Initial)?;
        Ok(sim)
    }
    pub(crate) fn report_origin(&self) -> &TurbopropReportOrigin {
        &self.origin
    }
    #[must_use]
    pub const fn snapshot(&self) -> TurbopropSnapshot {
        self.committed
    }
    #[must_use]
    pub const fn state(&self) -> &TurbopropState {
        &self.committed.state
    }
    #[must_use]
    pub const fn elapsed(&self) -> Seconds {
        self.committed.elapsed
    }
    #[must_use]
    pub const fn log(&self) -> FlightLog {
        self.committed.log
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<TurbopropTerminalEvent> {
        self.terminal
    }
    #[must_use]
    pub const fn environment(&self) -> TurbopropEnvironment {
        self.environment
    }
    #[must_use]
    pub const fn config(&self) -> &TurbopropAircraftConfig {
        self.dynamics.config()
    }
    #[must_use]
    pub const fn accumulated(&self) -> Seconds {
        self.fixed.accumulated()
    }

    /// Rebuild clocks, contact history, interpolation and terminal state.
    /// # Errors
    /// Invalid replacement state or environment. The old simulation is preserved.
    pub fn restart_at(&mut self, state: TurbopropState) -> Result<(), TurbopropSimulationError> {
        let replacement = Self::from_state(self.config().clone(), state, self.environment)?;
        *self = replacement;
        Ok(())
    }

    /// Prepare controls on a cloned controller and commit that clone only after
    /// an entire external fixed step succeeds. The closure must keep its mutable
    /// pilot/ramp state in that clone, not in unrelated captured state. Clone must
    /// isolate all mutable state; Rc/Arc shared mutability is not transactional.
    pub fn advance_with_controller<C: Clone>(
        &mut self,
        frame_time: Seconds,
        controller: &mut C,
        mut prepare: impl FnMut(&mut C, Seconds, &TurbopropState) -> ControlInputs,
    ) -> TurbopropAdvance {
        let mut report = TurbopropAdvance {
            origin: Some(self.origin.clone()),
            ..TurbopropAdvance::default()
        };
        if self.terminal.is_some() {
            return report;
        }
        let due = self.fixed.advance(frame_time);
        for _ in 0..due {
            let mut proposed = controller.clone();
            let controls = prepare(&mut proposed, TURBOPROP_FIXED_DT, self.state());
            match self.attempt_step(controls) {
                Ok(step) => {
                    *controller = proposed;
                    report.committed.push(step);
                }
                Err(failure) => {
                    report.terminal = Some(TurbopropTerminalEvent {
                        cursor: self.committed.committed_steps,
                        controls,
                        failure,
                    });
                    report.terminal_state = Some(*self.state());
                    break;
                }
            }
        }
        report
    }
    pub fn advance(&mut self, frame_time: Seconds, controls: ControlInputs) -> TurbopropAdvance {
        self.advance_with_controller(frame_time, &mut (), |(), _, _| controls)
    }

    /// Attempt one exact replay tick, independent of the render accumulator.
    pub(crate) fn attempt_step(
        &mut self,
        controls: ControlInputs,
    ) -> Result<TurbopropCommittedStep, TurbopropStepError> {
        if let Some(event) = self.terminal {
            return Err(event.failure);
        }
        let before = self.committed.state;
        let ground = self.sample_ground(before.rigid_body.geodetic());
        let elapsed = self.committed.elapsed + TURBOPROP_FIXED_DT;
        let environment = self.environment_at(&before, ground, elapsed);
        let outcome = match self
            .dynamics
            .step(TURBOPROP_FIXED_DT, controls, &environment)
        {
            Ok(outcome) => outcome,
            Err(failure) => {
                let event = TurbopropTerminalEvent {
                    cursor: self.committed.committed_steps,
                    controls,
                    failure,
                };
                self.terminal = Some(event);
                self.fixed.reset();
                return Err(event.failure);
            }
        };
        self.committed.previous = before;
        self.committed.state = *self.dynamics.state();
        self.committed.ground = ground;
        self.committed.gear_clearances = outcome.gear_clearances;
        self.committed.elapsed = elapsed;
        self.committed.committed_steps += 1;
        self.update_contact_and_log();
        Ok(TurbopropCommittedStep {
            cursor: self.committed.committed_steps,
            controls,
            before,
            after: self.committed.state,
        })
    }
    /// A scratch full-state FDM plus the prospective environment uses the live
    /// executed clock. Nothing in the committed host is advanced or latched.
    pub(crate) fn probe_rejection(
        &mut self,
        controls: ControlInputs,
    ) -> Result<(), TurbopropStepError> {
        let ground = self.sample_ground(self.state().rigid_body.geodetic());
        let environment =
            self.environment_at(self.state(), ground, self.elapsed() + TURBOPROP_FIXED_DT);
        let mut candidate = self.dynamics.clone();
        candidate
            .step(TURBOPROP_FIXED_DT, controls, &environment)
            .map(|_| ())
    }
    /// Validate the next recorded attempt at its prospective wind clock without
    /// advancing the committed state, time, contacts or controller history.
    pub(crate) fn validate_next_attempt(
        &mut self,
        controls: ControlInputs,
    ) -> Result<(), TurbopropStepError> {
        let ground = self.sample_ground(self.state().rigid_body.geodetic());
        let environment =
            self.environment_at(self.state(), ground, self.elapsed() + TURBOPROP_FIXED_DT);
        self.dynamics
            .clone()
            .step(Seconds::ZERO, controls, &environment)
            .map(|_| ())
    }
    pub(crate) fn latch_terminal(&mut self, event: TurbopropTerminalEvent) {
        self.terminal = Some(event);
        self.fixed.reset();
    }
    /// Roll back a successful numerical attempt whose checkpoint is corrupt.
    /// The immutable terrain cache may contain reads but has no time/history.
    pub(crate) fn restore_committed(&mut self, snapshot: TurbopropSnapshot) {
        self.dynamics
            .set_state(snapshot.state)
            .expect("previously accepted state");
        self.committed = snapshot;
        self.fixed.reset();
        self.terminal = None;
    }
    #[must_use]
    pub fn interpolated(&self) -> crate::InterpolatedState {
        self.interpolated_with_alpha(if self.terminal.is_some() {
            1.0
        } else {
            self.fixed.interpolation_alpha()
        })
    }
    pub(crate) fn interpolated_with_alpha(&self, alpha: f64) -> crate::InterpolatedState {
        let previous = &self.committed.previous.rigid_body;
        let current = &self.state().rigid_body;
        let position = Ecef::from_vec(
            previous
                .position
                .as_vec()
                .lerp(current.position.as_vec(), alpha),
        );
        let orientation = previous
            .orientation
            .slerp(current.orientation, alpha)
            .normalize();
        let geodetic = position.to_geodetic();
        let attitude = Attitude::from_quaternion(
            LocalFrame::new(geodetic).ned_to_ecef_rotation().inverse() * orientation,
        );
        crate::InterpolatedState {
            position,
            orientation,
            geodetic,
            attitude,
        }
    }
    fn sample_ground(&mut self, position: Geodetic) -> GroundPlane {
        if let Some(terrain) = &mut self.terrain {
            GroundSampler::default().sample(terrain, position)
        } else {
            let TurbopropTerrain::Flat { elevation } = self.environment.terrain else {
                unreachable!()
            };
            GroundPlane {
                reference: Geodetic::new(position.latitude, position.longitude, Meters::ZERO),
                elevation,
                slope: GroundSlope::LEVEL,
                from_terrain: true,
            }
        }
    }
    fn environment_at(
        &self,
        state: &TurbopropState,
        ground: GroundPlane,
        elapsed: Seconds,
    ) -> Environment {
        let position = state.rigid_body.geodetic();
        let atmosphere = match (&self.climate, self.environment.conditions.climate_date) {
            (Some(climate), Some(date)) => Atmosphere::with_temperature_offset(
                climate.sample(position, date).isa_temperature_offset.get(),
            ),
            _ => Atmosphere::standard(),
        };
        Environment::with_wind_ned(
            atmosphere,
            position,
            self.environment.conditions.wind.to_ned(),
        )
        .with_turbulence(self.environment.conditions.turbulence, elapsed, position)
        .with_ground_plane(ground.reference, ground.elevation, ground.slope)
    }
    #[must_use]
    pub fn airspeed(&self) -> MetersPerSecond {
        flightsim_fdm::aero_angles_of(
            &self.state().rigid_body,
            &self.environment_at(self.state(), self.committed.ground, self.elapsed()),
        )
        .true_airspeed
    }
    /// Actual rotated-wheel geometry, positive above the held ground plane.
    #[must_use]
    pub fn minimum_gear_clearance(&self) -> Meters {
        Meters(
            self.committed
                .gear_clearances
                .iter()
                .map(|value| value.get())
                .fold(f64::INFINITY, f64::min),
        )
    }
    fn update_contact_and_log(&mut self) {
        let state = self.committed.state;
        let before = self.committed.previous;
        let clearance = self.minimum_gear_clearance().get();
        if self.committed.airborne && clearance <= 0.0 {
            self.committed.airborne = false;
            let attitude = before.rigid_body.attitude();
            self.committed.last_touchdown = Some(Touchdown {
                position: state.rigid_body.geodetic(),
                sink_rate: MetersPerSecond(-before.rigid_body.vertical_speed().get()),
                ground_speed: before.rigid_body.ground_speed(),
                bank: attitude.roll,
                heading: attitude.yaw,
                elapsed: self.elapsed(),
            });
            self.committed.touchdown_count = self.committed.touchdown_count.saturating_add(1);
            self.committed.log.landings = self.committed.touchdown_count;
        } else if !self.committed.airborne && clearance > 0.5 {
            self.committed.airborne = true;
        }
        self.committed.log.distance += before
            .rigid_body
            .geodetic()
            .great_circle_distance(state.rigid_body.geodetic());
        if self.committed.airborne {
            self.committed.log.airborne_time += TURBOPROP_FIXED_DT;
        }
        self.committed.log.peak_agl = Meters(
            self.committed
                .log
                .peak_agl
                .get()
                .max(state.rigid_body.altitude().get() - self.committed.ground.elevation.get()),
        );
        self.committed.log.peak_airspeed = MetersPerSecond(
            self.committed
                .log
                .peak_airspeed
                .get()
                .max(self.airspeed().get()),
        );
    }
}
