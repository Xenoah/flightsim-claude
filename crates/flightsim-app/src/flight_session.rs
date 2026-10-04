//! App-owned explicit four-family dispatch. Each replay owns its only simulation.
use crate::{BoxedSource, StartCondition, Startup};
use bevy::prelude::*;
use flightsim_core::{Geodetic, Meters, MetersPerSecond, Radians, Seconds};
use flightsim_fdm::{ControlInputs, RigidBodyState};
use flightsim_input::{PilotControls, SampledPilotInput};
use flightsim_sim::{
    Simulation,
    model_simulation::{JetEnvironment, JetSimulation, JetTerrain},
    near_static_turboprop_simulation::NearStaticTurbopropSimulation,
    replay_v4::{JetRecorder, JetReplayPlayer},
    replay_v5::{TurbopropRecorder, TurbopropReplayPlayer},
    replay_v6::{NearStaticTurbopropRecorder, NearStaticTurbopropReplayPlayer},
    turboprop_simulation::TurbopropSimulation,
};

pub(super) enum FlightSession {
    Legacy(Simulation<BoxedSource>),
    JetLive {
        simulation: JetSimulation,
        recorder: JetRecorder,
        parking_brake: bool,
        pending_parking_toggle: bool,
        last_controls: ControlInputs,
        recording_error: Option<String>,
        fault: Option<String>,
    },
    JetReplay {
        player: JetReplayPlayer,
        fault: Option<String>,
        pending_seek: Option<u32>,
        total: Seconds,
    },
    TurbopropLive {
        simulation: TurbopropSimulation,
        recorder: TurbopropRecorder,
        parking_brake: bool,
        pending_parking_toggle: bool,
        last_controls: ControlInputs,
        recording_error: Option<String>,
        fault: Option<String>,
    },
    TurbopropReplay {
        player: TurbopropReplayPlayer,
        fault: Option<String>,
        pending_seek: Option<u32>,
        total: Seconds,
    },
    NearStaticTurbopropLive {
        simulation: NearStaticTurbopropSimulation,
        recorder: NearStaticTurbopropRecorder,
        parking_brake: bool,
        pending_parking_toggle: bool,
        last_controls: ControlInputs,
        recording_error: Option<String>,
        fault: Option<String>,
    },
    NearStaticTurbopropReplay {
        player: NearStaticTurbopropReplayPlayer,
        fault: Option<String>,
        pending_seek: Option<u32>,
        total: Seconds,
    },
}
impl From<Simulation<BoxedSource>> for FlightSession {
    fn from(value: Simulation<BoxedSource>) -> Self {
        Self::Legacy(value)
    }
}
#[derive(Resource)]
pub(super) struct PreparedJetSession(pub Option<(FlightSession, StartCondition)>);

impl FlightSession {
    pub fn replay(player: JetReplayPlayer) -> Self {
        let total = player.recording().duration();
        Self::JetReplay {
            player,
            total,
            fault: None,
            pending_seek: None,
        }
    }

    pub fn replay_turboprop(player: TurbopropReplayPlayer) -> Self {
        let total = player.recording().duration();
        Self::TurbopropReplay {
            player,
            total,
            fault: None,
            pending_seek: None,
        }
    }
    pub fn replay_near_static_turboprop(player: NearStaticTurbopropReplayPlayer) -> Self {
        let total = player.recording().duration();
        Self::NearStaticTurbopropReplay {
            player,
            total,
            fault: None,
            pending_seek: None,
        }
    }
    pub fn prepare_jet(
        startup: &Startup,
        clock: &flightsim_render::TimeOfDay,
        start: StartCondition,
    ) -> Result<Self, String> {
        validate_jet_sources(startup)?;
        let config = startup
            .aircraft
            .jet()
            .ok_or("jet profile required")?
            .configuration()
            .clone();
        let mut conditions = crate::environment_conditions(startup, clock);
        conditions.time_rate = clock.rate.get();
        let environment = JetEnvironment {
            conditions,
            terrain: if startup.world.global_terrain {
                JetTerrain::BundledGlobal
            } else {
                JetTerrain::Flat {
                    elevation: Meters::ZERO,
                }
            },
            weather: startup.weather.selection,
        };
        let (simulation, parking_brake) = match start {
            StartCondition::Parked { position, heading } => (
                JetSimulation::parked(config, position, heading, environment)
                    .map_err(|e| e.to_string())?,
                true,
            ),
            StartCondition::InFlight(state) => (
                JetSimulation::from_state(config, state, environment).map_err(|e| e.to_string())?,
                false,
            ),
        };
        validate_jet_state(simulation.state())?;
        if simulation.presentation().aero_coefficients.is_none() {
            return Err("initial jet pressure, temperature or Mach lies outside the aircraft operating envelope".into());
        }
        let recorder = JetRecorder::new(&simulation).map_err(|e| e.to_string())?;
        let input = crate::world_runtime::initial_controls(startup).to_control_inputs();
        Ok(Self::JetLive {
            simulation,
            recorder,
            parking_brake,
            pending_parking_toggle: false,
            last_controls: input.with_brakes(input.brakes().max(if parking_brake {
                1.0
            } else {
                0.0
            })),
            recording_error: None,
            fault: None,
        })
    }
    pub fn is_jet(&self) -> bool {
        matches!(self, Self::JetLive { .. } | Self::JetReplay { .. })
    }
    pub fn is_turboprop(&self) -> bool {
        matches!(
            self,
            Self::TurbopropLive { .. }
                | Self::TurbopropReplay { .. }
                | Self::NearStaticTurbopropLive { .. }
                | Self::NearStaticTurbopropReplay { .. }
        )
    }
    pub fn uses_bounded_model(&self) -> bool {
        matches!(
            self,
            Self::JetLive { .. }
                | Self::JetReplay { .. }
                | Self::TurbopropLive { .. }
                | Self::TurbopropReplay { .. }
                | Self::NearStaticTurbopropLive { .. }
                | Self::NearStaticTurbopropReplay { .. }
        )
    }
    pub fn prepare_bounded(
        startup: &Startup,
        clock: &flightsim_render::TimeOfDay,
        start: StartCondition,
    ) -> Result<Self, String> {
        match &startup.aircraft {
            crate::aircraft_profile::SelectedAircraftProfile::Jet(_) => {
                Self::prepare_jet(startup, clock, start)
            }
            crate::aircraft_profile::SelectedAircraftProfile::Turboprop(_) => {
                crate::turboprop_session::prepare(startup, clock, start)
            }
            crate::aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(_) => {
                crate::near_static_turboprop_session::prepare(startup, clock, start)
            }
            crate::aircraft_profile::SelectedAircraftProfile::Legacy(_) => {
                Err("bounded session requires an explicit jet or turboprop profile".into())
            }
        }
    }
    pub fn turboprop(&self) -> Option<&TurbopropSimulation> {
        match self {
            Self::TurbopropLive { simulation, .. } => Some(simulation),
            Self::TurbopropReplay { player, .. } => Some(player.simulation()),
            _ => None,
        }
    }
    pub fn is_near_static_turboprop(&self) -> bool {
        matches!(
            self,
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. }
        )
    }
    pub fn near_static_turboprop(&self) -> Option<&NearStaticTurbopropSimulation> {
        match self {
            Self::NearStaticTurbopropLive { simulation, .. } => Some(simulation),
            Self::NearStaticTurbopropReplay { player, .. } => Some(player.simulation()),
            _ => None,
        }
    }
    pub fn model_conditions(&self) -> Option<flightsim_sim::replay::EnvironmentConditions> {
        match self {
            Self::JetLive { simulation, .. } => Some(simulation.environment().conditions),
            Self::JetReplay { player, .. } => Some(player.simulation().environment().conditions),
            Self::TurbopropLive { simulation, .. } => Some(simulation.environment().conditions),
            Self::NearStaticTurbopropLive { simulation, .. } => {
                Some(simulation.environment().conditions)
            }
            Self::TurbopropReplay { player, .. } => {
                Some(player.simulation().environment().conditions)
            }
            Self::NearStaticTurbopropReplay { player, .. } => {
                Some(player.simulation().environment().conditions)
            }
            Self::Legacy(_) => None,
        }
    }
    pub fn weather_selection(&self) -> Option<flightsim_sim::weather::WeatherSelection> {
        match self {
            Self::JetLive { simulation, .. } => Some(simulation.environment().weather),
            Self::JetReplay { player, .. } => Some(player.simulation().environment().weather),
            Self::TurbopropLive { simulation, .. } => Some(simulation.environment().weather),
            Self::NearStaticTurbopropLive { simulation, .. } => {
                Some(simulation.environment().weather)
            }
            Self::TurbopropReplay { player, .. } => Some(player.simulation().environment().weather),
            Self::NearStaticTurbopropReplay { player, .. } => {
                Some(player.simulation().environment().weather)
            }
            Self::Legacy(_) => None,
        }
    }
    pub fn is_replay(&self) -> bool {
        matches!(
            self,
            Self::JetReplay { .. }
                | Self::TurbopropReplay { .. }
                | Self::NearStaticTurbopropReplay { .. }
        )
    }
    pub fn jet(&self) -> Option<&JetSimulation> {
        match self {
            Self::Legacy(_)
            | Self::TurbopropLive { .. }
            | Self::TurbopropReplay { .. }
            | Self::NearStaticTurbopropLive { .. }
            | Self::NearStaticTurbopropReplay { .. } => None,
            Self::JetLive { simulation, .. } => Some(simulation),
            Self::JetReplay { player, .. } => Some(player.simulation()),
        }
    }
    #[cfg(test)]
    pub fn jet_environment(&self) -> Option<JetEnvironment> {
        self.jet().map(JetSimulation::environment)
    }
    pub fn legacy(&self) -> Option<&Simulation<BoxedSource>> {
        if let Self::Legacy(sim) = self {
            Some(sim)
        } else {
            None
        }
    }
    pub fn legacy_mut(&mut self) -> &mut Simulation<BoxedSource> {
        match self {
            Self::Legacy(sim) => sim,
            _ => panic!("legacy API requires legacy flight session"),
        }
    }
    pub fn config(&self) -> &flightsim_fdm::AircraftConfig {
        self.legacy()
            .expect("legacy config requires legacy session")
            .config()
    }
    pub fn state(&self) -> &RigidBodyState {
        match self {
            Self::Legacy(s) => s.state(),
            Self::JetLive { simulation, .. } => simulation.state(),
            Self::JetReplay { player, .. } => player.simulation().state(),
            Self::TurbopropLive { simulation, .. } => &simulation.state().rigid_body,
            Self::NearStaticTurbopropLive { simulation, .. } => &simulation.state().rigid_body,
            Self::TurbopropReplay { player, .. } => &player.simulation().state().rigid_body,
            Self::NearStaticTurbopropReplay { player, .. } => {
                &player.simulation().state().rigid_body
            }
        }
    }
    pub fn elapsed(&self) -> Seconds {
        match self {
            Self::Legacy(s) => s.elapsed(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().elapsed(),
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().elapsed()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().elapsed()
            }
        }
    }
    pub fn interpolated(&self) -> flightsim_sim::InterpolatedState {
        match self {
            Self::Legacy(s) => s.interpolated(),
            Self::JetLive { simulation, .. } => simulation.interpolated(),
            Self::JetReplay { player, .. } => player.interpolated(),
            Self::TurbopropLive { simulation, .. } => simulation.interpolated(),
            Self::NearStaticTurbopropLive { simulation, .. } => simulation.interpolated(),
            Self::TurbopropReplay { player, .. } => player.interpolated(),
            Self::NearStaticTurbopropReplay { player, .. } => player.interpolated(),
        }
    }
    pub fn ground(&self) -> flightsim_sim::GroundPlane {
        match self {
            Self::Legacy(s) => s.ground(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().snapshot().ground,
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().snapshot().ground
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().snapshot().ground
            }
        }
    }
    pub fn agl(&self) -> Meters {
        match self {
            Self::Legacy(s) => s.agl(),
            _ => self.state().altitude() - self.ground().elevation,
        }
    }
    pub fn wind(&self) -> flightsim_sim::Wind {
        match self {
            Self::Legacy(s) => s.wind(),
            _ => self.model_conditions().unwrap().wind,
        }
    }
    #[cfg(test)]
    pub fn climate(&self) -> Option<flightsim_world::ClimateDate> {
        match self {
            Self::Legacy(s) => s.climate(),
            _ => self.model_conditions().unwrap().climate_date,
        }
    }
    pub fn climate_sample(&self) -> Option<flightsim_world::ClimateSample> {
        match self {
            Self::Legacy(s) => s.climate_sample(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().climate_sample(),
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().climate_sample()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().climate_sample()
            }
        }
    }
    pub fn atmosphere_sample(&self) -> flightsim_fdm::AtmosphereSample {
        match self {
            Self::Legacy(s) => s.atmosphere_sample(),
            Self::JetLive { .. } | Self::JetReplay { .. } => {
                self.jet().unwrap().atmosphere_sample()
            }
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().atmosphere_sample()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().atmosphere_sample()
            }
        }
    }
    pub fn aero_angles(&self) -> flightsim_fdm::AeroAngles {
        match self {
            Self::Legacy(s) => s.aero_angles(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().aero_angles(),
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().aero_angles()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().aero_angles()
            }
        }
    }
    pub fn airspeed(&self) -> MetersPerSecond {
        match self {
            Self::Legacy(s) => s.airspeed(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().airspeed(),
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().airspeed()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().airspeed()
            }
        }
    }
    pub fn log(&self) -> flightsim_sim::FlightLog {
        match self {
            Self::Legacy(s) => s.log(),
            Self::JetLive { .. } | Self::JetReplay { .. } => self.jet().unwrap().log(),
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().log()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop().unwrap().log()
            }
        }
    }
    pub fn crash(&self) -> Option<&flightsim_sim::Crash> {
        self.legacy().and_then(Simulation::crash)
    }
    pub fn crashed(&self) -> bool {
        self.crash().is_some()
    }
    pub fn diverged(&self) -> bool {
        match self {
            Self::Legacy(s) => s.diverged(),
            _ => self.fault().is_some(),
        }
    }
    pub fn touchdown_count(&self) -> u32 {
        match self {
            Self::Legacy(s) => s.touchdown_count(),
            Self::JetLive { .. } | Self::JetReplay { .. } => {
                self.jet().unwrap().snapshot().touchdown_count
            }
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().snapshot().touchdown_count
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop()
                    .unwrap()
                    .snapshot()
                    .touchdown_count
            }
        }
    }
    pub fn last_touchdown(&self) -> Option<flightsim_sim::Touchdown> {
        match self {
            Self::Legacy(s) => s.last_touchdown().copied(),
            Self::JetLive { .. } | Self::JetReplay { .. } => {
                self.jet().unwrap().snapshot().last_touchdown
            }
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().snapshot().last_touchdown
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop()
                    .unwrap()
                    .snapshot()
                    .last_touchdown
            }
        }
    }
    pub fn on_ground(&self) -> bool {
        match self {
            Self::Legacy(s) => s.agl().get() < flightsim_sim::gear_height(s.config()).get() + 0.3,
            Self::JetLive { .. } | Self::JetReplay { .. } => {
                self.jet().unwrap().presentation().on_ground
            }
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().presentation().on_ground
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => {
                self.near_static_turboprop()
                    .unwrap()
                    .presentation()
                    .on_ground
            }
        }
    }
    pub fn fault(&self) -> Option<&str> {
        match self {
            Self::Legacy(_) => None,
            Self::JetLive { fault, .. }
            | Self::JetReplay { fault, .. }
            | Self::TurbopropLive { fault, .. }
            | Self::TurbopropReplay { fault, .. }
            | Self::NearStaticTurbopropLive { fault, .. }
            | Self::NearStaticTurbopropReplay { fault, .. } => fault.as_deref(),
        }
    }
    pub fn terminal_message(&self) -> Option<String> {
        self.fault().map(str::to_owned).or_else(|| {
            if let Some(jet) = self.jet() {
                jet.terminal()
                    .map(|event| format!("JET STOPPED at step {}: {}", event.cursor, event.failure))
            } else if let Some(turboprop) = self.turboprop() {
                turboprop.terminal().map(|event| {
                    format!(
                        "TURBOPROP STOPPED at step {}: {}",
                        event.cursor, event.failure
                    )
                })
            } else if let Some(simulation) = self.near_static_turboprop() {
                simulation.terminal().map(|event| {
                    format!(
                        "NEAR-STATIC TURBOPROP STOPPED at step {}: {}",
                        event.cursor, event.failure
                    )
                })
            } else {
                None
            }
        })
    }
    pub fn audio_paused(&self) -> bool {
        match self {
            Self::Legacy(_) => false,
            Self::JetLive {
                simulation, fault, ..
            } => simulation.terminal().is_some() || fault.is_some(),
            Self::JetReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                pending_seek.is_some()
                    || player.paused()
                    || player.seeking()
                    || player.finished()
                    || player.faulted()
                    || fault.is_some()
            }
            Self::TurbopropLive {
                simulation, fault, ..
            } => simulation.terminal().is_some() || fault.is_some(),
            Self::NearStaticTurbopropLive {
                simulation, fault, ..
            } => simulation.terminal().is_some() || fault.is_some(),
            Self::TurbopropReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                pending_seek.is_some()
                    || player.paused()
                    || player.seeking()
                    || player.finished()
                    || player.faulted()
                    || fault.is_some()
            }
            Self::NearStaticTurbopropReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                pending_seek.is_some()
                    || player.paused()
                    || player.seeking()
                    || player.finished()
                    || player.faulted()
                    || fault.is_some()
            }
        }
    }
    pub fn seeking(&self) -> bool {
        match self {
            Self::JetReplay {
                player,
                pending_seek,
                ..
            } => pending_seek.is_some() || player.seeking(),
            Self::TurbopropReplay {
                player,
                pending_seek,
                ..
            } => pending_seek.is_some() || player.seeking(),
            Self::NearStaticTurbopropReplay {
                player,
                pending_seek,
                ..
            } => pending_seek.is_some() || player.seeking(),
            _ => false,
        }
    }
    pub fn last_controls(&self) -> Option<ControlInputs> {
        match self {
            Self::Legacy(_) => None,
            Self::JetLive { last_controls, .. }
            | Self::TurbopropLive { last_controls, .. }
            | Self::NearStaticTurbopropLive { last_controls, .. } => Some(*last_controls),
            Self::JetReplay { player, .. } => Some(player.last_controls()),
            Self::TurbopropReplay { player, .. } => Some(player.last_controls()),
            Self::NearStaticTurbopropReplay { player, .. } => Some(player.last_controls()),
        }
    }
    pub fn queue_parking_toggle(&mut self) {
        if let Self::JetLive {
            pending_parking_toggle,
            ..
        }
        | Self::TurbopropLive {
            pending_parking_toggle,
            ..
        }
        | Self::NearStaticTurbopropLive {
            pending_parking_toggle,
            ..
        } = self
        {
            *pending_parking_toggle = !*pending_parking_toggle;
        }
    }
    pub fn parking_brake(&self) -> Option<(bool, bool)> {
        match self {
            Self::JetLive {
                parking_brake,
                pending_parking_toggle,
                ..
            }
            | Self::TurbopropLive {
                parking_brake,
                pending_parking_toggle,
                ..
            }
            | Self::NearStaticTurbopropLive {
                parking_brake,
                pending_parking_toggle,
                ..
            } => Some((*parking_brake, *pending_parking_toggle)),
            _ => None,
        }
    }
    pub fn advance_bounded(
        &mut self,
        dt: Seconds,
        controls: &mut PilotControls,
        sample: &SampledPilotInput,
        recording_allowed: bool,
    ) {
        match self {
            Self::JetLive {
                simulation,
                recorder,
                parking_brake,
                pending_parking_toggle,
                last_controls,
                recording_error,
                fault,
            } => {
                if fault.is_some() || simulation.terminal().is_some() {
                    return;
                }
                let mut pilot = (
                    *controls,
                    *parking_brake,
                    *pending_parking_toggle,
                    *last_controls,
                );
                let report =
                    simulation.advance_with_controller(dt, &mut pilot, |pilot, fixed_dt, _| {
                        pilot.0.update_from_sample(fixed_dt, sample);
                        if pilot.2 {
                            pilot.1 = !pilot.1;
                            pilot.2 = false;
                        }
                        let input = pilot.0.to_control_inputs();
                        pilot.3 =
                            input.with_brakes(input.brakes().max(if pilot.1 { 1.0 } else { 0.0 }));
                        pilot.3
                    });
                *controls = pilot.0;
                *parking_brake = pilot.1;
                *pending_parking_toggle = pilot.2;
                *last_controls = pilot.3;
                if recording_allowed
                    && recording_error.is_none()
                    && report.attempted_steps() > 0
                    && let Err(error) = recorder.record(&report)
                {
                    *recording_error = Some(format!(
                        "RECORDING STOPPED: {error}; F9 saves the last valid prefix"
                    ));
                }
                if let Err(error) = validate_jet_state(simulation.state()) {
                    *fault = Some(format!("JET RENDER STOPPED: {error}"));
                }
            }
            Self::JetReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                let result = if let Some(target) = pending_seek.take() {
                    *fault = None;
                    player.seek_to(target)
                } else {
                    if fault.is_some() {
                        return;
                    }
                    if player.seeking() {
                        player.continue_seek()
                    } else {
                        player.advance(dt)
                    }
                };
                if let Err(error) = result {
                    *fault = Some(format!("JET REPLAY STOPPED: {error}"));
                }
                if let Err(error) = validate_jet_state(player.simulation().state()) {
                    *fault = Some(format!("JET REPLAY RENDER STOPPED: {error}"));
                }
            }
            Self::TurbopropLive {
                simulation,
                recorder,
                parking_brake,
                pending_parking_toggle,
                last_controls,
                recording_error,
                fault,
            } => {
                if fault.is_some() || simulation.terminal().is_some() {
                    return;
                }
                let mut pilot = (
                    *controls,
                    *parking_brake,
                    *pending_parking_toggle,
                    *last_controls,
                );
                let report =
                    simulation.advance_with_controller(dt, &mut pilot, |pilot, fixed_dt, _| {
                        pilot.0.update_from_sample(fixed_dt, sample);
                        if pilot.2 {
                            pilot.1 = !pilot.1;
                            pilot.2 = false;
                        }
                        let input = pilot.0.to_control_inputs();
                        pilot.3 =
                            input.with_brakes(input.brakes().max(if pilot.1 { 1.0 } else { 0.0 }));
                        pilot.3
                    });
                *controls = pilot.0;
                *parking_brake = pilot.1;
                *pending_parking_toggle = pilot.2;
                *last_controls = pilot.3;
                if recording_allowed
                    && recording_error.is_none()
                    && report.attempted_steps() > 0
                    && let Err(error) = recorder.record(&report)
                {
                    *recording_error = Some(format!(
                        "RECORDING STOPPED: {error}; F9 saves the last valid prefix"
                    ));
                }
                if let Err(error) = validate_model_state(&simulation.state().rigid_body) {
                    *fault = Some(format!("TURBOPROP RENDER STOPPED: {error}"));
                }
            }
            Self::NearStaticTurbopropLive {
                simulation,
                recorder,
                parking_brake,
                pending_parking_toggle,
                last_controls,
                recording_error,
                fault,
            } => {
                if fault.is_some() || simulation.terminal().is_some() {
                    return;
                }
                let mut pilot = (
                    *controls,
                    *parking_brake,
                    *pending_parking_toggle,
                    *last_controls,
                );
                let report =
                    simulation.advance_with_controller(dt, &mut pilot, |pilot, fixed_dt, _| {
                        pilot.0.update_from_sample(fixed_dt, sample);
                        if pilot.2 {
                            pilot.1 = !pilot.1;
                            pilot.2 = false;
                        }
                        let input = pilot.0.to_control_inputs();
                        pilot.3 =
                            input.with_brakes(input.brakes().max(if pilot.1 { 1.0 } else { 0.0 }));
                        pilot.3
                    });
                *controls = pilot.0;
                *parking_brake = pilot.1;
                *pending_parking_toggle = pilot.2;
                *last_controls = pilot.3;
                if recording_allowed
                    && recording_error.is_none()
                    && report.attempted_steps() > 0
                    && let Err(error) = recorder.record(&report)
                {
                    *recording_error = Some(format!(
                        "RECORDING STOPPED: {error}; F9 saves the last valid prefix"
                    ));
                }
                if let Err(error) = validate_model_state(&simulation.state().rigid_body) {
                    *fault = Some(format!("TURBOPROP RENDER STOPPED: {error}"));
                }
            }
            Self::TurbopropReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                let result = if let Some(target) = pending_seek.take() {
                    *fault = None;
                    player.seek_to(target)
                } else {
                    if fault.is_some() {
                        return;
                    }
                    if player.seeking() {
                        player.continue_seek()
                    } else {
                        player.advance(dt)
                    }
                };
                if let Err(error) = result {
                    *fault = Some(format!("TURBOPROP REPLAY STOPPED: {error}"));
                }
                if let Err(error) = validate_model_state(&player.simulation().state().rigid_body) {
                    *fault = Some(format!("TURBOPROP REPLAY RENDER STOPPED: {error}"));
                }
            }
            Self::NearStaticTurbopropReplay {
                player,
                fault,
                pending_seek,
                ..
            } => {
                let result = if let Some(target) = pending_seek.take() {
                    *fault = None;
                    player.seek_to(target)
                } else {
                    if fault.is_some() {
                        return;
                    }
                    if player.seeking() {
                        player.continue_seek()
                    } else {
                        player.advance(dt)
                    }
                };
                if let Err(error) = result {
                    *fault = Some(format!("TURBOPROP REPLAY STOPPED: {error}"));
                }
                if let Err(error) = validate_model_state(&player.simulation().state().rigid_body) {
                    *fault = Some(format!("TURBOPROP REPLAY RENDER STOPPED: {error}"));
                }
            }
            Self::Legacy(_) => unreachable!(),
        }
    }
    pub fn restart_at(&mut self, state: RigidBodyState) {
        self.legacy_mut().restart_at(state);
    }
    pub fn restart_parked_at(&mut self, position: Geodetic, heading: Radians) {
        self.legacy_mut().restart_parked_at(position, heading);
    }
    #[allow(clippy::cast_possible_truncation)]
    pub fn placeholder_parts(&self) -> Vec<flightsim_render::aircraft::AircraftPart> {
        if let Some(sim) = self.legacy() {
            return flightsim_render::placeholder_parts(sim.config());
        }
        let geometry = match self {
            Self::JetLive { .. } | Self::JetReplay { .. } => {
                self.jet().unwrap().config().airframe().geometry()
            }
            Self::TurbopropLive { .. } | Self::TurbopropReplay { .. } => {
                self.turboprop().unwrap().config().airframe().geometry()
            }
            Self::NearStaticTurbopropLive { .. } | Self::NearStaticTurbopropReplay { .. } => self
                .near_static_turboprop()
                .unwrap()
                .config()
                .airframe()
                .geometry(),
            Self::Legacy(_) => unreachable!(),
        };
        vec![
            flightsim_render::aircraft::AircraftPart {
                name: if self.is_jet() {
                    "generic jet body"
                } else {
                    "numerical turboprop body"
                },
                mesh: Mesh::from(Cuboid::new(6.0, 0.9, 0.9)),
                transform: Transform::default(),
                color: Color::srgb(0.8, 0.85, 0.9),
            },
            flightsim_render::aircraft::AircraftPart {
                name: if self.is_jet() {
                    "generic jet wing"
                } else {
                    "numerical turboprop wing"
                },
                mesh: Mesh::from(Cuboid::new(
                    geometry.mean_chord.get() as f32,
                    geometry.wing_span.get() as f32,
                    0.12,
                )),
                transform: Transform::default(),
                color: Color::srgb(0.25, 0.45, 0.75),
            },
        ]
    }
}

pub(super) fn validate_model_sources(startup: &Startup) -> Result<(), String> {
    if startup.tiles.is_some()
        || startup.active_region.is_some()
        || startup.regions.select.is_some()
    {
        return Err("Jet/turboprop flights support only bundled global terrain or explicit flat-zero terrain; remove --tiles and regional package selection".into());
    }
    Ok(())
}
pub(super) fn validate_model_state(state: &RigidBodyState) -> Result<(), String> {
    crate::replay_runtime::validate_replay_position(state.position).map_err(str::to_owned)?;
    if !state.is_finite()
        || !state.velocity.length_squared().is_finite()
        || !state.angular_velocity.length_squared().is_finite()
        || (state.orientation.length() - 1.0).abs() > 1e-9
    {
        return Err("state has nonfinite magnitudes or a nonunit model attitude".into());
    }
    Ok(())
}

pub(super) fn validate_jet_sources(startup: &Startup) -> Result<(), String> {
    validate_model_sources(startup)
}
pub(super) fn validate_jet_state(state: &RigidBodyState) -> Result<(), String> {
    validate_model_state(state)
}

pub(super) fn guidance_for(
    profile: &crate::aircraft_profile::SelectedAircraftProfile,
) -> flightsim_ui::FlightGuidance {
    match profile {
        crate::aircraft_profile::SelectedAircraftProfile::Jet(_) => jet_guidance(),
        crate::aircraft_profile::SelectedAircraftProfile::Turboprop(_) => {
            crate::turboprop_session::guidance()
        }
        crate::aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(_) => {
            crate::near_static_turboprop_session::guidance()
        }
        crate::aircraft_profile::SelectedAircraftProfile::Legacy(_) => {
            flightsim_ui::FlightGuidance::default()
        }
    }
}

/// Read and validate the complete v4 player before changing startup selection.
pub(super) fn resolve_jet_sources(
    startup: &mut Startup,
    diagnostics: &mut crate::StartupDiagnostics,
) -> Result<Option<JetReplayPlayer>, String> {
    validate_jet_sources(startup)?;
    let Some(path) = startup.replay.as_ref() else {
        crate::resolve_airport_database(startup, diagnostics);
        crate::apply_difficulty(startup);
        return Ok(None);
    };
    if startup.weather.was_given || startup.weather.seed_was_given || startup.clouds_were_given {
        return Err(
            "recorded replay weather is authoritative; remove weather and manual cloud overrides"
                .into(),
        );
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let recording = match flightsim_sim::replay_v4::ModelReplayFile::read_from(
        &mut std::io::BufReader::new(file),
    )
    .map_err(|error| crate::replay_policy::read_error(&error))?
    {
        flightsim_sim::replay_v4::ModelReplayFile::V4(recording) => recording,
        flightsim_sim::replay_v4::ModelReplayFile::Existing(recording) => {
            return Err(crate::replay_policy::profile_requirement(
                recording.format_version(),
            ));
        }
    };
    let environment = recording.conditions().environment;
    if let JetTerrain::Flat { elevation } = environment.terrain
        && (startup.world.global_terrain || elevation != Meters::ZERO)
    {
        return Err("flat jet replay requires explicit --global-terrain off and recorded flat elevation zero".into());
    }
    validate_jet_state(&recording.conditions().initial_state)?;
    let player = JetReplayPlayer::new(
        startup.aircraft.jet().unwrap().configuration().clone(),
        recording,
    )
    .map_err(|e| e.to_string())?;
    let conditions = environment.conditions;
    let mut candidate = startup.clone();
    candidate.start = conditions.start;
    candidate.heading = conditions.heading;
    candidate.start_was_explicit = true;
    candidate.heading_was_explicit = true;
    candidate.wind = conditions.wind;
    candidate.turbulence = conditions.turbulence;
    candidate.time_rate = conditions.time_rate;
    candidate.world.global_terrain = conditions.world_terrain;
    candidate.world.climate_enabled = conditions.climate_date.is_some();
    if let Some(date) = conditions.climate_date {
        candidate.world.climate_date = date;
    }
    candidate.weather.selection = environment.weather;
    candidate.world.fly_height = None;
    candidate.approach = None;
    candidate.drop_height = None;
    crate::resolve_airport_database(&mut candidate, diagnostics);
    *startup = candidate;
    Ok(Some(player))
}

pub(super) fn prepare_startup(
    startup: &Startup,
    clock: &flightsim_render::TimeOfDay,
    player: Option<JetReplayPlayer>,
) -> Result<(FlightSession, StartCondition), String> {
    if let Some(player) = player {
        let environment = player.recording().conditions().environment;
        if crate::replay_visual_epoch(
            clock.utc,
            player.recording().duration(),
            environment.conditions.time_rate,
        )
        .is_none()
        {
            return Err("resolved jet replay visual clock exceeds its supported range".into());
        }
        let state = *player.simulation().state();
        return Ok((
            FlightSession::replay(player),
            StartCondition::InFlight(state),
        ));
    }
    let mut terrain = flightsim_world::Terrain::new(
        crate::make_source(startup),
        8 * 1024 * 1024,
        crate::world_runtime::terrain_levels(startup),
    );
    let start = if let Some(height) = startup.world.fly_height {
        StartCondition::InFlight(crate::world_runtime::airborne_state(
            startup,
            &mut terrain,
            height,
        ))
    } else if let Some(miles) = startup.approach {
        let ground = flightsim_sim::GroundSampler::default()
            .sample(&mut terrain, startup.runway.threshold)
            .elevation;
        StartCondition::InFlight(startup.aircraft.approach_state(
            &startup.runway.with_elevation(ground),
            flightsim_core::NauticalMiles(miles).to_meters(),
            flightsim_core::Degrees(3.0).to_radians(),
        ))
    } else if let Some(height) = startup.drop_height {
        let ground = flightsim_sim::GroundSampler::default()
            .sample(&mut terrain, startup.start)
            .elevation;
        StartCondition::InFlight(RigidBodyState::from_geodetic(
            Geodetic::new(
                startup.start.latitude,
                startup.start.longitude,
                Meters(ground.get() + height),
            ),
            flightsim_core::Attitude::new(Radians::ZERO, Radians::ZERO, startup.heading),
            flightsim_core::Ned::new(0.0, 0.0, 0.0),
        ))
    } else {
        StartCondition::Parked {
            position: startup.start,
            heading: startup.heading,
        }
    };
    Ok((FlightSession::prepare_jet(startup, clock, start)?, start))
}

pub(super) fn save_jet_recording(
    recording: &flightsim_sim::replay_v4::JetRecording,
) -> Result<std::path::PathBuf, String> {
    // Empty initial state and terminal-at-zero are both meaningful v4 snapshots.
    let (path, file) =
        crate::create_replay_file(std::path::Path::new(".")).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    recording.write_to(&mut writer).map_err(|e| e.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(super) fn jet_guidance() -> flightsim_ui::FlightGuidance {
    flightsim_ui::FlightGuidance { tutorial_enabled: false, compact_live_help: Some("W/S pitch; A/D roll; Q/E yaw\nPageUp/Down thrust; F/G flaps\n[/] trim; Space/B brakes\nJ/L roll trim; U/O yaw trim\nShift fine; K reset roll/yaw\nC view; M map; R restart; F9 save\nEsc pause / complete controls".into()), live_help: Some("DRY JET CONTROLS\nW/S pitch  A/D roll  Q/E yaw\nPageUp/= up  PageDown/- down (thrust)\n[ / ] trim  F/G flaps  Space service brake\nB parking brake  C camera\nJ/L roll trim  U/O yaw trim  K reset both\nShift + trim: fine (0.002/s); normal 0.01/s\nEsc pause  R restart  F9 save replay\nM new flight / weather\n\nEngine runs at idle at 0% throttle.\nRelease parking brake before taxi.\nExperimental model; no landing grading.".into()) }
}

pub(super) fn ascii_notice(message: &str) -> String {
    message
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                ' '
            }
        })
        .collect()
}

pub(super) fn jet_notice(session: &FlightSession, startup: Option<&Startup>) -> String {
    let jet = session.jet().expect("jet notice only for jet session");
    let envelope = jet.config().envelope().definition();
    let input = session
        .last_controls()
        .unwrap_or_else(ControlInputs::neutral);
    let engine = if input.throttle() == 0.0 {
        "IDLE"
    } else {
        "DRY THRUST"
    };
    let brake = session.parking_brake().map_or_else(
        || format!("recorded brake {:.0}%", input.brakes() * 100.0),
        |(active, pending)| {
            format!(
                "PARK {} [B]{} | brake {:.0}%",
                if active { "ON" } else { "OFF" },
                if pending { " pending" } else { "" },
                input.brakes() * 100.0
            )
        },
    );
    let recording_note = match session {
        FlightSession::JetLive {
            recording_error: Some(error),
            ..
        } => format!(" | {error}"),
        _ => String::new(),
    };
    let manual = if startup.is_some_and(|s| s.clouds_were_given) {
        " | F9 OFF: manual clouds"
    } else {
        ""
    };
    ascii_notice(&format!(
        "{} | {engine} {:.0}% | {brake} | Mach {:.2}..{:.2}{manual}{recording_note}",
        jet.config().airframe().name(),
        input.throttle() * 100.0,
        envelope.mach[0],
        envelope.mach[1]
    ))
}
