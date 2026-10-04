//! App-owned dispatch and accepted-step control regressions, without a GPU.
#![allow(clippy::float_cmp)]
use super::*;
use flightsim_fdm::RigidBodyState;
use flightsim_input::{InputDevices, PilotKeys};
use flightsim_sim::replay_v4::{JetRecording, JetReplayPlayer, ModelReplayFile};
use std::time::Duration;

fn startup() -> Startup {
    let mut startup = Startup {
        aircraft: aircraft_profile::SelectedAircraftProfile::Jet(
            flightsim_sim::aircraft_profile::AircraftProfileV2::parse(include_str!(
                "../../../assets/aircraft/kestrel_jet_trainer.json"
            ))
            .unwrap(),
        ),
        ..default()
    };
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    startup.wind = flightsim_sim::Wind::CALM;
    startup.turbulence = flightsim_fdm::Turbulence::CALM;
    startup
}
fn airborne(speed: f64) -> StartCondition {
    StartCondition::InFlight(RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1000.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(speed, 0.0, 0.0),
    ))
}
fn app(start: StartCondition) -> App {
    let startup = startup();
    let session =
        FlightSession::prepare_jet(&startup, &flightsim_render::TimeOfDay::default(), start)
            .unwrap();
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(world_runtime::initial_controls(&startup))
        .insert_resource(startup)
        .insert_resource(start)
        .insert_resource(FlightSimulation(session))
        .init_resource::<SampledPilotInput>()
        .init_resource::<flightsim_ui::Paused>()
        .init_resource::<world_runtime::MapCapture>()
        .add_systems(
            Update,
            advance_simulation.run_if(world_runtime::flight_controls_active),
        );
    configure_live_input_scheduling(&mut app);
    app
}
fn sample(keys: PilotKeys) -> SampledPilotInput {
    SampledPilotInput::from_bindings(
        keys,
        &flightsim_input::InputConfiguration::default(),
        &InputDevices::default(),
    )
}
fn tick(app: &mut App, dt: Duration, keys: PilotKeys) {
    app.world_mut().resource_mut::<Time>().advance_by(dt);
    *app.world_mut().resource_mut::<SampledPilotInput>() = sample(keys);
    app.update();
}
fn export(app: &App) -> JetRecording {
    let FlightSession::JetLive { recorder, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        panic!("live jet")
    };
    recorder.export()
}
fn run(fps: u32) -> App {
    let mut app = app(airborne(50.0));
    let mut previous = Duration::ZERO;
    for frame in 0..fps * 2 {
        let now = Duration::from_secs_f64(f64::from(frame + 1) / f64::from(fps));
        tick(
            &mut app,
            now - previous,
            PilotKeys {
                throttle_up: frame < fps,
                trim_down: frame >= fps,
                ..default()
            },
        );
        previous = now;
    }
    app
}
#[test]
fn jet_live_controls_record_only_accepted_steps_and_match_shared_cadences() {
    let reference = run(60);
    let expected = export(&reference);
    assert!(!reference.world().contains_resource::<FlightRecorder>());
    assert_eq!(expected.controls().len(), 240);
    for fps in [30, 144] {
        let candidate = run(fps);
        assert_eq!(export(&candidate), expected, "at {fps} Hz");
    }
}
#[test]
fn parking_toggle_waits_for_an_accepted_step_and_uses_maximum_service_brake() {
    let startup = startup();
    let mut app = app(StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    });
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.parking_brake(),
        Some((true, false))
    );
    app.world_mut()
        .resource_mut::<FlightSimulation>()
        .0
        .queue_parking_toggle();
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.parking_brake(),
        Some((true, true))
    );
    tick(
        &mut app,
        Duration::from_millis(9),
        PilotKeys {
            brakes: true,
            ..default()
        },
    );
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.parking_brake(),
        Some((false, false))
    );
    assert_eq!(export(&app).controls()[0].brakes(), 1.0);
    tick(&mut app, Duration::from_millis(9), PilotKeys::default());
    assert_eq!(export(&app).controls()[1].brakes(), 0.0);
    app.world_mut()
        .resource_mut::<FlightSimulation>()
        .0
        .queue_parking_toggle();
    tick(&mut app, Duration::from_millis(9), PilotKeys::default());
    assert_eq!(export(&app).controls()[2].brakes(), 1.0);
}
#[test]
fn rejected_first_step_keeps_ramps_and_pending_toggle_and_exports_terminal_zero() {
    let mut app = app(airborne(50.0));
    // An imported v4 may authentically begin outside the operating box. Live
    // start preparation correctly rejects this state; inject it here solely to
    // exercise app terminal presentation/recorder rollback at cursor zero.
    let StartCondition::InFlight(state) = airborne(180.0) else {
        unreachable!()
    };
    let simulation = flightsim_sim::model_simulation::JetSimulation::from_state(
        startup().aircraft.jet().unwrap().configuration().clone(),
        state,
        flightsim_sim::model_simulation::JetEnvironment::default(),
    )
    .unwrap();
    let recorder = flightsim_sim::replay_v4::JetRecorder::new(&simulation).unwrap();
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::JetLive {
        simulation,
        recorder,
        parking_brake: false,
        pending_parking_toggle: false,
        last_controls: flightsim_fdm::ControlInputs::neutral(),
        recording_error: None,
        fault: None,
    };
    let before = *app.world().resource::<FlightSimulation>().0.state();
    let controls = app.world().resource::<PilotControls>().to_control_inputs();
    app.world_mut()
        .resource_mut::<FlightSimulation>()
        .0
        .queue_parking_toggle();
    tick(
        &mut app,
        Duration::from_millis(9),
        PilotKeys {
            throttle_up: true,
            pitch_up: true,
            trim_up: true,
            ..default()
        },
    );
    let session = &app.world().resource::<FlightSimulation>().0;
    assert_eq!(*session.state(), before);
    assert_eq!(session.elapsed(), Seconds::ZERO);
    assert_eq!(session.parking_brake(), Some((false, true)));
    assert_eq!(
        app.world().resource::<PilotControls>().to_control_inputs(),
        controls
    );
    let first = export(&app);
    assert!(first.controls().is_empty());
    assert!(first.terminal().is_some());
    assert_eq!(export(&app), first);
    let mut bytes = Vec::new();
    first.write_to(&mut bytes).unwrap();
    let ModelReplayFile::V4(recording) = ModelReplayFile::read_from(&mut bytes.as_slice()).unwrap()
    else {
        panic!("v4")
    };
    let replay = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        recording,
    )
    .unwrap();
    assert!(replay.finished());
    assert_eq!(replay.cursor(), 0);
    assert_eq!(*replay.simulation().state(), before);
}
#[test]
fn repeated_snapshots_continue_recording_and_app_playback_owns_its_state() {
    let mut live = run(60);
    let first = export(&live);
    tick(
        &mut live,
        Duration::from_secs_f64(1.0 / 60.0),
        PilotKeys::default(),
    );
    let second = export(&live);
    assert_eq!(second.controls().len(), first.controls().len() + 2);
    let expected = *live.world().resource::<FlightSimulation>().0.state();
    let mut playback = app(airborne(50.0));
    let player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        second,
    )
    .unwrap();
    playback.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    for _ in 0..12 {
        tick(
            &mut playback,
            Duration::from_millis(250),
            PilotKeys {
                pitch_up: true,
                throttle_up: true,
                brakes: true,
                ..default()
            },
        );
    }
    let session = &playback.world().resource::<FlightSimulation>().0;
    assert_eq!(*session.state(), expected);
    assert!(session.audio_paused());
    let FlightSession::JetReplay { player, fault, .. } = session else {
        unreachable!()
    };
    assert!(player.finished() && fault.is_none());
    assert_eq!(session.last_controls(), Some(player.last_controls()));
    assert!(!playback.world().contains_resource::<ReplayPlayback>());
}
#[test]
fn jet_render_quaternion_validation_is_separate_from_legacy_tolerance() {
    let StartCondition::InFlight(mut state) = airborne(50.0) else {
        unreachable!()
    };
    state.orientation *= 1.0 + 5e-10;
    assert!(flight_session::validate_jet_state(&state).is_ok());
    assert!(replay_runtime::validate_replay_state(&state).is_err());
    state.orientation *= 1.0 + 2e-9;
    assert!(flight_session::validate_jet_state(&state).is_err());
}
#[test]
fn jet_pause_and_focus_loss_release_transient_input_without_releasing_parking() {
    let startup = startup();
    let mut app = app(StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    });
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: false,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ))
        .id();
    tick(
        &mut app,
        Duration::from_secs_f64(1.0 / 60.0),
        PilotKeys {
            pitch_up: true,
            throttle_up: true,
            brakes: true,
            ..default()
        },
    );
    assert_eq!(
        app.world().resource::<PilotControls>().throttle.value(),
        0.0
    );
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.parking_brake(),
        Some((true, false))
    );
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = true;
    let before = export(&app);
    tick(
        &mut app,
        Duration::from_secs(1),
        PilotKeys {
            pitch_up: true,
            throttle_up: true,
            ..default()
        },
    );
    assert_eq!(export(&app), before);
    app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = false;
    tick(
        &mut app,
        Duration::from_secs_f64(1.0 / 60.0),
        PilotKeys::default(),
    );
    assert_eq!(
        app.world().resource::<PilotControls>().elevator.value(),
        0.0
    );
}
#[test]
fn jet_idle_parking_brake_has_measured_contact_creep_without_a_position_lock() {
    let startup = startup();
    let start = StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    };
    let mut parked = app(start);
    let initial = parked
        .world()
        .resource::<FlightSimulation>()
        .0
        .state()
        .position;
    for _ in 0..600 {
        tick(
            &mut parked,
            Duration::from_secs_f64(1.0 / 60.0),
            PilotKeys::default(),
        );
    }
    let session = &parked.world().resource::<FlightSimulation>().0;
    let creep = initial.as_vec().distance(session.state().position.as_vec());
    println!("Kestrel idle/parking contact displacement at 10 seconds: {creep:.9} m");
    assert!(session.terminal_message().is_none());
    assert!(
        creep > 0.0 && creep < 1.0,
        "measured parked displacement {creep} m"
    );
    assert!(
        export(&parked)
            .controls()
            .iter()
            .all(|input| input.brakes() == 1.0 && input.throttle() == 0.0)
    );
}
#[test]
fn jet_guidance_and_notices_are_ascii_and_do_not_claim_prop_rotation_or_landing_grade() {
    let guidance = flight_session::jet_guidance();
    let help = guidance.live_help.unwrap();
    let compact = guidance.compact_live_help.unwrap();
    assert!(compact.is_ascii());
    for key in [
        "W/S",
        "A/D",
        "Q/E",
        "PageUp/Down",
        "F/G",
        "[/]",
        "Space/B",
        "J/L",
        "U/O",
        "Shift",
        "K reset",
        "C view",
        "M map",
        "R restart",
        "F9",
        "Esc",
    ] {
        assert!(compact.contains(key), "compact jet reference lost {key}");
    }
    assert!(!guidance.tutorial_enabled && help.is_ascii());
    assert!(!help.contains("75 kt"));
    let app = app(airborne(50.0));
    let notice = flight_session::jet_notice(
        &app.world().resource::<FlightSimulation>().0,
        Some(&startup()),
    );
    assert!(notice.is_ascii() && notice.contains("Mach") && notice.contains("PARK OFF"));
}

fn add_shortcuts(app: &mut App) {
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<CameraRig>()
        .init_resource::<flightsim_ui::TutorialState>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .init_resource::<flightsim_render::TimeOfDay>()
        .insert_resource(flightsim_audio::SoundBridge(std::sync::Arc::new(
            flightsim_audio::SharedSound::default(),
        )))
        .add_systems(
            Update,
            (control_flight, control_replay).before(advance_simulation),
        );
}
fn key(app: &mut App, key: KeyCode) {
    let mut keyboard = ButtonInput::default();
    keyboard.press(key);
    app.insert_resource(keyboard);
}

fn add_hud(app: &mut App) -> Entity {
    app.init_resource::<StallWarningStatus>()
        .init_resource::<flightsim_audio::AircraftSound>()
        .insert_resource(ViewMode::Cockpit)
        .init_resource::<SunDirection>()
        .init_resource::<HudState>()
        .init_resource::<flightsim_ui::HudSmoothing>()
        .add_systems(Update, flightsim_ui::update_hud);
    configure_flight_presentation(app);
    app.world_mut()
        .spawn((flightsim_ui::HudText, Text::default()))
        .id()
}

fn stalling_start(speed: f64) -> StartCondition {
    StartCondition::InFlight(RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1000.0),
        Attitude::from_degrees(0.0, 17.0, 0.0),
        Ned::new(speed, 0.0, 0.0),
    ))
}

fn assert_stall_presentation(app: &App, text: Entity, warning: bool, muted: bool) {
    let sound = app.world().resource::<flightsim_audio::AircraftSound>();
    assert_eq!(sound.muted, muted);
    assert_eq!(sound.stall_warning, warning);
    let hud = app.world().resource::<HudState>();
    assert_eq!(hud.stall_warning, warning);
    assert!(!hud.stall_warning_unavailable);
    assert_eq!(
        app.world()
            .get::<Text>(text)
            .unwrap()
            .contains("STALL WARN"),
        warning
    );
}

#[test]
fn jet_live_pause_keeps_actual_stall_warning_but_mutes_sound_and_fault_clears_it() {
    let mut app = app(stalling_start(50.0));
    add_shortcuts(&mut app);
    let text = add_hud(&mut app);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, false);
    key(&mut app, KeyCode::Escape);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, true);
    key(&mut app, KeyCode::Escape);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, false);
    let FlightSession::JetLive { fault, .. } =
        &mut app.world_mut().resource_mut::<FlightSimulation>().0
    else {
        unreachable!()
    };
    *fault = Some("test invalid render state".into());
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, false, true);

    let mut slow = self::app(stalling_start(4.0));
    let text = add_hud(&mut slow);
    tick(&mut slow, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&slow, text, false, false);
}

fn stall_recording(steps: u32) -> JetRecording {
    use flightsim_sim::{
        model_simulation::{JET_FIXED_DT, JetEnvironment, JetSimulation},
        replay_v4::JetRecorder,
    };
    let StartCondition::InFlight(state) = stalling_start(50.0) else {
        unreachable!()
    };
    let mut simulation = JetSimulation::from_state(
        startup().aircraft.jet().unwrap().configuration().clone(),
        state,
        JetEnvironment::default(),
    )
    .unwrap();
    let mut recorder = JetRecorder::new(&simulation).unwrap();
    for _ in 0..steps {
        let report = simulation.advance(
            JET_FIXED_DT,
            flightsim_fdm::ControlInputs::neutral()
                .with_elevator(1.0)
                .with_throttle(0.8),
        );
        assert!(report.terminal().is_none());
        recorder.record(&report).unwrap();
    }
    recorder.export()
}

#[test]
fn jet_replay_pause_and_clean_completion_keep_actual_stall_warning_with_muted_audio() {
    let mut app = app(stalling_start(50.0));
    add_shortcuts(&mut app);
    let text = add_hud(&mut app);
    let mut player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        stall_recording(1),
    )
    .unwrap();
    player.set_paused(true);
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, true);
    key(&mut app, KeyCode::F5);
    tick(&mut app, Duration::from_millis(20), PilotKeys::default());
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert!(player.finished() && !player.faulted() && player.simulation().terminal().is_none());
    assert_stall_presentation(&app, text, true, true);
}

#[test]
fn jet_replay_queued_and_bounded_seek_show_current_warning_while_muted() {
    let mut app = app(stalling_start(50.0));
    let text = add_hud(&mut app);
    let player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        stall_recording(480),
    )
    .unwrap();
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    let FlightSession::JetReplay { pending_seek, .. } =
        &mut app.world_mut().resource_mut::<FlightSimulation>().0
    else {
        unreachable!()
    };
    *pending_seek = Some(480);
    // Map capture delays execution; the pending seek must not hide the still
    // valid current snapshot while audio is already muted.
    app.world_mut()
        .resource_mut::<world_runtime::MapCapture>()
        .captured = true;
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, true);
    app.world_mut()
        .resource_mut::<world_runtime::MapCapture>()
        .captured = false;
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert_eq!(player.cursor(), 240);
    assert!(player.seeking());
    assert!(
        app.world()
            .resource::<FlightSimulation>()
            .0
            .aero_angles()
            .angle_of_attack
            .to_degrees()
            .get()
            .abs()
            > 14.0
    );
    assert_stall_presentation(&app, text, true, true);
}

#[test]
fn jet_terminal_live_and_replay_clear_stale_warning_even_with_a_finite_stalled_snapshot() {
    let StartCondition::InFlight(mut state) = stalling_start(50.0) else {
        unreachable!()
    };
    state.angular_velocity.x = 100.0; // Forces the bounded integration budget to reject the first step.
    let mut app = app(StartCondition::InFlight(state));
    let text = add_hud(&mut app);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, false);
    tick(&mut app, Duration::from_millis(9), PilotKeys::default());
    assert!(
        app.world()
            .resource::<FlightSimulation>()
            .0
            .jet()
            .unwrap()
            .terminal()
            .is_some()
    );
    assert_eq!(*app.world().resource::<FlightSimulation>().0.state(), state);
    assert_stall_presentation(&app, text, false, true);
    let player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        export(&app),
    )
    .unwrap();
    assert!(player.finished() && player.simulation().terminal().is_some());
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, false, true);
}

#[test]
fn jet_replay_reproduction_fault_clears_stale_stall_warning() {
    let mut bytes = Vec::new();
    stall_recording(1).write_to(&mut bytes).unwrap();
    // No terminal block: the final 104-byte checkpoint precedes its zero
    // length field. Change one position bit while retaining a valid state.
    let position_byte = bytes.len() - 4 - 104;
    bytes[position_byte] ^= 1;
    let recording = JetRecording::read_from(&mut bytes.as_slice()).unwrap();
    let player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        recording,
    )
    .unwrap();
    let mut app = app(stalling_start(50.0));
    let text = add_hud(&mut app);
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_stall_presentation(&app, text, true, false);
    tick(&mut app, Duration::from_millis(20), PilotKeys::default());
    let FlightSession::JetReplay { player, fault, .. } =
        &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert!(player.faulted() && fault.is_some());
    assert_stall_presentation(&app, text, false, true);
}

fn assert_current_hud(app: &App, text: Entity) {
    let hud = app.world().resource::<HudState>();
    let shown = app
        .world()
        .resource::<flightsim_ui::HudSmoothing>()
        .displayed();
    assert_eq!(shown.airspeed, hud.equivalent_airspeed.to_knots());
    assert_eq!(shown.altitude, hud.altitude.to_feet());
    assert_eq!(shown.agl, hud.agl.to_feet());
    assert_eq!(
        shown.vertical_speed,
        hud.vertical_speed.to_feet_per_minute()
    );
    assert_eq!(
        shown.heading_degrees,
        hud.heading.wrap_positive().to_degrees().get()
    );
    assert_eq!(shown.pitch_degrees, hud.pitch.to_degrees().get());
    assert_eq!(shown.roll_degrees, hud.roll.to_degrees().get());
    assert_eq!(
        app.world().get::<Text>(text).unwrap().as_str(),
        flightsim_ui::format_hud(shown, hud)
    );
}

fn climbing(vertical_speed: f64) -> StartCondition {
    StartCondition::InFlight(RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1000.0),
        Attitude::from_degrees(0.0, 15.0, 0.0),
        Ned::new(80.0, 0.0, -vertical_speed),
    ))
}

#[test]
fn jet_restart_reseeds_actual_hud_on_first_update_even_when_time_does_not_advance() {
    let startup = startup();
    for start in [
        StartCondition::Parked {
            position: startup.start,
            heading: startup.heading,
        },
        climbing(-8.0),
    ] {
        let mut app = app(climbing(26.4));
        add_shortcuts(&mut app);
        let text = add_hud(&mut app);
        {
            let mut smoothing = app.world_mut().resource_mut::<flightsim_ui::HudSmoothing>();
            smoothing.refresh_interval = Seconds(0.75);
            smoothing.vertical_speed_time_constant = Seconds(2.4);
        }
        app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = true;
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_current_hud(&app, text);
        assert!(
            app.world()
                .resource::<flightsim_ui::HudSmoothing>()
                .displayed()
                .vertical_speed
                .get()
                > 5000.0
        );
        app.insert_resource(start);
        key(&mut app, KeyCode::KeyR);
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_current_hud(&app, text);
        assert!(app.world().resource::<HudState>().vertical_speed.get() <= 0.0);
        let smoothing = app.world().resource::<flightsim_ui::HudSmoothing>();
        assert_eq!(smoothing.refresh_interval, Seconds(0.75));
        assert_eq!(smoothing.vertical_speed_time_constant, Seconds(2.4));
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.elapsed(),
            Seconds::ZERO
        );
    }
}

#[test]
fn jet_hud_keeps_smoothing_normal_live_motion_after_initial_seed() {
    let mut app = app(climbing(26.4));
    let text = add_hud(&mut app);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_current_hud(&app, text);
    let before = app
        .world()
        .resource::<flightsim_ui::HudSmoothing>()
        .displayed()
        .vertical_speed
        .get();
    tick(&mut app, Duration::from_millis(100), PilotKeys::default());
    let raw = app
        .world()
        .resource::<HudState>()
        .vertical_speed
        .to_feet_per_minute()
        .get();
    let shown = app
        .world()
        .resource::<flightsim_ui::HudSmoothing>()
        .displayed()
        .vertical_speed
        .get();
    assert!(
        (raw - before).abs() > 1.0,
        "fixture must actually change the climb rate"
    );
    assert!(
        shown > raw.min(before) && shown < raw.max(before),
        "live motion must still be smoothed: {before} -> {shown} -> {raw}"
    );
}

#[test]
fn jet_map_new_flight_reseeds_displayed_values_in_the_commit_update() {
    use flightsim_ui::world_map::{WorldMapActions, WorldMapStart, WorldMapState};
    let mut app = app(climbing(26.4));
    add_shortcuts(&mut app);
    let text = add_hud(&mut app);
    app.init_resource::<WorldMapState>()
        .init_resource::<WorldMapActions>()
        .init_resource::<TerrainTiles>()
        .init_resource::<Assets<Mesh>>()
        .insert_resource(TerrainStreaming {
            selector: LodSelector::new(
                16.0,
                720.0,
                Degrees(60.0).to_radians(),
                13,
                Meters(20_000.0),
            ),
            source: Box::new(MemoryTileSource::new()) as BoxedSource,
            cache: TileCache::new(1024 * 1024),
            live: default(),
            material: default(),
        })
        .add_systems(
            Update,
            world_runtime::apply_world_map_start
                .before(control_flight)
                .before(control_replay)
                .before(advance_simulation),
        );
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert!(
        app.world()
            .resource::<flightsim_ui::HudSmoothing>()
            .displayed()
            .vertical_speed
            .get()
            > 5000.0
    );
    app.world_mut().resource_mut::<WorldMapActions>().start_at = Some(WorldMapStart {
        aircraft_choice: 0,
        generation: 0,
        position: Geodetic::from_degrees(0.0, -140.0, 0.0),
        month: 7,
    });
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    assert_current_hud(&app, text);
    assert!(
        app.world()
            .resource::<HudState>()
            .vertical_speed
            .get()
            .abs()
            < 1e-8
    );
}

#[test]
fn jet_restart_reconstructs_parked_brake_and_airborne_levers_repeatedly() {
    let startup = startup();
    for start in [
        StartCondition::Parked {
            position: startup.start,
            heading: startup.heading,
        },
        airborne(50.0),
    ] {
        let mut app = app(start);
        add_shortcuts(&mut app);
        app.world_mut()
            .resource_mut::<flightsim_render::TimeOfDay>()
            .rate = flightsim_render::TimeRate(600.0);
        for _ in 0..3 {
            key(&mut app, KeyCode::KeyB);
            tick(
                &mut app,
                Duration::from_millis(100),
                PilotKeys {
                    pitch_up: true,
                    throttle_up: true,
                    ..default()
                },
            );
            key(&mut app, KeyCode::Escape);
            tick(&mut app, Duration::ZERO, PilotKeys::default());
            key(&mut app, KeyCode::KeyR);
            tick(&mut app, Duration::ZERO, PilotKeys::default());
            let session = &app.world().resource::<FlightSimulation>().0;
            assert_eq!(session.elapsed(), Seconds::ZERO);
            assert!(export(&app).controls().is_empty());
            assert_eq!(
                export(&app).conditions().environment.conditions.time_rate,
                600.0
            );
            assert_eq!(
                session.parking_brake(),
                Some((matches!(start, StartCondition::Parked { .. }), false))
            );
            assert!(!app.world().resource::<flightsim_ui::Paused>().is_paused());
            assert_eq!(
                app.world().resource::<PilotControls>().to_control_inputs(),
                world_runtime::initial_controls(&startup).to_control_inputs()
            );
        }
    }
}
#[test]
fn jet_f8_seek_budget_repeated_seek_pause_speed_and_clock_use_player_state() {
    let startup = startup();
    let start = StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    };
    let mut live = app(start);
    for _ in 0..1800 {
        tick(
            &mut live,
            Duration::from_secs_f64(1.0 / 60.0),
            PilotKeys::default(),
        );
    }
    let recording = export(&live);
    assert_eq!(recording.controls().len(), 3600);
    let initial = recording.conditions().initial_state;
    let mut app = app(start);
    add_shortcuts(&mut app);
    let hud_text = add_hud(&mut app);
    app.add_systems(Update, sync_replay_clock.after(advance_simulation));
    let mut player = JetReplayPlayer::new(
        startup.aircraft.jet().unwrap().configuration().clone(),
        recording,
    )
    .unwrap();
    player.set_speed(8.0);
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    for _ in 0..16 {
        tick(&mut app, Duration::from_millis(250), PilotKeys::default());
    }
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert!(player.finished());
    key(&mut app, KeyCode::F8);
    tick(&mut app, Duration::from_millis(250), PilotKeys::default());
    assert_current_hud(&app, hud_text);
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert_eq!(player.cursor(), 240);
    assert_eq!(player.seek_target(), Some(2400));
    key(&mut app, KeyCode::F8);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_current_hud(&app, hud_text);
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert_eq!(player.cursor(), 240);
    assert_eq!(player.seek_target(), Some(1200));
    // Every bounded reconstruction batch, including the final one, must show
    // its own state rather than a blend of unrelated historical frames.
    app.insert_resource(ButtonInput::<KeyCode>::default());
    for _ in 0..4 {
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_current_hud(&app, hud_text);
    }
    assert!(!app.world().resource::<FlightSimulation>().0.seeking());
    key(&mut app, KeyCode::F8);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_current_hud(&app, hud_text);
    assert_eq!(
        *app.world().resource::<FlightSimulation>().0.state(),
        initial
    );
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.elapsed(),
        Seconds::ZERO
    );
    key(&mut app, KeyCode::F5);
    tick(&mut app, Duration::from_millis(250), PilotKeys::default());
    assert_eq!(
        app.world().resource::<FlightSimulation>().0.elapsed(),
        Seconds::ZERO
    );
    key(&mut app, KeyCode::F6);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    let FlightSession::JetReplay { player, .. } = &app.world().resource::<FlightSimulation>().0
    else {
        unreachable!()
    };
    assert_eq!(player.speed(), 4.0);
    assert!(player.paused());
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().rate,
        flightsim_render::TimeRate::PAUSED
    );
}

#[test]
fn recorder_failure_keeps_authentic_prefix_while_live_controls_and_physics_continue() {
    let mut app = app(airborne(50.0));
    let authentic = export(&app);
    let state = *app.world().resource::<FlightSimulation>().0.state();
    let aircraft = app
        .world_mut()
        .spawn((
            Aircraft,
            WorldPosition(state.position),
            WorldOrientation(state.orientation),
        ))
        .id();
    // Deliberately omit one report to exercise JetRecorder's closed-prefix
    // contract without simulating the production million-step capacity limit.
    let FlightSession::JetLive { simulation, .. } =
        &mut app.world_mut().resource_mut::<FlightSimulation>().0
    else {
        unreachable!()
    };
    simulation.advance(
        Seconds(1.0 / 120.0),
        flightsim_fdm::ControlInputs::neutral(),
    );
    tick(
        &mut app,
        Duration::from_millis(9),
        PilotKeys {
            throttle_up: true,
            ..default()
        },
    );
    let first_elapsed = app.world().resource::<FlightSimulation>().0.elapsed();
    let first_throttle = app.world().resource::<PilotControls>().throttle.value();
    assert_eq!(export(&app), authentic);
    tick(
        &mut app,
        Duration::from_millis(9),
        PilotKeys {
            throttle_up: true,
            ..default()
        },
    );
    let session = &app.world().resource::<FlightSimulation>().0;
    assert!(session.elapsed() > first_elapsed);
    assert!(app.world().resource::<PilotControls>().throttle.value() > first_throttle);
    assert!(session.fault().is_none() && session.terminal_message().is_none());
    assert_eq!(export(&app), authentic);
    let pose = session.interpolated();
    assert_eq!(
        app.world().get::<WorldPosition>(aircraft).unwrap().0,
        pose.position
    );
    assert_eq!(
        app.world().get::<WorldOrientation>(aircraft).unwrap().0,
        pose.orientation
    );
    assert_ne!(pose.position, state.position);
    assert!(flight_session::jet_notice(session, Some(&startup())).contains("RECORDING STOPPED"));
}

#[test]
fn live_sun_rate_change_stops_only_recording_and_restart_records_current_rate() {
    let mut app = run(60);
    let prefix = export(&app);
    let elapsed = app.world().resource::<FlightSimulation>().0.elapsed();
    add_shortcuts(&mut app);
    app.add_systems(Update, adjust_time_rate.before(advance_simulation));
    let old_rate = app.world().resource::<flightsim_render::TimeOfDay>().rate;
    key(&mut app, KeyCode::Period);
    tick(&mut app, Duration::from_millis(20), PilotKeys::default());
    let rate = app.world().resource::<flightsim_render::TimeOfDay>().rate;
    assert_ne!(old_rate, rate);
    assert_eq!(export(&app), prefix);
    assert!(app.world().resource::<FlightSimulation>().0.elapsed() > elapsed);
    assert!(
        flight_session::jet_notice(
            &app.world().resource::<FlightSimulation>().0,
            Some(&startup())
        )
        .contains("sun rate changed")
    );
    key(&mut app, KeyCode::KeyR);
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_eq!(
        export(&app).conditions().environment.conditions.time_rate,
        rate.get()
    );
    assert!(
        !flight_session::jet_notice(
            &app.world().resource::<FlightSimulation>().0,
            Some(&startup())
        )
        .contains("RECORDING STOPPED")
    );
}

#[test]
fn jet_presentation_systems_publish_recorded_controls_cockpit_and_no_landing_grade() {
    let mut app = run(60);
    let record = export(&app);
    let mut player = JetReplayPlayer::new(
        startup().aircraft.jet().unwrap().configuration().clone(),
        record,
    )
    .unwrap();
    while !player.finished() {
        player.advance(Seconds(0.25)).unwrap();
    }
    let input = player.last_controls();
    app.world_mut().resource_mut::<FlightSimulation>().0 = FlightSession::replay(player);
    app.world_mut()
        .resource_mut::<PilotControls>()
        .throttle
        .set_absolute(0.99);
    app.init_resource::<StallWarningStatus>()
        .init_resource::<flightsim_audio::AircraftSound>()
        .insert_resource(ViewMode::Cockpit)
        .init_resource::<SunDirection>()
        .init_resource::<HudState>()
        .init_resource::<flightsim_ui::CrashNotice>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .init_resource::<flightsim_ui::ReplayStatus>();
    configure_flight_presentation(&mut app);
    app.add_systems(
        Update,
        (
            publish_crash,
            publish_replay_status,
            report_landings,
            update_model_visibility,
        )
            .after(advance_simulation),
    );
    let exterior = app
        .world_mut()
        .spawn((ExteriorModel, Visibility::Hidden))
        .id();
    tick(&mut app, Duration::ZERO, PilotKeys::default());
    assert_eq!(
        app.world().get::<Visibility>(exterior),
        Some(&Visibility::Inherited)
    );
    assert!(
        !app.world()
            .contains_resource::<flightsim_ui::LandingReport>()
    );
    assert!(
        !app.world()
            .resource::<flightsim_ui::CrashNotice>()
            .is_crashed()
    );
    let sound = app.world().resource::<flightsim_audio::AircraftSound>();
    assert_eq!(sound.throttle, input.throttle());
    assert!(sound.muted);
    let hud = app.world().resource::<HudState>();
    assert_eq!(hud.throttle, input.throttle());
    assert_eq!(hud.flaps, input.flaps());
    assert_eq!(hud.trim, 0.0);
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert!(status.active && status.finished);
    assert!(status.notice.as_ref().unwrap().contains("recorded brake"));
}

#[test]
fn jet_warning_peak_tracks_current_mach_and_unavailable_domain() {
    use flightsim_sim::model_simulation::{JetEnvironment, JetSimulation};
    let startup = startup();
    let config = startup.aircraft.jet().unwrap().configuration().clone();
    // Kestrel currently holds equal endpoint curves. Author a supported varying
    // schedule here so this test detects accidentally caching only profile/flaps.
    let mut schedule = config.aero().definition().clone();
    schedule.knots[1].aero.stall_angle_rad = 0.20;
    let config = flightsim_fdm::subsonic::JetAircraftConfig::new(
        config.airframe().clone(),
        config.thrust().clone(),
        flightsim_fdm::subsonic::MachAeroSchedule::from_definition(schedule).unwrap(),
        *config.envelope(),
    )
    .unwrap();

    let position = Geodetic::from_degrees(0.0, 0.0, 1000.0);
    let sound = flightsim_fdm::Atmosphere::standard()
        .sample(position.altitude)
        .speed_of_sound
        .get();
    let simulation = |mach: f64| {
        JetSimulation::from_state(
            config.clone(),
            RigidBodyState::from_geodetic(
                position,
                Attitude::from_degrees(0.0, 0.0, 0.0),
                Ned::new(sound * mach, 0.0, 0.0),
            ),
            JetEnvironment::default(),
        )
        .unwrap()
    };
    let low = simulation(0.05);
    let high = simulation(0.34);
    let peak = |sim: &JetSimulation| {
        flightsim_fdm::aero::positive_stall_peak_angle(
            &sim.presentation().aero_coefficients.unwrap(),
            config.airframe().geometry(),
            0.0,
        )
        .unwrap()
    };
    let low_peak = peak(&low);
    let high_peak = peak(&high);
    assert_ne!(low_peak.get().to_bits(), high_peak.get().to_bits());
    let middle = Radians((low_peak.get() + high_peak.get()) * 0.5 * STALL_WARNING_FRACTION);
    let mut warning = StallWarningStatus::default();
    warning.update_jet(&low, 0.0, middle, true);
    assert!(!warning.active);
    warning.update_jet(&high, 0.0, middle, true);
    assert!(warning.active);
    warning.update_jet(&low, 0.0, middle, true);
    assert!(!warning.active);
    warning.update_jet(&simulation(0.5), 0.0, middle, true);
    assert!(warning.unavailable && !warning.active);
}

#[test]
fn jet_v4_source_admission_restores_recorded_environment_and_rejects_atomically() {
    let recording = export(&run(60));
    let directory = std::env::temp_dir().join(format!(
        "flightsim-app-jet-admission-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("jet.fsreplay");
    recording
        .write_to(&mut std::fs::File::create(&path).unwrap())
        .unwrap();
    let mut accepted = startup();
    accepted.replay = Some(path.clone());
    let mut diagnostics = StartupDiagnostics::default();
    let player = flight_session::resolve_jet_sources(&mut accepted, &mut diagnostics)
        .unwrap()
        .unwrap();
    assert_eq!(player.recording(), &recording);
    assert_eq!(
        accepted.wind,
        recording.conditions().environment.conditions.wind
    );
    for reject in 0..4 {
        let mut candidate = startup();
        candidate.replay = Some(path.clone());
        match reject {
            0 => candidate.world.global_terrain = true,
            1 => candidate.tiles = Some("unopened-tiles".into()),
            2 => candidate.clouds_were_given = true,
            _ => candidate.weather.was_given = true,
        }
        let before = format!("{candidate:?}");
        assert!(
            flight_session::resolve_jet_sources(&mut candidate, &mut StartupDiagnostics::default())
                .is_err()
        );
        assert_eq!(format!("{candidate:?}"), before);
    }
    let mut legacy = Startup {
        replay: Some(path),
        ..default()
    };
    let before = recording_conditions(&legacy, &flightsim_render::TimeOfDay::default());
    assert!(resolve_flight_sources(&mut legacy, &mut StartupDiagnostics::default()).is_none());
    assert_eq!(
        recording_conditions(&legacy, &flightsim_render::TimeOfDay::default()),
        before
    );
    std::fs::remove_dir_all(directory).unwrap();
}
