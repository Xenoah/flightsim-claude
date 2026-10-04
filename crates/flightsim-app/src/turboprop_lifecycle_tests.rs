//! GPU-free app lifecycle evidence for the explicitly selected third family.
//! The ballistic numerical fixture is not an authored aircraft qualification.
#![allow(clippy::float_cmp)]
use super::*;
use flightsim_fdm::{ControlInputs, RigidBodyState};
use flightsim_sim::{
    aircraft_profile_v3::AircraftProfileV3,
    replay_v5::{TurbopropRecorder, TurbopropRecording, TurbopropReplayPlayer, state_bits_equal},
    turboprop_simulation::{TURBOPROP_FIXED_DT, TurbopropEnvironment, TurbopropSimulation},
};
use std::time::Duration;

pub(super) fn startup() -> Startup {
    let mut profile: serde_json::Value = serde_json::from_str(include_str!(
        "../../../assets/aircraft/kestrel_jet_trainer.json"
    ))
    .unwrap();
    let mut dynamics: serde_json::Value = serde_json::from_str(include_str!(
        "../../flightsim-fdm/tests/fixtures/turboprop-numerical.json"
    ))
    .unwrap();
    dynamics["kind"] = "running_turboprop_table".into();
    dynamics["revision"] = 1.into();
    dynamics["running_start"] = serde_json::json!({
        "turbine_fraction": 0.4,
        "shaft_rad_s": 180.0,
        "blade_pitch_rad": 0.3,
    });
    profile["version"] = 3.into();
    profile["id"] = "turboprop-lifecycle-fixture".into();
    profile["dynamics"] = dynamics;
    profile["controls"]["default_trim"] = 0.0.into();
    let mut startup = Startup {
        aircraft: aircraft_profile::SelectedAircraftProfile::Turboprop(
            AircraftProfileV3::parse(&profile.to_string()).unwrap(),
        ),
        wind: flightsim_sim::Wind::CALM,
        turbulence: flightsim_fdm::Turbulence::CALM,
        ..default()
    };
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    startup
}

pub(super) fn start() -> StartCondition {
    StartCondition::InFlight(RigidBodyState::from_geodetic(
        Geodetic::from_degrees(0.0, 0.0, 1000.0),
        Attitude::new(
            Radians::ZERO,
            Radians(-std::f64::consts::FRAC_PI_2),
            Radians::ZERO,
        ),
        Ned::new(0.0, 0.0, 40.0),
    ))
}

fn app() -> App {
    let startup = startup();
    let clock = flightsim_render::TimeOfDay::default();
    let session = FlightSession::prepare_bounded(&startup, &clock, start()).unwrap();
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(world_runtime::initial_controls(&startup))
        .insert_resource(startup)
        .insert_resource(start())
        .insert_resource(clock)
        .insert_resource(FlightSimulation(session))
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

pub(super) fn recording(steps: u32) -> TurbopropRecording {
    let startup = startup();
    let FlightSession::TurbopropLive { simulation, .. } =
        FlightSession::prepare_bounded(&startup, &flightsim_render::TimeOfDay::default(), start())
            .unwrap()
    else {
        panic!("explicit turboprop session")
    };
    let mut environment = TurbopropEnvironment::default();
    environment.conditions.start_epoch = 2_451_545.0;
    environment.conditions.time_rate = 60.0;
    let input = ControlInputs::new(0.0, 0.0, 0.0, 0.4, 0.1);
    let mut simulation = TurbopropSimulation::from_supported_state(
        simulation.config().clone(),
        *simulation.state(),
        environment,
        input,
    )
    .unwrap();
    let mut recorder = TurbopropRecorder::new(&simulation).unwrap();
    for _ in 0..steps {
        let report = simulation.advance(TURBOPROP_FIXED_DT, input);
        assert!(report.terminal().is_none(), "{:?}", report.terminal());
        recorder.record(&report).unwrap();
    }
    recorder.finish()
}

fn install_replay(app: &mut App, record: TurbopropRecording) {
    let total = record.duration();
    let mut player = TurbopropReplayPlayer::new(
        startup()
            .aircraft
            .turboprop()
            .unwrap()
            .configuration()
            .clone(),
        record,
    )
    .unwrap();
    while !player.finished() {
        player.advance(Seconds(0.25)).unwrap();
    }
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::TurbopropReplay {
        player,
        fault: None,
        pending_seek: None,
        total,
    };
}

#[test]
fn v5_rewind_restores_complete_state_and_recorded_clock_then_honors_transport_keys() {
    let record = recording(481);
    let initial = record.conditions().initial_state;
    let epoch = record.conditions().environment.conditions.start_epoch;
    let mut app = app();
    install_replay(&mut app, record);
    tick(&mut app, None, Duration::ZERO);
    assert!(
        app.world()
            .resource::<flightsim_ui::ReplayStatus>()
            .finished
    );
    tick(&mut app, Some(KeyCode::F8), Duration::ZERO);
    let session = &app.world().resource::<FlightSimulation>().0;
    assert!(state_bits_equal(
        session.turboprop().unwrap().state(),
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
    assert!(app.world().resource::<flightsim_ui::ReplayStatus>().active);
}

#[test]
fn v5_presentation_uses_recorded_controls_mutes_completion_and_has_no_legacy_grading() {
    let mut app = app();
    install_replay(&mut app, recording(120));
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
    assert!(!app.world().contains_resource::<FlightRecorder>());
    assert!(
        !app.world()
            .contains_resource::<flightsim_ui::LandingReport>()
    );
    assert!(!app.world().resource::<flightsim_ui::TutorialVisibility>().0);
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert!(status.active && status.finished);
    let notice = status.notice.as_ref().unwrap();
    assert!(notice.is_ascii());
    assert!(notice.contains("rad/s"));
    assert!(!notice.contains("N1"));
}

#[test]
fn turboprop_restart_restores_engine_state_and_rate_change_closes_only_recording() {
    let mut app = app();
    let initial = *app
        .world()
        .resource::<FlightSimulation>()
        .0
        .turboprop()
        .unwrap()
        .state();
    tick(&mut app, None, Duration::from_millis(50));
    let prefix = match &app.world().resource::<FlightSimulation>().0 {
        FlightSession::TurbopropLive { recorder, .. } => recorder.export(),
        _ => unreachable!(),
    };
    let before = app.world().resource::<FlightSimulation>().0.elapsed();
    tick(&mut app, Some(KeyCode::Period), Duration::from_millis(50));
    assert!(app.world().resource::<FlightSimulation>().0.elapsed() > before);
    let rate = app.world().resource::<flightsim_render::TimeOfDay>().rate;
    let FlightSession::TurbopropLive {
        recorder,
        recording_error,
        ..
    } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert_eq!(recorder.export(), prefix);
    assert!(
        recording_error
            .as_ref()
            .unwrap()
            .contains("sun rate changed")
    );
    {
        let mut controls = app.world_mut().resource_mut::<PilotControls>();
        controls.aileron_trim.set(0.01234);
        controls.rudder_trim.set(-0.02345);
    }
    tick(&mut app, Some(KeyCode::KeyR), Duration::ZERO);
    assert_eq!(
        app.world().resource::<PilotControls>().aileron_trim.value(),
        0.0
    );
    assert_eq!(
        app.world().resource::<PilotControls>().rudder_trim.value(),
        0.0
    );
    let session = &app.world().resource::<FlightSimulation>().0;
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert!(state_bits_equal(
        session.turboprop().unwrap().state(),
        &initial
    ));
    let FlightSession::TurbopropLive {
        recorder,
        recording_error,
        pending_parking_toggle,
        ..
    } = session
    else {
        unreachable!()
    };
    assert!(recording_error.is_none());
    assert!(!pending_parking_toggle);
    assert!(recorder.export().controls().is_empty());
    assert_eq!(
        recorder
            .export()
            .conditions()
            .environment
            .conditions
            .time_rate,
        rate.get()
    );
}

#[test]
fn live_lateral_trim_survives_pause_focus_and_map_capture_without_hidden_commands() {
    use flightsim_input::{InputDevices, PilotKeys};
    let mut app = app();
    let window = app
        .world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow))
        .id();
    {
        let mut controls = app.world_mut().resource_mut::<PilotControls>();
        controls.aileron_trim.set(0.01234);
        controls.rudder_trim.set(-0.02345);
    }
    for suspension in ["paused", "unfocused", "map"] {
        app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = suspension == "paused";
        app.world_mut()
            .resource_mut::<world_runtime::MapCapture>()
            .captured = suspension == "map";
        app.world_mut().get_mut::<Window>(window).unwrap().focused = suspension != "unfocused";
        *app.world_mut().resource_mut::<SampledPilotInput>() = SampledPilotInput::from_bindings(
            PilotKeys {
                aileron_trim_right: true,
                rudder_trim_left: true,
                lateral_trim_reset: true,
                ..default()
            },
            &flightsim_input::InputConfiguration::default(),
            &InputDevices::default(),
        );
        tick(&mut app, None, Duration::from_millis(20));
        let controls = app.world().resource::<PilotControls>();
        assert_eq!(controls.aileron_trim.value(), 0.01234, "{suspension}");
        assert_eq!(controls.rudder_trim.value(), -0.02345, "{suspension}");
    }
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert!(
        status
            .notice
            .as_ref()
            .unwrap()
            .contains("ROLL TRIM +0.01234")
    );
    assert!(
        status
            .notice
            .as_ref()
            .unwrap()
            .contains("YAW TRIM -0.02345")
    );
    // Playback never presents a pilot's retained trim as recorded metadata.
    let playback = with_lateral_trim_notice(
        flightsim_ui::ReplayStatus {
            active: true,
            ..default()
        },
        Some(app.world().resource::<PilotControls>()),
        true,
    );
    assert!(playback.notice.is_none());
    let zero = PilotControls::default();
    assert!(
        with_lateral_trim_notice(default(), Some(&zero), false)
            .notice
            .is_none()
    );
    assert!(
        with_lateral_trim_notice(default(), Some(&zero), true)
            .notice
            .unwrap()
            .contains("ROLL TRIM +0.00000")
    );
}
