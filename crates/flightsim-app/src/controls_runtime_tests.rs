//! Actual app wiring regressions: one sampled command, one physics clock, and
//! the exact executed controls recorded before each step. No flight director.
// Exact equality is intentional for clamped endpoints, retained settings and
// deterministically identical control streams, rather than an accuracy estimate.
#![allow(clippy::float_cmp)]

use super::*;
use flightsim_core::MetersPerSecond;
use flightsim_fdm::RigidBodyState;
use flightsim_input::{InputDevices, PilotKeys};
use std::time::Duration;

fn sample(keys: PilotKeys) -> SampledPilotInput {
    SampledPilotInput::from_bindings(
        keys,
        &flightsim_input::InputConfiguration::default(),
        &InputDevices::default(),
    )
}

fn control_app(id: &str) -> App {
    let profile = aircraft_profile::AircraftProfile::builtin(id).unwrap();
    let config = profile.configuration();
    let initial = RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1500.0),
        Attitude::from_degrees(0.0, 4.0, 0.0),
        Ned::new(45.0, 0.0, 0.0),
    );
    let source: BoxedSource = Box::new(MemoryTileSource::new());
    let simulation = Simulation::from_state(
        config.clone(),
        initial,
        Terrain::new(source, 1024 * 1024, 8..=12),
        GroundSampler::default(),
    );
    let conditions = flightsim_sim::replay::Conditions {
        start: initial.geodetic(),
        ..default()
    }
    .with_aircraft(&config);
    let mut controls = profile.pilot_controls(false);
    controls.throttle.set_absolute(0.8);
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(controls)
        .insert_resource(SampledPilotInput::default())
        .insert_resource(flightsim_ui::Paused::default())
        .insert_resource(world_runtime::MapCapture::default())
        .insert_resource(FlightRecorder(flightsim_sim::Recorder::new(conditions)))
        .insert_resource(FlightSimulation(simulation))
        .add_systems(
            Update,
            advance_simulation.run_if(world_runtime::flight_controls_active),
        );
    configure_live_input_scheduling(&mut app);
    app
}

fn tick(app: &mut App, dt: Duration, keys: PilotKeys) {
    app.world_mut().resource_mut::<Time>().advance_by(dt);
    *app.world_mut().resource_mut::<SampledPilotInput>() = sample(keys);
    app.update();
}

fn run_partition(id: &str, fps: u32) -> App {
    let mut app = control_app(id);
    let mut previous = Duration::ZERO;
    for frame in 0..fps * 3 {
        // Nanosecond timestamps have identical shared endpoints. Repeating a
        // rounded 1/fps Duration would instead inject a genuinely different time.
        let now = Duration::from_secs_f64(f64::from(frame + 1) / f64::from(fps));
        let keys = PilotKeys {
            pitch_up: frame < fps / 3,
            throttle_up: frame < fps,
            trim_down: (fps..fps + fps / 3).contains(&frame),
            ..default()
        };
        tick(&mut app, now - previous, keys);
        previous = now;
    }
    app
}

#[test]
fn live_app_ramps_and_recorded_inputs_are_identical_at_shared_frame_boundaries() {
    for id in ["light-single", "swift-sport"] {
        let reference = run_partition(id, 60);
        let reference_sim = &reference.world().resource::<FlightSimulation>().0;
        let reference_recording = reference.world().resource::<FlightRecorder>().0.recording();
        assert_eq!(reference_recording.frames().len(), 360);
        for fps in [6, 30, 144] {
            let app = run_partition(id, fps);
            let sim = &app.world().resource::<FlightSimulation>().0;
            let recording = app.world().resource::<FlightRecorder>().0.recording();
            assert_eq!(sim.state(), reference_sim.state(), "{id} at {fps} Hz");
            assert_eq!(sim.elapsed(), reference_sim.elapsed());
            assert_eq!(recording.frames(), reference_recording.frames());
            assert_eq!(recording.keyframes(), reference_recording.keyframes());
        }
    }
}

#[test]
fn live_fixed_step_recording_replays_and_rewinds_through_actual_app_path() {
    for id in ["light-single", "swift-sport"] {
        let live = run_partition(id, 30);
        let expected = *live.world().resource::<FlightSimulation>().0.state();
        let recorded = live.world().resource::<FlightRecorder>().0.recording();
        let mut bytes = Vec::new();
        recorded.write_to(&mut bytes).unwrap();
        let recording = flightsim_sim::Recording::read_from(&mut bytes.as_slice()).unwrap();
        let mut app = control_app(id);
        app.insert_resource(ReplayPlayback::new(recording));
        for _ in 0..190 {
            tick(
                &mut app,
                Duration::from_secs_f64(1.0 / 60.0),
                PilotKeys {
                    pitch_down: true,
                    throttle_down: true,
                    ..default()
                },
            );
        }
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.state(),
            &expected
        );
        assert!(app.world().resource::<ReplayPlayback>().fault.is_none());
        assert!(
            app.world()
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );
        app.world_mut()
            .resource_scope(|world, mut playback: Mut<ReplayPlayback>| {
                playback.rewind(&mut world.resource_mut::<FlightSimulation>().0);
            });
        for _ in 0..190 {
            tick(
                &mut app,
                Duration::from_secs_f64(1.0 / 60.0),
                PilotKeys::default(),
            );
        }
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.state(),
            &expected
        );
        assert!(app.world().resource::<ReplayPlayback>().fault.is_none());
    }
}

#[test]
fn zero_step_frame_does_not_move_controls_or_make_a_recording() {
    let mut app = control_app("light-single");
    let before = app.world().resource::<PilotControls>().to_control_inputs();
    tick(
        &mut app,
        Duration::from_millis(1),
        PilotKeys {
            pitch_up: true,
            throttle_up: true,
            ..default()
        },
    );
    assert_eq!(
        app.world().resource::<PilotControls>().to_control_inputs(),
        before
    );
    assert!(
        app.world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .frames()
            .is_empty()
    );
    tick(
        &mut app,
        Duration::from_millis(8),
        PilotKeys {
            pitch_up: true,
            ..default()
        },
    );
    assert_eq!(
        app.world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .frames()
            .len(),
        1
    );
}

#[test]
fn pause_and_map_release_transients_without_changing_persistent_settings() {
    for map in [false, true] {
        let mut app = control_app("swift-sport");
        let keys = PilotKeys {
            pitch_up: true,
            roll_right: true,
            yaw_left: true,
            throttle_up: true,
            trim_up: true,
            flaps_extend: true,
            brakes: true,
            ..default()
        };
        tick(&mut app, Duration::from_millis(100), keys);
        let before = *app.world().resource::<FlightSimulation>().0.state();
        let count = app
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .frames()
            .len();
        let persistent = *app.world().resource::<PilotControls>();
        if map {
            app.world_mut()
                .resource_mut::<world_runtime::MapCapture>()
                .captured = true;
        } else {
            app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = true;
        }
        for _ in 0..20 {
            tick(&mut app, Duration::from_millis(100), keys);
        }
        let controls = app.world().resource::<PilotControls>();
        assert_eq!(controls.elevator.value(), 0.0);
        assert_eq!(controls.aileron.value(), 0.0);
        assert_eq!(controls.rudder.value(), 0.0);
        assert_eq!(controls.to_control_inputs().brakes(), 0.0);
        assert_eq!(controls.throttle, persistent.throttle);
        assert_eq!(controls.flaps, persistent.flaps);
        assert_eq!(controls.trim, persistent.trim);
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.state(),
            &before
        );
        assert_eq!(
            app.world()
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .len(),
            count
        );
        app.world_mut()
            .resource_mut::<world_runtime::MapCapture>()
            .captured = false;
        app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = false;
        tick(&mut app, Duration::from_millis(100), PilotKeys::default());
        assert_eq!(
            app.world().resource::<PilotControls>().elevator.value(),
            0.0
        );
    }
}

#[test]
fn focus_loss_drops_commands_but_does_not_secretly_pause_or_reset_trim() {
    let mut app = control_app("light-single");
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
    let before = *app.world().resource::<FlightSimulation>().0.state();
    let trim = app.world().resource::<PilotControls>().trim.value();
    tick(
        &mut app,
        Duration::from_millis(100),
        PilotKeys {
            pitch_up: true,
            trim_up: true,
            throttle_down: true,
            ..default()
        },
    );
    let controls = app.world().resource::<PilotControls>();
    assert_eq!(controls.elevator.value(), 0.0);
    assert_eq!(controls.trim.value(), trim);
    assert_eq!(controls.throttle.value(), 0.8);
    assert_ne!(
        app.world().resource::<FlightSimulation>().0.state(),
        &before
    );
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    tick(
        &mut app,
        Duration::from_millis(100),
        PilotKeys {
            pitch_up: true,
            ..default()
        },
    );
    assert!(app.world().resource::<PilotControls>().elevator.value() > 0.0);
}

#[test]
fn bundled_pitch_rate_is_gentler_without_reducing_roll_yaw_or_analog_range() {
    for id in ["light-single", "swift-sport"] {
        let profile = aircraft_profile::AircraftProfile::builtin(id).unwrap();
        let mut controls = profile.pilot_controls(false);
        controls.update_from_sample(
            Seconds(0.5),
            &sample(PilotKeys {
                pitch_up: true,
                roll_right: true,
                yaw_right: true,
                ..default()
            }),
        );
        assert!((controls.elevator.value() - 0.125).abs() < 1e-12);
        assert_eq!(controls.aileron.value(), 1.0);
        assert_eq!(controls.rudder.value(), 1.0);
        controls.update_from_sample(Seconds(0.025), &sample(PilotKeys::default()));
        assert_eq!(controls.elevator.value(), 0.0);
        assert!(controls.aileron.value() > 0.9);
        assert!(controls.rudder.value() > 0.9);
        controls.elevator.set_absolute(1.0);
        assert_eq!(controls.to_control_inputs().elevator(), 1.0);
    }
}

#[test]
fn bundled_ground_rotation_has_reaction_margin_and_unstalled_open_loop_climb() {
    for id in ["light-single", "swift-sport"] {
        for visible_pitch in [3.0, 5.0] {
            for reaction_ticks in [0_u32, 36] {
                let profile = aircraft_profile::AircraftProfile::builtin(id).unwrap();
                let config = profile.configuration();
                let lift_peak = flightsim_fdm::aero::positive_stall_peak_angle(
                    &config.aero,
                    &config.geometry,
                    0.0,
                )
                .unwrap();
                let mut simulation = Simulation::parked(
                    config,
                    Geodetic::from_degrees(35.54887, 139.77627, 0.0),
                    Degrees(50.0).to_radians(),
                    Terrain::new(MemoryTileSource::new(), 1024 * 1024, 8..=12),
                    GroundSampler::default(),
                );
                let mut controls = profile.pilot_controls(false);
                let mut pull_start = None;
                let mut release_tick = None;
                let mut first_lift = None;
                let mut peak_alpha = Radians::ZERO;
                for step in 0_u32..10_800 {
                    let eas = simulation.airspeed()
                        * simulation.atmosphere_sample().density_ratio().sqrt();
                    if pull_start.is_none()
                        && eas >= flightsim_core::Knots(75.0).to_meters_per_second()
                    {
                        pull_start = Some(step);
                    }
                    if pull_start.is_some()
                        && release_tick.is_none()
                        && simulation.state().attitude().pitch
                            >= Degrees(visible_pitch).to_radians()
                    {
                        release_tick = Some(step + reaction_ticks);
                    }
                    // A test pilot makes ONE pull then releases completely.
                    // No attitude/airspeed correction or automatic trim follows.
                    let command = sample(PilotKeys {
                        throttle_up: true,
                        pitch_up: pull_start.is_some() && release_tick.is_none_or(|at| step < at),
                        ..default()
                    });
                    simulation.advance_with_controls(Seconds(1.0 / 120.0), |dt, _| {
                        controls.update_from_sample(dt, &command);
                        controls.to_control_inputs()
                    });
                    assert!(!simulation.crashed() && !simulation.diverged(), "{id}");
                    if pull_start.is_some() {
                        peak_alpha = Radians(
                            peak_alpha
                                .get()
                                .max(simulation.aero_angles().angle_of_attack.get()),
                        );
                    }
                    if !simulation.on_ground() {
                        first_lift.get_or_insert(step);
                    } else {
                        assert!(
                            first_lift.is_none(),
                            "{id}: returned to ground after liftoff"
                        );
                    }
                }
                let context =
                    format!("{id}, pitch cue {visible_pitch}, reaction {reaction_ticks}/120s");
                let start = pull_start.expect("reached the training rotation speed");
                let release = release_tick.expect("nose visibly rose");
                let lift = first_lift.expect("took off from actual parked gear state");
                assert!(release > start && release - start < 360, "{context}");
                assert!(lift < release + 120, "{context}: no prompt liftoff");
                assert!(
                    peak_alpha < lift_peak,
                    "{context}: crossed modeled lift peak"
                );
                assert!(simulation.state().altitude() > Meters(100.0), "{context}");
                assert_eq!(controls.elevator.value(), 0.0);
                assert_eq!(controls.trim.value(), profile.controls.default_trim);
                assert_eq!(controls.throttle.value(), 1.0);
            }
        }
    }
}

#[test]
fn warning_onset_precedes_measured_lift_peak_and_has_hysteresis() {
    for config in [
        AircraftConfig::light_single(),
        aircraft_profile::AircraftProfile::builtin("swift-sport")
            .unwrap()
            .configuration(),
    ] {
        for flaps in [0.0, 0.5, 1.0] {
            let peak = flightsim_fdm::aero::positive_stall_peak_angle(
                &config.aero,
                &config.geometry,
                flaps,
            )
            .unwrap();
            let mut warning = StallWarningStatus::default();
            warning.update(&config, flaps, peak * 0.84, true);
            assert!(!warning.active && !warning.unavailable);
            warning.update(&config, flaps, peak * 0.86, true);
            assert!(warning.active);
            assert!(peak * STALL_WARNING_FRACTION < peak);
            warning.update(&config, flaps, peak * 0.80, true);
            assert!(warning.active);
            warning.update(&config, flaps, peak * 0.77, true);
            assert!(!warning.active);
            warning.update(&config, flaps, peak, false);
            assert!(!warning.active);
        }
    }
}

#[test]
fn unsupported_curve_clears_a_stale_warning_and_can_become_available_again() {
    let config = AircraftConfig::light_single();
    let mut warning = StallWarningStatus::default();
    warning.update(&config, 0.0, Degrees(20.0).to_radians(), true);
    assert!(warning.active);
    let mut unsupported = config.clone();
    unsupported.aero.stall_blend_rate = 0.0;
    warning.update(&unsupported, 0.0, Degrees(20.0).to_radians(), true);
    assert!(warning.unavailable && !warning.active);
    warning.update(&config, 0.0, Radians::ZERO, true);
    assert!(!warning.unavailable && !warning.active);
}

#[test]
fn density_equivalent_speed_differs_from_tas_in_thinner_and_warmer_air() {
    let tas = MetersPerSecond(50.0);
    let sea = flightsim_fdm::Atmosphere::standard().sample(Meters::ZERO);
    let high = flightsim_fdm::Atmosphere::standard().sample(Meters(3000.0));
    let warm = flightsim_fdm::Atmosphere::with_temperature_offset(20.0).sample(Meters(3000.0));
    let eas = |air: flightsim_fdm::AtmosphereSample| tas * air.density_ratio().sqrt();
    assert!((eas(sea).get() - tas.get()).abs() < 1e-12);
    assert!(eas(high) < tas);
    assert!(eas(warm) < eas(high));
}

#[test]
fn low_speed_arbitrary_alpha_does_not_trigger_stall_tone() {
    for speed in [0.0, 0.001, 4.99, 5.01] {
        let mut app = control_app("light-single");
        let alpha = Degrees(20.0).to_radians().get();
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 1500.0),
            Attitude::from_degrees(0.0, 0.0, 0.0),
            Ned::new(speed * alpha.cos(), 0.0, speed * alpha.sin()),
        );
        app.world_mut()
            .resource_mut::<FlightSimulation>()
            .0
            .restart_at(state);
        app.init_resource::<StallWarningStatus>()
            .init_resource::<flightsim_audio::AircraftSound>()
            .init_resource::<ViewMode>()
            .init_resource::<SunDirection>()
            .init_resource::<HudState>();
        configure_flight_presentation(&mut app);
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        let warning = app.world().resource::<StallWarningStatus>();
        assert!(!warning.unavailable);
        assert_eq!(warning.active, speed >= 5.0);
        assert_eq!(
            app.world()
                .resource::<flightsim_audio::AircraftSound>()
                .stall_warning,
            warning.active
        );
        let hud = app.world().resource::<HudState>();
        assert_eq!(hud.stall_warning, warning.active);
        assert_eq!(hud.stall_warning_unavailable, warning.unavailable);
        let simulation = &app.world().resource::<FlightSimulation>().0;
        let expected =
            simulation.airspeed() * simulation.atmosphere_sample().density_ratio().sqrt();
        assert_eq!(hud.equivalent_airspeed, expected);
        assert_eq!(hud.airspeed, simulation.airspeed());
    }
}

#[test]
fn warning_transition_reaches_audio_bridge_and_rendered_text_in_same_update() {
    let mut app = control_app("light-single");
    let bridge = std::sync::Arc::new(flightsim_audio::SharedSound::default());
    app.init_resource::<StallWarningStatus>()
        .init_resource::<flightsim_audio::AircraftSound>()
        .init_resource::<flightsim_audio::AudioSettings>()
        .insert_resource(flightsim_audio::SoundBridge(bridge.clone()))
        .init_resource::<ViewMode>()
        .init_resource::<SunDirection>()
        .init_resource::<HudState>()
        .init_resource::<flightsim_ui::HudSmoothing>()
        .add_systems(Update, flightsim_audio::publish_sound)
        .add_systems(Update, flightsim_ui::update_hud);
    let text = app
        .world_mut()
        .spawn((Text::default(), flightsim_ui::HudText))
        .id();
    configure_flight_presentation(&mut app);
    for (alpha_degrees, expected) in [(20.0, true), (0.0, false), (20.0, true)] {
        let alpha = Degrees(alpha_degrees).to_radians().get();
        let state = RigidBodyState::from_geodetic(
            Geodetic::from_degrees(35.55, 139.78, 1500.0),
            Attitude::from_degrees(0.0, 0.0, 0.0),
            Ned::new(40.0 * alpha.cos(), 0.0, 40.0 * alpha.sin()),
        );
        app.world_mut()
            .resource_mut::<FlightSimulation>()
            .0
            .restart_at(state);
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_eq!(bridge.get().stall_warning, expected);
        assert_eq!(app.world().resource::<HudState>().stall_warning, expected);
        assert_eq!(
            app.world()
                .get::<Text>(text)
                .unwrap()
                .contains("STALL WARN"),
            expected
        );
    }
}

#[test]
fn restart_after_sampling_clears_held_commands_before_first_recorded_step() {
    let mut app = control_app("light-single");
    let mut startup = Startup::default();
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    let start = StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    };
    let expected_trim = startup.aircraft.controls.default_trim;
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::KeyR);
    keyboard.press(KeyCode::KeyS);
    keyboard.press(KeyCode::PageUp);
    app.insert_resource(startup)
        .insert_resource(start)
        .insert_resource(keyboard)
        .init_resource::<InputDevices>()
        .init_resource::<flightsim_input::InputSettings>()
        .init_resource::<CameraRig>()
        .init_resource::<flightsim_ui::TutorialState>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .insert_resource(flightsim_audio::SoundBridge(std::sync::Arc::new(
            flightsim_audio::SharedSound::default(),
        )))
        .insert_resource(flightsim_render::TimeOfDay {
            utc: flightsim_render::JulianDate::J2000,
            rate: flightsim_render::TimeRate::REAL_TIME,
        })
        .add_systems(
            Update,
            flightsim_input::sample_pilot_input.in_set(flightsim_input::InputSystems::Sample),
        )
        .add_systems(Update, control_flight.before(advance_simulation));
    tick(&mut app, Duration::from_millis(20), PilotKeys::default());
    let recording = app.world().resource::<FlightRecorder>().0.recording();
    assert_eq!(recording.frames().len(), 2);
    for frame in recording.frames() {
        assert_eq!(frame.controls.elevator(), expected_trim);
        assert_eq!(frame.controls.throttle(), 0.0);
    }
}
