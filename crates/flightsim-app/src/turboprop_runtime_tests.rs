//! Controlled app/session witnesses. Ballistic synthetic data is not Cedar or a
//! qualification of human handling, propulsion fidelity, animation or acoustics.
#![allow(clippy::float_cmp)]
use super::*;
use flightsim_fdm::{ControlInputs, turboprop::TurbopropState};
use flightsim_input::{InputDevices, PilotKeys};
use flightsim_sim::{
    replay_v5::{
        TurbopropRecorder, TurbopropRecording, TurbopropReplayPlayer, snapshot_bits_equal,
        state_bits_equal,
    },
    turboprop_simulation::{TURBOPROP_FIXED_DT, TurbopropSimulation},
};
use turboprop_lifecycle_tests::{recording, start, startup};

fn session() -> FlightSession {
    FlightSession::prepare_bounded(&startup(), &flightsim_render::TimeOfDay::default(), start())
        .unwrap()
}
fn sample(keys: PilotKeys) -> SampledPilotInput {
    SampledPilotInput::from_bindings(
        keys,
        &flightsim_input::InputConfiguration::default(),
        &InputDevices::default(),
    )
}
fn export(session: &FlightSession) -> TurbopropRecording {
    let FlightSession::TurbopropLive { recorder, .. } = session else {
        panic!("live turboprop")
    };
    recorder.export()
}
fn bytes(recording: &TurbopropRecording) -> Vec<u8> {
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    bytes
}
struct RecordingFile(std::path::PathBuf);
impl RecordingFile {
    fn new(recording: &TurbopropRecording) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flightsim-v5-app-{}-{:?}.fsreplay",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, bytes(recording)).unwrap();
        Self(path)
    }
}
impl Drop for RecordingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn exact_engine_start_and_render_bridge_do_not_warm_or_reinterpret_signed_zero() {
    let mut startup = startup();
    let json = startup
        .aircraft
        .turboprop()
        .unwrap()
        .to_json()
        .unwrap()
        .replace("\"turbine_fraction\": 0.4", "\"turbine_fraction\": -0.0");
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
        flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(&json).unwrap(),
    );
    let profile = startup.aircraft.turboprop().unwrap();
    let authored = profile.running_start();
    let session =
        FlightSession::prepare_bounded(&startup, &flightsim_render::TimeOfDay::default(), start())
            .unwrap();
    assert!(!session.is_jet());
    assert!(session.is_turboprop());
    assert!(session.legacy().is_none());
    assert!(session.jet().is_none());
    let StartCondition::InFlight(rigid_body) = start() else {
        unreachable!()
    };
    let expected = TurbopropState {
        rigid_body,
        turbine_fraction: authored.turbine_fraction(),
        shaft_rad_s: authored.shaft_speed(),
        blade_pitch_rad: authored.blade_pitch(),
    };
    let before = session.turboprop().unwrap().snapshot();
    assert!(state_bits_equal(&before.state, &expected));
    assert_eq!(
        before.state.turbine_fraction.get().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert_eq!(*session.state(), rigid_body);
    assert_eq!(session.interpolated().position, rigid_body.position);
    let presentation = session.turboprop().unwrap().presentation();
    assert!(presentation.aero_coefficients.is_some());
    assert_eq!(presentation.aero_angles, session.aero_angles());
    assert_eq!(session.airspeed().get(), 40.0);
    assert!(snapshot_bits_equal(
        &before,
        &session.turboprop().unwrap().snapshot()
    ));
    assert!(state_bits_equal(
        &export(&session).conditions().initial_state,
        &expected
    ));
}

#[test]
fn no_step_keeps_controls_full_state_recorder_and_pending_parking_toggle() {
    let mut session = session();
    let mut pilot = world_runtime::initial_controls(&startup());
    let initial_pilot = pilot;
    let initial = session.turboprop().unwrap().snapshot();
    let old_bytes = bytes(&export(&session));
    session.queue_parking_toggle();
    session.advance_bounded(
        Seconds::ZERO,
        &mut pilot,
        &sample(PilotKeys {
            throttle_up: true,
            aileron_trim_right: true,
            ..default()
        }),
        true,
    );
    assert_eq!(format!("{pilot:?}"), format!("{initial_pilot:?}"));
    assert_eq!(session.parking_brake(), Some((false, true)));
    assert!(snapshot_bits_equal(
        &initial,
        &session.turboprop().unwrap().snapshot()
    ));
    assert_eq!(bytes(&export(&session)), old_bytes);
    session.advance_bounded(TURBOPROP_FIXED_DT, &mut pilot, &sample(default()), true);
    assert_eq!(session.parking_brake(), Some((true, false)));
    assert_eq!(export(&session).controls()[0].brakes(), 1.0);
}

fn run_cadence(fps: u32) -> (FlightSession, PilotControls) {
    let mut session = session();
    let mut pilot = world_runtime::initial_controls(&startup());
    pilot.throttle.set_absolute(0.4);
    // All cadence boundaries share the same held command and physical interval.
    let sample = sample(PilotKeys {
        aileron_trim_right: true,
        rudder_trim_right: true,
        lateral_trim_fine: true,
        ..default()
    });
    for _ in 0..fps {
        session.advance_bounded(Seconds(1.0 / f64::from(fps)), &mut pilot, &sample, true);
    }
    (session, pilot)
}

#[test]
fn full_state_and_effective_lateral_trim_recording_are_cadence_invariant() {
    let (baseline, baseline_pilot) = run_cadence(60);
    let record = export(&baseline);
    assert_eq!(record.controls().len(), 120);
    assert!(record.terminal().is_none());
    assert!(baseline_pilot.aileron_trim.value() > 0.0019);
    assert!(baseline_pilot.rudder_trim.value() > 0.0019);
    assert_eq!(
        record.controls().last().unwrap().aileron(),
        baseline_pilot.effective_aileron()
    );
    assert_eq!(
        record.controls().last().unwrap().rudder(),
        baseline_pilot.effective_rudder()
    );
    for fps in [30, 120, 144] {
        let (other, pilot) = run_cadence(fps);
        assert_eq!(bytes(&record), bytes(&export(&other)), "{fps} Hz");
        assert_eq!(format!("{pilot:?}"), format!("{baseline_pilot:?}"));
        assert!(snapshot_bits_equal(
            &baseline.turboprop().unwrap().snapshot(),
            &other.turboprop().unwrap().snapshot()
        ));
    }
    let config = startup()
        .aircraft
        .turboprop()
        .unwrap()
        .configuration()
        .clone();
    let mut replay = TurbopropReplayPlayer::new(config, record).unwrap();
    for _ in 0..4 {
        replay.advance(Seconds(0.25)).unwrap();
    }
    assert!(state_bits_equal(
        replay.simulation().state(),
        baseline.turboprop().unwrap().state()
    ));
    replay.seek_to(0).unwrap();
    assert_eq!(replay.cursor(), 0);
    for _ in 0..4 {
        replay.advance(Seconds(0.25)).unwrap();
    }
    assert!(state_bits_equal(
        replay.simulation().state(),
        baseline.turboprop().unwrap().state()
    ));
}

#[test]
fn rejected_full_state_step_keeps_controller_trim_toggle_clock_and_exportable_terminal() {
    let seed = session();
    let sim = seed.turboprop().unwrap();
    let config = sim.config().clone();
    let mut environment = sim.environment();
    // A forged unsupported wind produces a real domain terminal without
    // changing the valid complete state or narrowing the configuration.
    environment.conditions.wind.speed = flightsim_core::MetersPerSecond(1000.0);
    let simulation =
        TurbopropSimulation::from_state(config.clone(), *sim.state(), environment).unwrap();
    let recorder = TurbopropRecorder::new(&simulation).unwrap();
    let initial = simulation.snapshot();
    let mut session = FlightSession::TurbopropLive {
        simulation,
        recorder,
        parking_brake: false,
        pending_parking_toggle: true,
        last_controls: ControlInputs::neutral(),
        recording_error: None,
        fault: None,
    };
    let mut pilot = world_runtime::initial_controls(&startup());
    let initial_pilot = pilot;
    session.advance_bounded(
        TURBOPROP_FIXED_DT,
        &mut pilot,
        &sample(PilotKeys {
            aileron_trim_right: true,
            throttle_up: true,
            ..default()
        }),
        true,
    );
    assert!(session.turboprop().unwrap().terminal().is_some());
    assert!(snapshot_bits_equal(
        &initial,
        &session.turboprop().unwrap().snapshot()
    ));
    assert_eq!(format!("{pilot:?}"), format!("{initial_pilot:?}"));
    assert_eq!(session.parking_brake(), Some((false, true)));
    assert!(session.audio_paused());
    assert!(
        session
            .terminal_message()
            .unwrap()
            .starts_with("TURBOPROP STOPPED")
    );
    let saved = export(&session);
    assert_eq!(saved.controls().len(), 0);
    assert!(saved.terminal().is_some());
    let player = TurbopropReplayPlayer::new(config, saved).unwrap();
    assert!(player.finished());
    assert!(state_bits_equal(
        player.simulation().state(),
        &initial.state
    ));
}

#[test]
fn nonconsuming_v5_exports_keep_all_engine_scalars_and_continue_the_same_recorder() {
    let mut session = session();
    let mut pilot = world_runtime::initial_controls(&startup());
    let empty = export(&session);
    assert_eq!(empty.controls().len(), 0);
    assert_eq!(bytes(&empty), bytes(&export(&session)));
    for count in [1, 2] {
        session.advance_bounded(TURBOPROP_FIXED_DT, &mut pilot, &sample(default()), true);
        let record = export(&session);
        assert_eq!(record.controls().len(), count);
        let decoded = TurbopropRecording::read_from(&mut bytes(&record).as_slice()).unwrap();
        assert!(state_bits_equal(
            decoded.final_state(),
            session.turboprop().unwrap().state()
        ));
        assert_eq!(bytes(&record), bytes(&export(&session)));
    }
}

#[test]
fn v5_source_admission_restores_recorded_fields_and_rejects_mismatch_without_mutation() {
    let record = recording(3);
    let file = RecordingFile::new(&record);
    let mut startup = startup();
    let mut profile: serde_json::Value =
        serde_json::from_str(&startup.aircraft.turboprop().unwrap().to_json().unwrap()).unwrap();
    profile["dynamics"]["running_start"] = serde_json::json!({
        "turbine_fraction": 0.3, "shaft_rad_s": 175.0, "blade_pitch_rad": 0.31,
    });
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
        flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(&profile.to_string()).unwrap(),
    );
    assert_ne!(
        startup
            .aircraft
            .turboprop()
            .unwrap()
            .running_start()
            .turbine_fraction(),
        record.conditions().initial_state.turbine_fraction
    );
    assert_ne!(
        startup
            .aircraft
            .turboprop()
            .unwrap()
            .running_start()
            .shaft_speed(),
        record.conditions().initial_state.shaft_rad_s
    );
    assert_ne!(
        startup
            .aircraft
            .turboprop()
            .unwrap()
            .running_start()
            .blade_pitch(),
        record.conditions().initial_state.blade_pitch_rad
    );
    startup.replay = Some(file.0.clone());
    startup.wind = flightsim_sim::Wind {
        from: Radians(0.23),
        speed: flightsim_core::MetersPerSecond(12.3),
    };
    startup.world.climate_enabled = true;
    startup.world.fly_height = Some(Meters(444.0));
    startup.approach = Some(1.5);
    startup.drop_height = Some(1200.0);
    let player = turboprop_session::resolve_sources(&mut startup, &mut default())
        .unwrap()
        .unwrap();
    let env = record.conditions().environment;
    assert_eq!(startup.start, env.conditions.start);
    assert_eq!(startup.heading, env.conditions.heading);
    assert_eq!(startup.wind, env.conditions.wind);
    assert_eq!(startup.turbulence, env.conditions.turbulence);
    assert_eq!(startup.time_rate, env.conditions.time_rate);
    assert_eq!(startup.weather.selection, env.weather);
    assert_eq!(
        startup.world.climate_enabled,
        env.conditions.climate_date.is_some()
    );
    assert!(
        startup.approach.is_none()
            && startup.drop_height.is_none()
            && startup.world.fly_height.is_none()
    );
    assert!(state_bits_equal(
        player.simulation().state(),
        &record.conditions().initial_state
    ));
    // An exact-identity mismatch may not silently replace metadata or physical state.
    let mut altered: serde_json::Value =
        serde_json::from_str(&startup.aircraft.turboprop().unwrap().to_json().unwrap()).unwrap();
    let torque = altered["dynamics"]["turbine"]["output_torque_limit_nm"]
        .as_f64()
        .unwrap();
    altered["dynamics"]["turbine"]["output_torque_limit_nm"] = (torque * 1.01).into();
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
        flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(&altered.to_string()).unwrap(),
    );
    let before = format!("{startup:?}");
    assert!(turboprop_session::resolve_sources(&mut startup, &mut default()).is_err());
    assert_eq!(format!("{startup:?}"), before);
}

#[test]
fn v5_overrides_and_unsupported_sources_fail_before_startup_mutation() {
    let file = RecordingFile::new(&recording(0));
    for case in 0..6 {
        let mut startup = startup();
        startup.replay = Some(file.0.clone());
        match case {
            0 => startup.weather.was_given = true,
            1 => startup.weather.seed_was_given = true,
            2 => startup.clouds_were_given = true,
            3 => startup.tiles = Some("raw".into()),
            4 => startup.regions.select = Some("fixture@1".into()),
            5 => startup.world.global_terrain = true,
            _ => unreachable!(),
        }
        let before = format!("{startup:?}");
        assert!(
            turboprop_session::resolve_sources(&mut startup, &mut default()).is_err(),
            "case {case}"
        );
        assert_eq!(format!("{startup:?}"), before);
    }
}

#[test]
fn telemetry_and_help_name_modeled_state_and_explicit_presentation_limits() {
    let session = session();
    let notice = turboprop_session::notice(&session, Some(&startup()));
    assert!(notice.contains("modeled turbine x 0.400"));
    assert!(notice.contains("shaft 180.00 rad/s"));
    assert!(notice.contains("blade"));
    assert!(notice.contains("static rotor / synthetic audio"));
    let help = turboprop_session::guidance().live_help.unwrap();
    let compact = turboprop_session::guidance().compact_live_help.unwrap();
    assert!(compact.is_ascii());
    for key in [
        "W/S",
        "A/D",
        "Q/E",
        "PageUp/Down",
        "F/G",
        "[/]",
        "J/L",
        "U/O",
        "Shift",
        "K reset",
        "Space/B",
        "C view",
        "M map",
        "R restart",
        "F9",
        "Esc",
    ] {
        assert!(
            compact.contains(key),
            "compact turboprop reference lost {key}"
        );
    }
    assert!(help.contains("not N1"));
    assert!(help.contains("J/L roll trim") && help.contains("U/O yaw trim"));
    assert!(help.contains("Shift") && help.contains("K reset both"));
}

#[test]
fn v3_cli_keeps_exact_family_and_disables_afterburner_for_synthetic_sound_override() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
    );
    for sound in ["piston", "turbine"] {
        let (startup, notes) = parse_arguments_from(
            ["--aircraft", path, "--no-model", "--engine", sound].map(str::to_owned),
        );
        assert!(notes.0.is_empty(), "{:?}", notes.0);
        assert!(startup.aircraft.is_turboprop());
        assert!(!startup.aircraft.is_jet());
        assert!(startup.model.is_none());
        if let flightsim_audio::EngineKind::Turbine(spec) = startup.engine_sound {
            assert_eq!(spec.afterburner_threshold, 1.0);
            assert_eq!(spec.afterburner_exhaust_speed, spec.military_exhaust_speed);
        }
    }
}

#[test]
fn v5_restores_exact_nondefault_wind_climate_weather_and_restarts_from_recorded_state() {
    use flightsim_sim::{
        turboprop_simulation::{TurbopropEnvironment, TurbopropTerrain},
        weather::{WeatherPreset, WeatherScenario, WeatherSelection},
    };
    let config = startup()
        .aircraft
        .turboprop()
        .unwrap()
        .configuration()
        .clone();
    let initial = *session().turboprop().unwrap().state();
    let mut env = TurbopropEnvironment::default();
    env.terrain = TurbopropTerrain::BundledGlobal;
    env.conditions = env.conditions.with_world_climate(
        true,
        Some(flightsim_world::ClimateDate::from_annual_phase(0.25).unwrap()),
    );
    env.conditions.wind = flightsim_sim::Wind {
        from: Radians(1.234_567_890_123_456_7),
        speed: flightsim_core::MetersPerSecond(1.0),
    };
    env.conditions.turbulence = flightsim_fdm::Turbulence::light(u64::MAX - 7);
    env.conditions.start_epoch = 2_451_545.125;
    env.conditions.time_rate = 3.0;
    env.weather = WeatherSelection::Modeled(
        WeatherScenario::from_preset(WeatherPreset::Rain, initial.rigid_body.geodetic(), 456)
            .unwrap(),
    );
    let sim =
        TurbopropSimulation::from_supported_state(config, initial, env, ControlInputs::neutral())
            .unwrap();
    let record = TurbopropRecorder::new(&sim).unwrap().finish();
    let file = RecordingFile::new(&record);
    let mut startup = startup();
    startup.replay = Some(file.0.clone());
    let player = turboprop_session::resolve_sources(&mut startup, &mut default())
        .unwrap()
        .unwrap();
    assert_eq!(
        startup.wind.from.get().to_bits(),
        env.conditions.wind.from.get().to_bits()
    );
    assert_eq!(startup.turbulence, env.conditions.turbulence);
    assert_eq!(startup.weather.selection, env.weather);
    assert_eq!(startup.world.climate_date(), env.conditions.climate_date);
    assert!(startup.world.global_terrain);
    let expected_atmosphere = flightsim_sim::climate_atmosphere_sample(
        initial.rigid_body.geodetic(),
        env.conditions.climate_date,
    )
    .unwrap();
    assert_eq!(player.simulation().atmosphere_sample(), expected_atmosphere);
    let (mut session, _) = turboprop_session::prepare_startup(
        &startup,
        &flightsim_render::TimeOfDay::default(),
        Some(player),
    )
    .unwrap();
    let FlightSession::TurbopropReplay { player, .. } = &mut session else {
        unreachable!()
    };
    player.restart().unwrap();
    assert!(state_bits_equal(player.simulation().state(), &initial));
    assert_eq!(
        player.simulation().environment(),
        env_with_recorded_start(env, initial)
    );
}

fn env_with_recorded_start(
    mut env: flightsim_sim::turboprop_simulation::TurbopropEnvironment,
    state: TurbopropState,
) -> flightsim_sim::turboprop_simulation::TurbopropEnvironment {
    env.conditions.start = state.rigid_body.geodetic();
    env.conditions.heading = state.rigid_body.attitude().yaw;
    env
}

#[test]
fn v5_recorder_continuity_failure_closes_prefix_without_freezing_live_flight() {
    let mut session = session();
    let mut altered = startup();
    let profile = altered
        .aircraft
        .turboprop()
        .unwrap()
        .to_json()
        .unwrap()
        .replace("\"turbine_fraction\": 0.4", "\"turbine_fraction\": 0.3");
    altered.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
        flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(&profile).unwrap(),
    );
    let FlightSession::TurbopropLive {
        recorder: other, ..
    } = FlightSession::prepare_bounded(&altered, &flightsim_render::TimeOfDay::default(), start())
        .unwrap()
    else {
        unreachable!()
    };
    let old_prefix = bytes(&other.export());
    let FlightSession::TurbopropLive { recorder, .. } = &mut session else {
        unreachable!()
    };
    *recorder = other;
    let mut pilot = world_runtime::initial_controls(&startup());
    for _ in 0..2 {
        session.advance_bounded(TURBOPROP_FIXED_DT, &mut pilot, &sample(default()), true);
    }
    let FlightSession::TurbopropLive {
        recorder,
        recording_error,
        simulation,
        ..
    } = &session
    else {
        unreachable!()
    };
    assert!(
        recording_error
            .as_ref()
            .unwrap()
            .contains("RECORDING STOPPED")
    );
    assert!(recorder.closed());
    assert_eq!(bytes(&recorder.export()), old_prefix);
    assert_eq!(simulation.snapshot().committed_steps, 2);
    assert!(simulation.terminal().is_none());
}
