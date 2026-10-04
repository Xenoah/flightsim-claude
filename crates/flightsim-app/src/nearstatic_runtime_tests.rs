//! Exact profile-4/law-2 app ownership and v6 lifecycle witnesses, using only
//! explicit numerical fixtures. These tests make no aircraft handling claim.
#![cfg(not(feature = "commercial-staging"))]
#![allow(clippy::float_cmp)]
use super::*;
use crate::aircraft_picker_runtime::nearstatic_lifecycle_tests::{
    recording, runtime_startup as startup, start,
};
use bevy::math::DVec3;
use flightsim_fdm::{ControlInputs, turboprop::TurbopropState};
use flightsim_input::{InputDevices, PilotKeys};
use flightsim_sim::{
    aircraft_profile_v4::AircraftProfileV4,
    near_static_turboprop_simulation::{
        NEAR_STATIC_TURBOPROP_FIXED_DT, NearStaticTurbopropEnvironment,
        NearStaticTurbopropSimulation, NearStaticTurbopropTerrain,
    },
    replay_v6::{
        NearStaticTurbopropRecorder, NearStaticTurbopropRecording, NearStaticTurbopropReplayPlayer,
        snapshot_bits_equal, state_bits_equal,
    },
    weather::{WeatherPreset, WeatherScenario, WeatherSelection},
};
use flightsim_ui::world_map::{WorldMapActions, WorldMapState};
use std::time::Duration;

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
fn export(session: &FlightSession) -> NearStaticTurbopropRecording {
    let FlightSession::NearStaticTurbopropLive { recorder, .. } = session else {
        panic!("explicit law-2 live session required")
    };
    recorder.export()
}
fn bytes(record: &NearStaticTurbopropRecording) -> Vec<u8> {
    let mut bytes = Vec::new();
    record.write_to(&mut bytes).unwrap();
    bytes
}
struct RecordingFile(PathBuf);
impl RecordingFile {
    fn new(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flightsim-v6-app-{}-{:?}.fsreplay",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}
impl Drop for RecordingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn app() -> App {
    let startup = startup();
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(world_runtime::initial_controls(&startup))
        .insert_resource(startup)
        .insert_resource(start())
        .insert_resource(flightsim_render::TimeOfDay::default())
        .insert_resource(FlightSimulation(session()))
        .insert_resource(flightsim_audio::SoundBridge(std::sync::Arc::new(
            flightsim_audio::SharedSound::default(),
        )))
        .init_resource::<SampledPilotInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<CameraRig>()
        .init_resource::<flightsim_ui::Paused>()
        .init_resource::<flightsim_ui::TutorialState>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .init_resource::<flightsim_ui::ReplayStatus>()
        .init_resource::<flightsim_ui::CrashNotice>()
        .init_resource::<world_runtime::MapCapture>()
        .init_resource::<scenery_runtime::SceneryReset>()
        .init_resource::<JetHudReset>()
        .add_systems(
            Update,
            (
                adjust_time_rate,
                control_flight,
                control_replay,
                advance_simulation.run_if(world_runtime::flight_controls_active),
                sync_replay_clock,
                publish_crash,
                publish_replay_status,
                report_landings,
            )
                .chain(),
        );
    configure_live_input_scheduling(&mut app);
    app
}
fn tick(app: &mut App, key: Option<KeyCode>, dt: Duration) {
    let mut keyboard = ButtonInput::default();
    if let Some(key) = key {
        keyboard.press(key);
    }
    app.insert_resource(keyboard);
    app.world_mut().resource_mut::<Time>().advance_by(dt);
    app.update();
}

#[test]
fn exact_authored_full_state_and_signed_zero_are_not_warmed_or_reinterpreted() {
    let mut startup = startup();
    let text = startup
        .aircraft
        .near_static_turboprop()
        .unwrap()
        .to_json()
        .unwrap();
    assert!(text.contains("\"turbine_fraction\": 0.4"));
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(
        AircraftProfileV4::parse(
            &text.replace("\"turbine_fraction\": 0.4", "\"turbine_fraction\": -0.0"),
        )
        .unwrap(),
    );
    let engine = startup
        .aircraft
        .near_static_turboprop()
        .unwrap()
        .running_start();
    let StartCondition::InFlight(rigid_body) = start() else {
        unreachable!()
    };
    let expected = TurbopropState {
        rigid_body,
        turbine_fraction: engine.turbine_fraction(),
        shaft_rad_s: engine.shaft_speed(),
        blade_pitch_rad: engine.blade_pitch(),
    };
    let mut session =
        FlightSession::prepare_bounded(&startup, &flightsim_render::TimeOfDay::default(), start())
            .unwrap();
    assert!(session.is_turboprop());
    assert!(session.turboprop().is_none());
    assert!(session.jet().is_none() && session.legacy().is_none());
    let before = session.near_static_turboprop().unwrap().snapshot();
    assert!(state_bits_equal(&before.state, &expected));
    assert_eq!(
        before.state.turbine_fraction.get().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert_eq!(*session.state(), rigid_body);
    assert_eq!(session.interpolated().position, rigid_body.position);
    assert!(
        session
            .near_static_turboprop()
            .unwrap()
            .presentation()
            .aero_coefficients
            .is_some()
    );
    assert_eq!(session.airspeed().get(), 40.0);
    let accumulated = session.near_static_turboprop().unwrap().accumulated();
    for _ in 0..10 {
        let read = session.near_static_turboprop().unwrap().presentation();
        assert_eq!(read.aero_angles, session.aero_angles());
        assert_eq!(read.pose.position, rigid_body.position);
        assert_eq!(read.ground, session.ground());
        assert_eq!(
            session.near_static_turboprop().unwrap().accumulated(),
            accumulated
        );
    }
    assert!(snapshot_bits_equal(
        &before,
        &session.near_static_turboprop().unwrap().snapshot()
    ));
    assert!(state_bits_equal(
        &export(&session).conditions().initial_state,
        &expected
    ));
    let mut pilot = world_runtime::initial_controls(&startup);
    pilot.aileron_trim.set(0.01234);
    pilot.rudder_trim.set(-0.02345);
    let controls = format!("{pilot:?}");
    let prefix = bytes(&export(&session));
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
    assert_eq!(format!("{pilot:?}"), controls);
    assert_eq!(session.parking_brake(), Some((false, true)));
    assert!(snapshot_bits_equal(
        &before,
        &session.near_static_turboprop().unwrap().snapshot()
    ));
    assert_eq!(bytes(&export(&session)), prefix);
}

#[test]
fn v6_cadence_and_effective_trim_match_and_each_export_keeps_the_recorder_live() {
    let mut baseline = None;
    for fps in [60, 30, 120, 144] {
        let mut session = session();
        let mut pilot = world_runtime::initial_controls(&startup());
        pilot.throttle.set_absolute(0.4);
        let keys = sample(PilotKeys {
            aileron_trim_right: true,
            rudder_trim_right: true,
            lateral_trim_fine: true,
            ..default()
        });
        for _ in 0..fps {
            session.advance_bounded(Seconds(1.0 / f64::from(fps)), &mut pilot, &keys, true);
        }
        let record = export(&session);
        assert_eq!(record.controls().len(), 120);
        assert!(record.terminal().is_none());
        assert!(pilot.aileron_trim.value() > 0.0019 && pilot.rudder_trim.value() > 0.0019);
        assert_eq!(
            record.controls().last().unwrap().aileron(),
            pilot.effective_aileron()
        );
        assert_eq!(
            record.controls().last().unwrap().rudder(),
            pilot.effective_rudder()
        );
        let encoded = bytes(&record);
        assert_eq!(u16::from_le_bytes(encoded[8..10].try_into().unwrap()), 6);
        assert_eq!(record.conditions().identity.schema, 4);
        assert_eq!(record.conditions().identity.law_revision, 2);
        let snapshot = session.near_static_turboprop().unwrap().snapshot();
        if let Some((expected, expected_bytes, controls)) = &baseline {
            assert!(snapshot_bits_equal(expected, &snapshot), "{fps} Hz");
            assert_eq!(expected_bytes, &encoded, "{fps} Hz");
            assert_eq!(controls, &format!("{pilot:?}"));
        } else {
            baseline = Some((snapshot, encoded.clone(), format!("{pilot:?}")));
        }
        assert_eq!(encoded, bytes(&export(&session)));
        session.advance_bounded(
            NEAR_STATIC_TURBOPROP_FIXED_DT,
            &mut pilot,
            &sample(default()),
            true,
        );
        assert_eq!(export(&session).controls().len(), 121);
        let mut replay = NearStaticTurbopropReplayPlayer::new(
            startup()
                .aircraft
                .near_static_turboprop()
                .unwrap()
                .configuration()
                .clone(),
            record,
        )
        .unwrap();
        while !replay.finished() {
            replay.advance(Seconds(0.25)).unwrap();
        }
        assert!(snapshot_bits_equal(
            &snapshot,
            &replay.simulation().snapshot()
        ));
    }
}

#[test]
fn v6_rewind_restores_engine_clock_and_transport_and_presentation_uses_recorded_controls() {
    let record = recording(481);
    let initial = record.conditions().initial_state;
    let epoch = record.conditions().environment.conditions.start_epoch;
    let mut player = NearStaticTurbopropReplayPlayer::new(
        startup()
            .aircraft
            .near_static_turboprop()
            .unwrap()
            .configuration()
            .clone(),
        record,
    )
    .unwrap();
    while !player.finished() {
        player.advance(Seconds(0.25)).unwrap();
    }
    let final_snapshot = player.simulation().snapshot();
    let mut app = app();
    app.world_mut().resource_mut::<FlightSimulation>().0 =
        FlightSession::replay_near_static_turboprop(player);
    app.world_mut()
        .resource_mut::<PilotControls>()
        .throttle
        .set_absolute(0.99);
    app.world_mut()
        .resource_mut::<PilotControls>()
        .flaps
        .set_absolute(0.75);
    app.init_resource::<StallWarningStatus>()
        .init_resource::<flightsim_audio::AircraftSound>()
        .insert_resource(ViewMode::Cockpit)
        .init_resource::<SunDirection>()
        .init_resource::<HudState>()
        .init_resource::<flightsim_ui::HudSmoothing>()
        .insert_resource(flightsim_ui::TutorialVisibility(true))
        .add_systems(Update, (update_model_visibility, toggle_tutorial));
    configure_flight_presentation(&mut app);
    let exterior = app
        .world_mut()
        .spawn((ExteriorModel, Visibility::Hidden))
        .id();
    tick(&mut app, None, Duration::ZERO);
    assert!(snapshot_bits_equal(
        &final_snapshot,
        &app.world()
            .resource::<FlightSimulation>()
            .0
            .near_static_turboprop()
            .unwrap()
            .snapshot()
    ));
    let sound = app.world().resource::<flightsim_audio::AircraftSound>();
    assert_eq!(sound.throttle, 0.4);
    assert!(sound.muted);
    let hud = app.world().resource::<HudState>();
    assert_eq!(hud.throttle, 0.4);
    assert_eq!(hud.flaps, 0.1);
    assert_eq!(hud.trim, None);
    assert_eq!(
        app.world().get::<Visibility>(exterior),
        Some(&Visibility::Inherited)
    );
    assert!(
        !app.world().contains_resource::<FlightRecorder>()
            && !app
                .world()
                .contains_resource::<flightsim_ui::LandingReport>()
    );
    assert!(!app.world().resource::<flightsim_ui::TutorialVisibility>().0);
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert!(status.active && status.finished);
    assert!(status.notice.as_ref().unwrap().contains("LAW 2"));
    assert!(status.notice.as_ref().unwrap().is_ascii());
    tick(&mut app, Some(KeyCode::F8), Duration::ZERO);
    let session = &app.world().resource::<FlightSimulation>().0;
    assert!(state_bits_equal(
        session.near_static_turboprop().unwrap().state(),
        &initial
    ));
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert_eq!(
        app.world()
            .resource::<flightsim_render::TimeOfDay>()
            .utc
            .get(),
        epoch
    );
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().rate,
        flightsim_render::TimeRate::PAUSED
    );
    tick(&mut app, Some(KeyCode::F5), Duration::from_millis(50));
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.elapsed(),
        Seconds::ZERO
    );
    tick(&mut app, Some(KeyCode::F7), Duration::ZERO);
    assert_eq!(
        app.world().resource::<flightsim_ui::ReplayStatus>().speed,
        2.0
    );
    tick(&mut app, Some(KeyCode::F6), Duration::ZERO);
    assert_eq!(
        app.world().resource::<flightsim_ui::ReplayStatus>().speed,
        1.0
    );
    tick(&mut app, Some(KeyCode::F5), Duration::from_millis(50));
    let elapsed = app.world().resource::<FlightSimulation>().0.elapsed();
    assert!(elapsed > Seconds::ZERO);
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().utc,
        replay_visual_epoch(flightsim_render::JulianDate(epoch), elapsed, 60.0).unwrap()
    );
}

#[test]
fn rejected_law2_step_rolls_back_controls_and_all_state_but_exports_authentic_terminal() {
    let seed = session();
    let sim = seed.near_static_turboprop().unwrap();
    let config = sim.config().clone();
    // Keep the accepted law-2 component and its limits. A real negative-flow
    // transverse-domain terminal exercises the new closed v6 failure family.
    let mut unsupported = *sim.state();
    unsupported.rigid_body.velocity =
        unsupported.rigid_body.orientation * DVec3::new(-0.1, 1.0, 0.0);
    let simulation =
        NearStaticTurbopropSimulation::from_state(config.clone(), unsupported, sim.environment())
            .unwrap();
    let recorder = NearStaticTurbopropRecorder::new(&simulation).unwrap();
    let initial = simulation.snapshot();
    let mut session = FlightSession::NearStaticTurbopropLive {
        simulation,
        recorder,
        parking_brake: false,
        pending_parking_toggle: true,
        last_controls: ControlInputs::neutral(),
        recording_error: None,
        fault: None,
    };
    let mut pilot = world_runtime::initial_controls(&startup());
    pilot.aileron_trim.set(0.01234);
    pilot.rudder_trim.set(-0.02345);
    let controls = format!("{pilot:?}");
    session.advance_bounded(
        NEAR_STATIC_TURBOPROP_FIXED_DT,
        &mut pilot,
        &sample(PilotKeys {
            throttle_up: true,
            aileron_trim_right: true,
            ..default()
        }),
        true,
    );
    assert!(
        session
            .near_static_turboprop()
            .unwrap()
            .terminal()
            .is_some()
    );
    assert!(snapshot_bits_equal(
        &initial,
        &session.near_static_turboprop().unwrap().snapshot()
    ));
    assert_eq!(format!("{pilot:?}"), controls);
    assert_eq!(session.parking_brake(), Some((false, true)));
    assert!(session.audio_paused());
    assert!(session.terminal_message().is_some());
    let saved = export(&session);
    assert!(saved.controls().is_empty() && saved.terminal().is_some());
    assert!(matches!(
        saved.terminal().unwrap().failure.reason,
        flightsim_fdm::turboprop::near_static::TurbopropFailureReason::OutsideNearStaticDomain(_)
    ));
    let decoded = NearStaticTurbopropRecording::read_from(&mut bytes(&saved).as_slice()).unwrap();
    let mut player = NearStaticTurbopropReplayPlayer::new(config, decoded).unwrap();
    assert!(player.finished());
    assert!(snapshot_bits_equal(
        &initial,
        &player.simulation().snapshot()
    ));
    player.restart().unwrap();
    assert!(player.finished());
    assert!(snapshot_bits_equal(
        &initial,
        &player.simulation().snapshot()
    ));
}

#[test]
fn v6_source_resolution_restores_exact_environment_and_rejects_wrong_family_or_overrides() {
    let seed = session();
    let initial = *seed.near_static_turboprop().unwrap().state();
    let mut env = NearStaticTurbopropEnvironment::default();
    env.terrain = NearStaticTurbopropTerrain::BundledGlobal;
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
    let sim = NearStaticTurbopropSimulation::from_supported_state(
        seed.near_static_turboprop().unwrap().config().clone(),
        initial,
        env,
        ControlInputs::neutral(),
    )
    .unwrap();
    let record = NearStaticTurbopropRecorder::new(&sim).unwrap().finish();
    let file = RecordingFile::new(&bytes(&record));
    let mut selected = startup();
    selected.replay = Some(file.0.clone());
    selected.approach = Some(1.5);
    selected.drop_height = Some(1200.0);
    selected.world.fly_height = Some(Meters(444.0));
    let player = near_static_turboprop_session::resolve_sources(&mut selected, &mut default())
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.wind.from.get().to_bits(),
        env.conditions.wind.from.get().to_bits()
    );
    assert_eq!(selected.wind.speed, env.conditions.wind.speed);
    assert_eq!(selected.turbulence, env.conditions.turbulence);
    assert_eq!(selected.weather.selection, env.weather);
    assert_eq!(selected.world.climate_date(), env.conditions.climate_date);
    assert!(
        selected.world.global_terrain
            && selected.approach.is_none()
            && selected.drop_height.is_none()
            && selected.world.fly_height.is_none()
    );
    assert!(state_bits_equal(player.simulation().state(), &initial));
    let (session, _) = near_static_turboprop_session::prepare_startup(
        &selected,
        &flightsim_render::TimeOfDay::default(),
        Some(player),
    )
    .unwrap();
    assert!(session.is_replay());
    assert!(session.turboprop().is_none());
    for case in 0..6 {
        let mut bad = selected.clone();
        match case {
            0 => bad.weather.was_given = true,
            1 => bad.weather.seed_was_given = true,
            2 => bad.clouds_were_given = true,
            3 => bad.tiles = Some("raw".into()),
            4 => bad.regions.select = Some("fixture@1".into()),
            _ => {
                let mut value: serde_json::Value = serde_json::from_str(
                    &bad.aircraft
                        .near_static_turboprop()
                        .unwrap()
                        .to_json()
                        .unwrap(),
                )
                .unwrap();
                value["dynamics"]["turbine"]["output_torque_limit_nm"] = 4900.0.into();
                bad.aircraft = aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(
                    AircraftProfileV4::parse(&value.to_string()).unwrap(),
                );
            }
        }
        let before = format!("{bad:?}");
        assert!(
            near_static_turboprop_session::resolve_sources(&mut bad, &mut default()).is_err(),
            "case {case}"
        );
        assert_eq!(format!("{bad:?}"), before);
    }
    let mut old = crate::turboprop_lifecycle_tests::startup();
    old.replay = Some(file.0.clone());
    let before = format!("{old:?}");
    let error = turboprop_session::resolve_sources(&mut old, &mut default()).unwrap_err();
    assert!(
        error.contains("profile-v4 law-2") && error.contains("--aircraft FILE"),
        "{error}"
    );
    assert!(!error.contains("this build reads"));
    assert_eq!(format!("{old:?}"), before);
    let old_record = crate::turboprop_lifecycle_tests::recording(2);
    let mut old_bytes = Vec::new();
    old_record.write_to(&mut old_bytes).unwrap();
    std::fs::write(&file.0, old_bytes).unwrap();
    let before = format!("{selected:?}");
    let error =
        near_static_turboprop_session::resolve_sources(&mut selected, &mut default()).unwrap_err();
    assert!(
        error.contains("profile-v3 law-1") && error.contains("--aircraft FILE"),
        "{error}"
    );
    assert!(!error.contains("this build reads"));
    assert_eq!(format!("{selected:?}"), before);
}

#[test]
fn live_restart_resets_engine_and_trims_and_sun_rate_only_closes_the_old_v6_prefix() {
    let mut app = app();
    let initial = *app
        .world()
        .resource::<FlightSimulation>()
        .0
        .near_static_turboprop()
        .unwrap()
        .state();
    tick(&mut app, None, Duration::from_millis(50));
    let prefix = bytes(&export(&app.world().resource::<FlightSimulation>().0));
    let before = app.world().resource::<FlightSimulation>().0.elapsed();
    tick(&mut app, Some(KeyCode::Period), Duration::from_millis(50));
    assert!(app.world().resource::<FlightSimulation>().0.elapsed() > before);
    let FlightSession::NearStaticTurbopropLive {
        recording_error, ..
    } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert!(
        recording_error
            .as_ref()
            .unwrap()
            .contains("sun rate changed")
    );
    assert_eq!(
        bytes(&export(&app.world().resource::<FlightSimulation>().0)),
        prefix
    );
    let rate = app.world().resource::<flightsim_render::TimeOfDay>().rate;
    {
        let mut controls = app.world_mut().resource_mut::<PilotControls>();
        controls.aileron_trim.set(0.01234);
        controls.rudder_trim.set(-0.02345);
    }
    app.world_mut()
        .resource_mut::<FlightSimulation>()
        .0
        .queue_parking_toggle();
    tick(&mut app, Some(KeyCode::KeyR), Duration::ZERO);
    let controls = app.world().resource::<PilotControls>();
    assert_eq!(controls.aileron_trim.value(), 0.0);
    assert_eq!(controls.rudder_trim.value(), 0.0);
    let session = &app.world().resource::<FlightSimulation>().0;
    assert!(state_bits_equal(
        session.near_static_turboprop().unwrap().state(),
        &initial
    ));
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert_eq!(session.parking_brake(), Some((false, false)));
    let FlightSession::NearStaticTurbopropLive {
        recording_error, ..
    } = session
    else {
        unreachable!()
    };
    assert!(recording_error.is_none());
    assert!(export(session).controls().is_empty());
    assert_eq!(
        export(session)
            .conditions()
            .environment
            .conditions
            .time_rate,
        rate.get()
    );
}

#[test]
fn typed_v6_weather_is_authoritative_and_manual_clouds_disable_live_recording() {
    let mut app = app();
    app.init_resource::<WorldMapState>()
        .init_resource::<WorldMapActions>();
    weather_runtime::configure(&mut app);
    let initial = *session().near_static_turboprop().unwrap().state();
    let expected = WeatherSelection::Modeled(
        WeatherScenario::from_preset(WeatherPreset::Snow, initial.rigid_body.geodetic(), 919)
            .unwrap(),
    );
    let simulation = NearStaticTurbopropSimulation::from_supported_state(
        startup()
            .aircraft
            .near_static_turboprop()
            .unwrap()
            .configuration()
            .clone(),
        initial,
        NearStaticTurbopropEnvironment {
            weather: expected,
            ..default()
        },
        ControlInputs::neutral(),
    )
    .unwrap();
    let record = NearStaticTurbopropRecorder::new(&simulation)
        .unwrap()
        .finish();
    let player = NearStaticTurbopropReplayPlayer::new(simulation.config().clone(), record).unwrap();
    app.world_mut().resource_mut::<FlightSimulation>().0 =
        FlightSession::replay_near_static_turboprop(player);
    app.world_mut().resource_mut::<Startup>().weather.requested = Some(WeatherPreset::Clear);
    app.world_mut().resource_mut::<WorldMapState>().visible = true;
    app.world_mut()
        .resource_mut::<world_runtime::MapCapture>()
        .captured = true;
    app.world_mut()
        .resource_mut::<WorldMapActions>()
        .new_flight_shortcuts_available = true;
    tick(&mut app, Some(KeyCode::F12), Duration::ZERO);
    assert_eq!(
        app.world()
            .resource::<flightsim_render::RenderWeather>()
            .selection,
        expected
    );
    assert_eq!(
        app.world()
            .resource::<weather_runtime::PendingWeather>()
            .requested,
        Some(WeatherPreset::Clear)
    );
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .weather_note
            .contains("recorded weather")
    );
    let mut app = self::app();
    app.world_mut().resource_mut::<Startup>().clouds_were_given = true;
    let before = bytes(&export(&app.world().resource::<FlightSimulation>().0));
    tick(&mut app, None, Duration::from_millis(50));
    assert!(app.world().resource::<FlightSimulation>().0.elapsed() > Seconds::ZERO);
    assert_eq!(
        bytes(&export(&app.world().resource::<FlightSimulation>().0)),
        before
    );
    assert!(
        app.world()
            .resource::<flightsim_ui::ReplayStatus>()
            .notice
            .as_ref()
            .unwrap()
            .contains("F9 OFF: manual clouds")
    );
}
