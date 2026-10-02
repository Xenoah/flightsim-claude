//! Integration regressions for application scheduling and replay presentation.
use super::*;

fn simulation() -> FlightSimulation {
    let source: BoxedSource = Box::new(MemoryTileSource::new());
    FlightSimulation(Simulation::parked(
        AircraftConfig::light_single(),
        Geodetic::from_degrees(35.55, 139.78, 0.0),
        Radians::ZERO,
        Terrain::new(source, 1024, 8..=12),
        GroundSampler::default(),
    ))
}

fn camera_app(mode: ViewMode) -> (App, Entity, Entity) {
    let start = Geodetic::from_degrees(35.55, 139.78, 0.0);
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(mode)
        .insert_resource(RenderOrigin::new(start))
        .insert_resource(CameraWorldPosition(start))
        .insert_resource(TowerViewAnchor(Geodetic::from_degrees(
            35.55, 139.78, 725.0,
        )))
        .insert_resource(CameraRig::default())
        .insert_resource(simulation())
        .configure_sets(
            Update,
            (RenderSet::Rebase, RenderSet::Transforms, RenderSet::Sun).chain(),
        )
        .add_systems(
            Update,
            (
                flightsim_render::rebase_render_origin.in_set(RenderSet::Rebase),
                flightsim_render::apply_world_positions.in_set(RenderSet::Transforms),
            ),
        );
    configure_camera_tracking(&mut app);
    let aircraft = app
        .world_mut()
        .spawn((
            Aircraft,
            WorldPosition(start.to_ecef()),
            WorldOrientation(LocalFrame::new(start).ned_to_ecef_rotation()),
            Transform::default(),
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), Transform::default()))
        .id();
    (app, aircraft, camera)
}

#[test]
fn chase_and_free_cameras_do_not_keep_coordinates_from_the_old_origin() {
    for mode in [ViewMode::Chase, ViewMode::Free] {
        let (mut app, aircraft, camera) = camera_app(mode);
        app.update();
        let far = Geodetic::from_degrees(35.60, 139.78, 0.0);
        app.world_mut().resource_mut::<CameraWorldPosition>().0 = far;
        app.world_mut()
            .get_mut::<WorldPosition>(aircraft)
            .unwrap()
            .0 = far.to_ecef();
        app.update();
        let plane = app.world().get::<Transform>(aircraft).unwrap();
        let actual = app.world().get::<Transform>(camera).unwrap().translation;
        let expected = match mode {
            ViewMode::Chase => {
                plane.translation + plane.rotation * Vec3::new(-35.0, 0.0, 0.0) + Vec3::Y * 10.0
            }
            _ => plane.translation + Vec3::new(120.0, 60.0, 120.0),
        };
        assert!(
            (actual - expected).length() < 0.001,
            "{mode:?}: {actual:?} != {expected:?}"
        );
        // Zero dt deliberately preserves smoothing on an ordinary non-rebase frame.
        app.world_mut()
            .get_mut::<WorldPosition>(aircraft)
            .unwrap()
            .0 = far.offset_by(Meters(1.0), Meters::ZERO).to_ecef();
        app.update();
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            actual
        );
    }
}

#[test]
fn tower_keeps_its_elevated_world_anchor_and_lod_observation_while_aircraft_moves() {
    let (mut app, aircraft, camera) = camera_app(ViewMode::Tower);
    app.add_systems(
        Update,
        update_camera_world_position.before(RenderSet::Rebase),
    );
    app.update();
    let anchor = app.world().resource::<TowerViewAnchor>().0;
    assert_eq!(app.world().resource::<CameraWorldPosition>().0, anchor);
    let far = Geodetic::from_degrees(36.0, 140.0, 3000.0);
    let state = flightsim_fdm::RigidBodyState::from_geodetic(
        far,
        Attitude::from_degrees(0.0, 0.0, 0.0),
        Ned::new(40.0, 0.0, 0.0),
    );
    app.world_mut()
        .resource_mut::<FlightSimulation>()
        .0
        .restart_at(state);
    app.world_mut()
        .get_mut::<WorldPosition>(aircraft)
        .unwrap()
        .0 = far.to_ecef();
    app.update();
    assert_eq!(app.world().resource::<CameraWorldPosition>().0, anchor);
    let expected = app
        .world()
        .resource::<RenderOrigin>()
        .0
        .to_render(anchor.to_ecef());
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation,
        expected
    );
    assert_eq!(anchor.altitude, Meters(725.0));
}

#[test]
fn new_recording_uses_current_visual_epoch_and_restored_rate() {
    let mut recorder = FlightRecorder(flightsim_sim::Recorder::new(
        flightsim_sim::replay::Conditions::default(),
    ));
    let clock = flightsim_render::TimeOfDay {
        utc: flightsim_render::JulianDate::J2000,
        rate: flightsim_render::TimeRate(600.0),
    };
    refresh_recording_clock(&mut recorder, &clock);
    assert_eq!(
        recorder.0.recording().conditions().start_epoch.to_bits(),
        clock.utc.get().to_bits()
    );
    assert_eq!(
        recorder.0.recording().conditions().time_rate.to_bits(),
        600.0_f64.to_bits()
    );
}

#[test]
fn invalid_fallback_replay_epoch_does_not_poison_the_sun() {
    let sim = simulation();
    let mut recorder = flightsim_sim::Recorder::new(flightsim_sim::replay::Conditions::default());
    recorder.record(
        Seconds(0.02),
        flightsim_fdm::ControlInputs::neutral(),
        Some(sim.0.state()),
    );
    let mut playback = ReplayPlayback::new(recorder.finish());
    playback.elapsed = Seconds(86400.0);
    let epoch = flightsim_render::JulianDate(flightsim_sim::replay::MAX_VISUAL_EPOCH);
    let mut app = App::new();
    app.insert_resource(playback)
        .insert_resource(flightsim_render::TimeOfDay {
            utc: epoch,
            ..default()
        })
        .add_systems(Update, sync_replay_clock);
    app.update();
    assert!(app.world().resource::<ReplayPlayback>().fault.is_some());
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().utc,
        epoch
    );
}

fn single_frame_recording(
    state: &flightsim_fdm::RigidBodyState,
    conditions: flightsim_sim::replay::Conditions,
) -> flightsim_sim::Recording {
    let mut recorder = flightsim_sim::Recorder::new(conditions);
    recorder.record(
        Seconds(0.02),
        flightsim_fdm::ControlInputs::neutral().with_throttle(0.42),
        Some(state),
    );
    recorder.finish()
}

#[test]
fn resolved_visual_clock_checks_origin_rate_elapsed_and_finite_products() {
    let epoch = flightsim_render::JulianDate::J2000;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        assert!(replay_visual_epoch(epoch, Seconds(value), 1.0).is_none());
        assert!(replay_visual_epoch(epoch, Seconds(1.0), value).is_none());
        assert!(
            replay_visual_epoch(flightsim_render::JulianDate(value), Seconds::ZERO, 1.0).is_none()
        );
    }
    assert!(replay_visual_epoch(epoch, Seconds(f64::MAX), 2.0).is_none());
    assert_eq!(
        replay_visual_epoch(epoch, Seconds(f64::MAX), 0.0),
        Some(epoch)
    );
    let limit = flightsim_render::JulianDate(flightsim_sim::replay::MAX_VISUAL_EPOCH);
    assert_eq!(replay_visual_epoch(limit, Seconds::ZERO, 1.0), Some(limit));
    assert!(replay_visual_epoch(limit, Seconds(86_400.0), 1.0).is_none());
    // The decoder checks the zero-sentinel offset; only the app knows its real origin.
    let nearly_maximum_duration =
        Seconds((flightsim_sim::replay::MAX_VISUAL_EPOCH - 1.0) * 86_400.0);
    assert!(
        replay_visual_epoch(
            flightsim_render::JulianDate(0.0),
            nearly_maximum_duration,
            1.0
        )
        .is_some()
    );
    assert!(replay_visual_epoch(epoch, nearly_maximum_duration, 1.0).is_none());
}

#[test]
fn faulted_replay_keeps_last_good_world_transform_on_every_update() {
    let mut sim = simulation();
    let initial = *sim.0.state();
    let recording = single_frame_recording(&initial, flightsim_sim::replay::Conditions::default());
    let mut invalid = initial;
    invalid.position = flightsim_core::Ecef::new(1e30, 0.0, 0.0);
    invalid.orientation = bevy::math::DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0);
    sim.0.restart_at(invalid);
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(PilotControls::default())
        .insert_resource(flightsim_ui::Paused::default())
        .insert_resource(FlightRecorder(flightsim_sim::Recorder::new(
            flightsim_sim::replay::Conditions::default(),
        )))
        .insert_resource(ReplayPlayback::new(recording))
        .insert_resource(ViewMode::Cockpit)
        .insert_resource(TowerViewAnchor(initial.geodetic()))
        .insert_resource(CameraWorldPosition(initial.geodetic()))
        .insert_resource(sim)
        .add_systems(
            Update,
            (advance_simulation, update_camera_world_position).chain(),
        );
    let entity = app
        .world_mut()
        .spawn((
            Aircraft,
            WorldPosition(initial.position),
            WorldOrientation(initial.orientation),
        ))
        .id();
    for _ in 0..3 {
        app.update();
        assert!(app.world().resource::<ReplayPlayback>().fault.is_some());
        assert_eq!(
            app.world().get::<WorldPosition>(entity).unwrap().0,
            initial.position
        );
        assert_eq!(
            app.world().get::<WorldOrientation>(entity).unwrap().0,
            initial.orientation
        );
        assert_eq!(
            app.world().resource::<CameraWorldPosition>().0,
            initial.geodetic()
        );
    }
}

#[test]
fn replay_disables_tutorial_and_live_visual_rate_keys() {
    let sim = simulation();
    let recording =
        single_frame_recording(sim.0.state(), flightsim_sim::replay::Conditions::default());
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::KeyH);
    keyboard.press(KeyCode::Period);
    let clock = flightsim_render::TimeOfDay {
        rate: flightsim_render::TimeRate(60.0),
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(keyboard)
        .insert_resource(ReplayPlayback::new(recording))
        .insert_resource(flightsim_ui::Paused::default())
        .insert_resource(flightsim_ui::TutorialVisibility(true))
        .insert_resource(clock)
        .add_systems(Update, (toggle_tutorial, adjust_time_rate));
    app.update();
    assert!(!app.world().resource::<flightsim_ui::TutorialVisibility>().0);
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().rate,
        clock.rate
    );
}

#[test]
fn replay_sound_and_completion_status_use_recorded_inputs() {
    let sim = simulation();
    let recording =
        single_frame_recording(sim.0.state(), flightsim_sim::replay::Conditions::default());
    let mut controls = PilotControls::default();
    controls.throttle.set_absolute(0.9);
    let mut app = App::new();
    app.insert_resource(sim)
        .insert_resource(controls)
        .insert_resource(ReplayPlayback::new(recording))
        .insert_resource(flightsim_ui::Paused::default())
        .insert_resource(flightsim_ui::ReplayStatus::default())
        .insert_resource(flightsim_audio::AircraftSound::default())
        .add_systems(Update, (publish_sound, publish_replay_status));
    app.update();
    let sound = app.world().resource::<flightsim_audio::AircraftSound>();
    assert_eq!(sound.throttle.to_bits(), 0.42_f64.to_bits());
    assert!(!sound.muted);
    app.world_mut()
        .resource_mut::<ReplayPlayback>()
        .player
        .set_paused(true);
    app.update();
    assert!(
        app.world()
            .resource::<flightsim_audio::AircraftSound>()
            .muted
    );
    app.world_mut()
        .resource_mut::<ReplayPlayback>()
        .player
        .set_paused(false);
    app.world_mut()
        .resource_mut::<ReplayPlayback>()
        .player
        .step_once();
    app.update();
    assert!(
        app.world()
            .resource::<flightsim_audio::AircraftSound>()
            .muted
    );
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert!(status.active && status.finished);
    assert!(flightsim_ui::replay::format_replay_banner(status).contains("REPLAY COMPLETE"));
}

#[test]
fn restart_while_paused_restores_rate_and_records_current_epoch() {
    let startup = Startup::default();
    let mut rig = CameraRig::default();
    rig.follow(Vec3::splat(123.0), Seconds::ZERO);
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::Escape);
    let mut app = App::new();
    app.insert_resource(StartCondition::Parked {
        position: startup.start,
        heading: startup.heading,
    })
    .insert_resource(startup)
    .insert_resource(simulation())
    .insert_resource(rig)
    .insert_resource(PilotControls::default())
    .insert_resource(FlightRecorder(flightsim_sim::Recorder::new(
        flightsim_sim::replay::Conditions::default(),
    )))
    .insert_resource(flightsim_ui::Paused::default())
    .insert_resource(flightsim_ui::TutorialState::default())
    .insert_resource(flightsim_ui::LandingReportState::default())
    .insert_resource(flightsim_audio::SoundBridge(std::sync::Arc::new(
        flightsim_audio::SharedSound::default(),
    )))
    .insert_resource(flightsim_render::TimeOfDay {
        utc: flightsim_render::JulianDate::J2000,
        rate: flightsim_render::TimeRate(600.0),
    })
    .insert_resource(keyboard)
    .add_systems(Update, control_flight);
    app.update();
    assert!(app.world().resource::<flightsim_ui::Paused>().is_paused());
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().rate,
        flightsim_render::TimeRate::PAUSED
    );
    let epoch = flightsim_render::JulianDate::J2000.advanced_by(Seconds(123.0));
    app.world_mut()
        .resource_mut::<flightsim_render::TimeOfDay>()
        .utc = epoch;
    let mut keyboard = ButtonInput::default();
    keyboard.press(KeyCode::KeyR);
    app.insert_resource(keyboard);
    app.update();
    assert!(!app.world().resource::<flightsim_ui::Paused>().is_paused());
    assert_eq!(
        app.world().resource::<flightsim_render::TimeOfDay>().rate,
        flightsim_render::TimeRate(600.0)
    );
    let conditions = app
        .world()
        .resource::<FlightRecorder>()
        .0
        .recording()
        .conditions();
    assert_eq!(conditions.start_epoch.to_bits(), epoch.get().to_bits());
    assert_eq!(conditions.time_rate.to_bits(), 600.0_f64.to_bits());
    assert_eq!(
        app.world_mut()
            .resource_mut::<CameraRig>()
            .follow(Vec3::ZERO, Seconds::ZERO),
        Vec3::ZERO
    );
}

#[test]
fn replay_location_and_weather_win_before_airport_and_difficulty_resolution() {
    let directory =
        std::env::temp_dir().join(format!("flightsim-replay-sources-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let replay_path = directory.join("recorded.fsreplay");
    let airports_path = directory.join("airports.fsairports");
    let mut startup = Startup {
        replay: Some(replay_path.clone()),
        airports: Some(airports_path.clone()),
        difficulty: Difficulty::Realistic,
        approach: Some(2.0),
        drop_height: Some(100.0),
        ..default()
    };
    let recorded_start = Geodetic::from_degrees(0.0, 0.0, 100.0);
    let initial = flightsim_fdm::RigidBodyState::from_geodetic(
        recorded_start,
        Attitude::from_degrees(0.0, 0.0, 123.0),
        Ned::new(40.0, 0.0, 0.0),
    );
    let conditions = flightsim_sim::replay::Conditions {
        start: recorded_start,
        heading: Degrees(123.0).to_radians(),
        time_rate: 60.0,
        ..default()
    }
    .with_aircraft(&startup.aircraft.configuration());
    single_frame_recording(&initial, conditions.clone())
        .write_to(&mut std::fs::File::create(&replay_path).unwrap())
        .unwrap();
    let near_recording = flightsim_world::AirportRunway::from_endpoints(
        111,
        recorded_start,
        Geodetic::from_degrees(0.01, 0.0, 100.0),
        Meters(45.0),
    )
    .unwrap();
    let near_default = flightsim_world::AirportRunway::from_endpoints(
        222,
        startup.start,
        startup.start.offset_by(Meters(1000.0), Meters::ZERO),
        Meters(45.0),
    )
    .unwrap();
    AirportDatabase::new(vec![near_recording, near_default])
        .unwrap()
        .write_to(&mut std::fs::File::create(&airports_path).unwrap())
        .unwrap();
    let mut diagnostics = StartupDiagnostics::default();
    let recording = resolve_flight_sources(&mut startup, &mut diagnostics).unwrap();
    assert!(diagnostics.0.is_empty(), "{:?}", diagnostics.0);
    assert_eq!(
        startup.runway_source,
        RunwaySource::OpenStreetMap { way_id: 111 }
    );
    assert_eq!(startup.start, recorded_start);
    assert_eq!(startup.heading, conditions.heading);
    assert_eq!(startup.wind, conditions.wind);
    assert_eq!(startup.turbulence, conditions.turbulence);
    assert_eq!(startup.time_rate.to_bits(), conditions.time_rate.to_bits());
    assert_eq!(recording.keyframe_exactly_at(0).unwrap().state, initial);
    assert!(startup.approach.is_none() && startup.drop_height.is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn format_valid_but_unrenderable_replays_are_rejected_before_setup() {
    let directory = std::env::temp_dir().join(format!(
        "flightsim-replay-render-boundary-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("extreme.fsreplay");
    for magnitude in [1e30, 1e39] {
        for invalid_start_only in [false, true] {
            let mut startup = Startup {
                replay: Some(path.clone()),
                ..default()
            };
            let mut state = *simulation().0.state();
            let mut conditions = flightsim_sim::replay::Conditions::default()
                .with_aircraft(&startup.aircraft.configuration());
            if invalid_start_only {
                conditions.start.altitude = Meters(magnitude);
            } else {
                state.position = flightsim_core::Ecef::new(magnitude, 0.0, 0.0);
            }
            let recording = single_frame_recording(&state, conditions);
            let mut bytes = Vec::new();
            recording.write_to(&mut bytes).unwrap();
            // These remain valid persisted data. Only the renderer has the f32 limit.
            let decoded = flightsim_sim::Recording::read_from(&mut &bytes[..]).unwrap();
            assert_eq!(decoded, recording);
            std::fs::write(&path, bytes).unwrap();
            let mut diagnostics = StartupDiagnostics::default();
            assert!(resolve_replay(&mut startup, &mut diagnostics).is_none());
            assert!(startup.replay.is_none());
            assert!(
                diagnostics
                    .0
                    .iter()
                    .any(|message| message.contains("cannot be rendered")
                        && message.contains("render-coordinate range"))
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
