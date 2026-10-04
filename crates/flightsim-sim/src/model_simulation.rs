//! Transactional simulation for the explicit jet model. Legacy simulation is untouched.
use crate::{
    FlightLog, GroundPlane, GroundSampler, Touchdown,
    replay::{EnvironmentConditions, ReplayError},
    weather::WeatherSelection,
};
use flightsim_core::{
    Attitude, FixedStep, Geodetic, Meters, MetersPerSecond, Ned, Radians, Seconds,
};
use flightsim_fdm::subsonic::{JetAircraftConfig, JetFlightDynamics, JetStepError};
use flightsim_fdm::{Atmosphere, ControlInputs, Environment, GroundSlope, RigidBodyState};
use flightsim_world::{
    GlobalClimate, MemoryTileSource, Terrain,
    global::{GlobalTerrain, GlobalTileSource},
};

/// Pins fixed time, prospective weather, terrain sampling and contact semantics.
pub const JET_SIMULATION_REVISION: u32 = 1;
pub const JET_FIXED_DT: Seconds = Seconds(1.0 / 120.0);

/// The only supported terrain sources; regional packages cannot enter this API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JetTerrain {
    Flat { elevation: Meters },
    BundledGlobal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JetEnvironment {
    pub conditions: EnvironmentConditions,
    pub terrain: JetTerrain,
    /// Complete authored scenario identity. Headless execution does not render it.
    pub weather: WeatherSelection,
}
impl Default for JetEnvironment {
    fn default() -> Self {
        Self {
            conditions: EnvironmentConditions::default(),
            terrain: JetTerrain::Flat {
                elevation: Meters::ZERO,
            },
            weather: WeatherSelection::Legacy,
        }
    }
}
impl JetEnvironment {
    /// Validate structural settings. Dataset compatibility is checked separately
    /// when constructing a simulation, so unknown fingerprints remain inspectable.
    /// # Errors
    /// Invalid, inconsistent or unsupported conditions.
    pub fn validate(self) -> Result<(), ReplayError> {
        crate::replay::validate_environment(&self.conditions)?;
        let global = matches!(self.terrain, JetTerrain::BundledGlobal);
        crate::replay::require_valid(
            global == self.conditions.world_terrain,
            "jet terrain kind",
            None,
            "must agree with global terrain flag",
        )?;
        if let JetTerrain::Flat { elevation } = self.terrain {
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
pub enum JetSimulationError {
    Conditions(ReplayError),
    Initial(JetStepError),
    World(String),
}
impl std::fmt::Display for JetSimulationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conditions(e) => e.fmt(f),
            Self::Initial(e) => e.fmt(f),
            Self::World(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for JetSimulationError {}

/// Committed state only. A rejection may change the visible terminal latch and
/// discard fractional frame budget; every field of this snapshot stays exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JetSnapshot {
    pub state: RigidBodyState,
    pub previous: RigidBodyState,
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
pub struct JetTerminalEvent {
    pub cursor: u32,
    pub controls: ControlInputs,
    pub failure: JetStepError,
}
#[derive(Debug, Clone, Copy)]
pub struct JetCommittedStep {
    pub(crate) cursor: u32,
    pub(crate) controls: ControlInputs,
    pub(crate) before: RigidBodyState,
    pub(crate) after: RigidBodyState,
}
#[derive(Debug, Default)]
pub struct JetAdvance {
    pub(crate) committed: Vec<JetCommittedStep>,
    pub(crate) terminal: Option<JetTerminalEvent>,
}
impl JetAdvance {
    #[must_use]
    pub fn committed_steps(&self) -> usize {
        self.committed.len()
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<JetTerminalEvent> {
        self.terminal
    }
    /// Successful calls plus this report's one possible rejected call.
    #[must_use]
    pub fn attempted_steps(&self) -> usize {
        self.committed.len() + usize::from(self.terminal.is_some())
    }
}

#[derive(Debug)]
pub struct JetSimulation {
    dynamics: JetFlightDynamics,
    environment: JetEnvironment,
    terrain: Option<Terrain<GlobalTileSource<MemoryTileSource>>>,
    climate: Option<GlobalClimate>,
    fixed: FixedStep,
    committed: JetSnapshot,
    terminal: Option<JetTerminalEvent>,
}
impl JetSimulation {
    /// Initial CG state is authoritative. Contact initialization uses its gear
    /// clearance (>0.5 m is airborne), identically for live and replay starts.
    /// # Errors
    /// Invalid state/environment or unavailable bundled datasets.
    pub fn from_state(
        config: JetAircraftConfig,
        state: RigidBodyState,
        environment: JetEnvironment,
    ) -> Result<Self, JetSimulationError> {
        environment
            .validate()
            .map_err(JetSimulationError::Conditions)?;
        environment
            .conditions
            .check_world()
            .map_err(JetSimulationError::Conditions)?;
        let dynamics =
            JetFlightDynamics::new(config, state).map_err(JetSimulationError::Initial)?;
        let terrain = if matches!(environment.terrain, JetTerrain::BundledGlobal) {
            let global =
                GlobalTerrain::bundled().map_err(|e| JetSimulationError::World(e.to_string()))?;
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
            .map_err(|e| JetSimulationError::World(e.to_string()))?;
        let placeholder = GroundPlane {
            reference: state.geodetic(),
            elevation: Meters::ZERO,
            slope: GroundSlope::LEVEL,
            from_terrain: false,
        };
        let mut sim = Self {
            dynamics,
            environment,
            terrain,
            climate,
            fixed: FixedStep::new(JET_FIXED_DT),
            committed: JetSnapshot {
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
        sim.committed.ground = sim.sample_ground(state.geodetic());
        sim.committed.gear_clearances = sim
            .dynamics
            .gear_clearances(
                &state,
                &sim.environment_at(&state, sim.committed.ground, Seconds::ZERO),
            )
            .map_err(JetSimulationError::Initial)?;
        sim.committed.airborne = sim.minimum_gear_clearance().get() > 0.5;
        Ok(sim)
    }
    #[must_use]
    pub const fn snapshot(&self) -> JetSnapshot {
        self.committed
    }
    #[must_use]
    pub const fn state(&self) -> &RigidBodyState {
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
    pub const fn terminal(&self) -> Option<JetTerminalEvent> {
        self.terminal
    }
    #[must_use]
    pub const fn environment(&self) -> JetEnvironment {
        self.environment
    }
    #[must_use]
    pub const fn config(&self) -> &JetAircraftConfig {
        self.dynamics.config()
    }
    #[must_use]
    pub const fn accumulated(&self) -> Seconds {
        self.fixed.accumulated()
    }

    /// Rebuild clocks, contact history, interpolation and terminal state.
    /// # Errors
    /// Invalid replacement state or environment. The old simulation is preserved.
    pub fn restart_at(&mut self, state: RigidBodyState) -> Result<(), JetSimulationError> {
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
        mut prepare: impl FnMut(&mut C, Seconds, &RigidBodyState) -> ControlInputs,
    ) -> JetAdvance {
        let mut report = JetAdvance::default();
        if self.terminal.is_some() {
            return report;
        }
        let due = self.fixed.advance(frame_time);
        for _ in 0..due {
            let mut proposed = controller.clone();
            let controls = prepare(&mut proposed, JET_FIXED_DT, self.state());
            match self.attempt_step(controls) {
                Ok(step) => {
                    *controller = proposed;
                    report.committed.push(step);
                }
                Err(event) => {
                    report.terminal = Some(event);
                    break;
                }
            }
        }
        report
    }
    pub fn advance(&mut self, frame_time: Seconds, controls: ControlInputs) -> JetAdvance {
        self.advance_with_controller(frame_time, &mut (), |(), _, _| controls)
    }

    /// Attempt one exact replay tick, independent of the render accumulator.
    pub(crate) fn attempt_step(
        &mut self,
        controls: ControlInputs,
    ) -> Result<JetCommittedStep, JetTerminalEvent> {
        if let Some(event) = self.terminal {
            return Err(event);
        }
        let before = self.committed.state;
        let ground = self.sample_ground(before.geodetic());
        let elapsed = self.committed.elapsed + JET_FIXED_DT;
        let environment = self.environment_at(&before, ground, elapsed);
        let outcome = match self.dynamics.step(JET_FIXED_DT, controls, &environment) {
            Ok(outcome) => outcome,
            Err(failure) => {
                let event = JetTerminalEvent {
                    cursor: self.committed.committed_steps,
                    controls,
                    failure,
                };
                self.terminal = Some(event);
                self.fixed.reset();
                return Err(event);
            }
        };
        self.committed.previous = before;
        self.committed.state = *self.dynamics.state();
        self.committed.ground = ground;
        self.committed.gear_clearances = outcome.gear_clearances;
        self.committed.elapsed = elapsed;
        self.committed.committed_steps += 1;
        self.update_contact_and_log();
        Ok(JetCommittedStep {
            cursor: self.committed.committed_steps,
            controls,
            before,
            after: self.committed.state,
        })
    }
    /// Probe terminal evidence without committing an unexpected successful step.
    pub(crate) fn attempt_expected_rejection(
        &mut self,
        controls: ControlInputs,
    ) -> Result<(), JetTerminalEvent> {
        if let Some(event) = self.terminal {
            return Err(event);
        }
        let before = self.committed.state;
        let ground = self.sample_ground(before.geodetic());
        let environment = self.environment_at(&before, ground, self.elapsed() + JET_FIXED_DT);
        let mut candidate = self.dynamics.clone();
        match candidate.step(JET_FIXED_DT, controls, &environment) {
            Ok(_) => Ok(()),
            Err(failure) => {
                let event = JetTerminalEvent {
                    cursor: self.committed.committed_steps,
                    controls,
                    failure,
                };
                self.terminal = Some(event);
                self.fixed.reset();
                Err(event)
            }
        }
    }
    fn sample_ground(&mut self, position: Geodetic) -> GroundPlane {
        if let Some(terrain) = &mut self.terrain {
            GroundSampler::default().sample(terrain, position)
        } else {
            let JetTerrain::Flat { elevation } = self.environment.terrain else {
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
        state: &RigidBodyState,
        ground: GroundPlane,
        elapsed: Seconds,
    ) -> Environment {
        let position = state.geodetic();
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
            self.state(),
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
            let attitude = before.attitude();
            self.committed.last_touchdown = Some(Touchdown {
                position: state.geodetic(),
                sink_rate: MetersPerSecond(-before.vertical_speed().get()),
                ground_speed: before.ground_speed(),
                bank: attitude.roll,
                heading: attitude.yaw,
                elapsed: self.elapsed(),
            });
            self.committed.touchdown_count = self.committed.touchdown_count.saturating_add(1);
            self.committed.log.landings = self.committed.touchdown_count;
        } else if !self.committed.airborne && clearance > 0.5 {
            self.committed.airborne = true;
        }
        self.committed.log.distance += before.geodetic().great_circle_distance(state.geodetic());
        if self.committed.airborne {
            self.committed.log.airborne_time += JET_FIXED_DT;
        }
        self.committed.log.peak_agl = Meters(
            self.committed
                .log
                .peak_agl
                .get()
                .max(state.altitude().get() - self.committed.ground.elevation.get()),
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

/// Flat-ground numerical fixture initialization. No legacy propeller config is created.
#[must_use]
pub fn jet_parked_state(
    config: &JetAircraftConfig,
    position: Geodetic,
    elevation: Meters,
    heading: Radians,
) -> RigidBodyState {
    let height = config
        .airframe()
        .landing_gear()
        .legs()
        .iter()
        .map(|leg| leg.contact_point().as_vec().z)
        .fold(0.0, f64::max);
    RigidBodyState::from_geodetic(
        Geodetic::new(
            position.latitude,
            position.longitude,
            Meters(elevation.get() + height),
        ),
        Attitude::new(Radians::ZERO, Radians::ZERO, heading),
        Ned::new(0.0, 0.0, 0.0),
    )
}
