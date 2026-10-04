//! Explicit opt-in running-turboprop app construction, replay admission and help.
//! The profile owns authored running engine values; replay owns all 16 recorded
//! state scalars. Neither path guesses trim, warms the engine or moves a rotor.
use crate::flight_session::{ascii_notice, validate_model_sources, validate_model_state};
use crate::{FlightSession, StartCondition, Startup};
use flightsim_core::{Geodetic, Meters, Radians};
use flightsim_fdm::{ControlInputs, RigidBodyState, turboprop::TurbopropState};
use flightsim_sim::{
    replay_v5::{TurbopropRecorder, TurbopropRecording, TurbopropReplayPlayer},
    turboprop_simulation::{TurbopropEnvironment, TurbopropSimulation, TurbopropTerrain},
};

pub(super) fn prepare(
    startup: &Startup,
    clock: &flightsim_render::TimeOfDay,
    start: StartCondition,
) -> Result<FlightSession, String> {
    crate::distribution::validate_profile_start(&startup.aircraft)?;
    validate_model_sources(startup)?;
    let profile = startup
        .aircraft
        .turboprop()
        .ok_or("turboprop profile required")?;
    let config = profile.configuration().clone();
    let engine = profile.running_start();
    let mut conditions = crate::environment_conditions(startup, clock);
    conditions.time_rate = clock.rate.get();
    let environment = TurbopropEnvironment {
        conditions,
        terrain: if startup.world.global_terrain {
            TurbopropTerrain::BundledGlobal
        } else {
            TurbopropTerrain::Flat {
                elevation: Meters::ZERO,
            }
        },
        weather: startup.weather.selection,
    };
    let parking_brake = matches!(start, StartCondition::Parked { .. });
    let input = crate::world_runtime::initial_controls(startup).to_control_inputs();
    let input = input.with_brakes(input.brakes().max(if parking_brake { 1.0 } else { 0.0 }));
    let complete = match start {
        StartCondition::Parked { position, heading } => {
            *TurbopropSimulation::parked(config.clone(), position, heading, environment, engine)
                .map_err(|error| error.to_string())?
                .state()
        }
        StartCondition::InFlight(rigid_body) => TurbopropState {
            rigid_body,
            turbine_fraction: engine.turbine_fraction(),
            shaft_rad_s: engine.shaft_speed(),
            blade_pitch_rad: engine.blade_pitch(),
        },
    };
    // Evaluate the actual body, engine, controls and environment at zero duration.
    // The supported-start constructor preserves every authored state bit.
    let simulation =
        TurbopropSimulation::from_supported_state(config, complete, environment, input)
            .map_err(|error| error.to_string())?;
    validate_model_state(&simulation.state().rigid_body)?;
    let recorder = TurbopropRecorder::new(&simulation).map_err(|error| error.to_string())?;
    Ok(FlightSession::TurbopropLive {
        simulation,
        recorder,
        parking_brake,
        pending_parking_toggle: false,
        last_controls: input,
        recording_error: None,
        fault: None,
    })
}

pub(super) fn resolve_sources(
    startup: &mut Startup,
    diagnostics: &mut crate::StartupDiagnostics,
) -> Result<Option<TurbopropReplayPlayer>, String> {
    crate::distribution::validate_profile_start(&startup.aircraft)?;
    validate_model_sources(startup)?;
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
    let recording = TurbopropRecording::read_from(&mut std::io::BufReader::new(file))
        .map_err(|error| format!("profile-v3 turboprop requires matching replay v5: {error}"))?;
    let environment = recording.conditions().environment;
    if let TurbopropTerrain::Flat { elevation } = environment.terrain
        && (startup.world.global_terrain || elevation != Meters::ZERO)
    {
        return Err("flat turboprop replay requires explicit --global-terrain off and recorded flat elevation zero".into());
    }
    validate_model_state(&recording.conditions().initial_state.rigid_body)?;
    let player = TurbopropReplayPlayer::new(
        startup
            .aircraft
            .turboprop()
            .unwrap()
            .configuration()
            .clone(),
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
    player: Option<TurbopropReplayPlayer>,
) -> Result<(FlightSession, StartCondition), String> {
    crate::distribution::validate_profile_start(&startup.aircraft)?;
    if let Some(player) = player {
        let environment = player.recording().conditions().environment;
        if crate::replay_visual_epoch(
            clock.utc,
            player.recording().duration(),
            environment.conditions.time_rate,
        )
        .is_none()
        {
            return Err(
                "resolved turboprop replay visual clock exceeds its supported range".into(),
            );
        }
        let state = player.simulation().state().rigid_body;
        return Ok((
            FlightSession::replay_turboprop(player),
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
    Ok((prepare(startup, clock, start)?, start))
}

pub(super) fn save_recording(
    recording: &flightsim_sim::replay_v5::TurbopropRecording,
) -> Result<std::path::PathBuf, String> {
    // Empty initial state and terminal-at-zero are both meaningful v5 snapshots.
    let (path, file) =
        crate::create_replay_file(std::path::Path::new(".")).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    recording.write_to(&mut writer).map_err(|e| e.to_string())?;
    std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(super) fn guidance() -> flightsim_ui::FlightGuidance {
    flightsim_ui::FlightGuidance { tutorial_enabled: false, compact_live_help: Some("W/S pitch; A/D roll; Q/E yaw\nPageUp/Down power; F/G flaps\n[/] pitch trim; J/L roll; U/O yaw\nShift fine; K reset roll/yaw\nSpace/B brakes; C view; M map\nR restart; F9 save replay\nEsc pause / complete controls".into()), live_help: Some(
        "RUNNING TURBOPROP CONTROLS\nW/S pitch  A/D roll  Q/E yaw\nPageUp/= up  PageDown/- down (power)\n[ / ] elevator trim  F/G flaps\nJ/L roll trim  U/O yaw trim  K reset both\nShift + trim: fine (0.002/s); normal 0.01/s\nSpace service brake  B parking brake\nC camera  Esc pause  R restart\nF9 save replay  M new flight / weather\n\n0% power is running idle, not shutdown.\nTurbine response x is modeled, not N1.\nShaft speed and blade pitch evolve physically.\nNo automatic roll/yaw trim or attitude hold.\nExperimental numerical model; no landing grading.\nModel rotor is static; audio is synthetic.".into()) }
}

pub(super) fn notice(session: &FlightSession, startup: Option<&Startup>) -> String {
    let simulation = session
        .turboprop()
        .expect("turboprop notice requires its family");
    let state = simulation.state();
    let input = session
        .last_controls()
        .unwrap_or_else(ControlInputs::neutral);
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
        FlightSession::TurbopropLive {
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
        "{} | power {:.0}% | modeled turbine x {:.3} | shaft {:.2} rad/s | blade {:.2} deg | {brake} | EXPERIMENTAL; static rotor / synthetic audio{manual}{recording_note}",
        simulation.config().airframe().name(),
        input.throttle() * 100.0,
        state.turbine_fraction.get(),
        state.shaft_rad_s.get(),
        state.blade_pitch_rad.to_degrees().get(),
    ))
}
