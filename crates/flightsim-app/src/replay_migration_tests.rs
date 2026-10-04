//! Application acceptance and exact playback across retained file versions.
use super::*;
use crate::world_runtime::startup_clock;
use controls_runtime_tests::{control_app, tick};
use flightsim_input::PilotKeys;
use flightsim_sim::{CurrentRecording, ReplayFile, replay::identity::AircraftCompatibility};
use std::time::Duration;

fn live_flight(id: &str) -> App {
    let mut app = control_app(id);
    let clock = flightsim_render::TimeOfDay {
        utc: flightsim_render::JulianDate::J2000,
        rate: flightsim_render::TimeRate(60.0),
    };
    refresh_recording_clock(
        &mut app.world_mut().resource_mut::<FlightRecorder>(),
        &clock,
    );
    for frame in 0..450 {
        tick(
            &mut app,
            Duration::from_secs_f64(1.0 / 30.0),
            PilotKeys {
                pitch_up: frame < 10,
                throttle_up: frame < 30,
                trim_down: (30..40).contains(&frame),
                ..default()
            },
        );
    }
    assert!(!app.world().resource::<FlightSimulation>().0.crashed());
    assert!(!app.world().resource::<FlightSimulation>().0.diverged());
    app
}

// Construct known-baseline legacy test fixtures, never a production conversion
// from old evidence to complete identity. V2 deliberately has a disabled world.
fn fixture(recording: &CurrentRecording, config: &AircraftConfig, version: u16) -> ReplayFile {
    if version == 3 {
        return ReplayFile::V3(recording.clone());
    }
    let environment = recording.conditions().environment;
    let conditions = flightsim_sim::replay::Conditions {
        start: environment.start,
        heading: environment.heading,
        wind: environment.wind,
        turbulence: environment.turbulence,
        start_epoch: environment.start_epoch,
        time_rate: environment.time_rate,
        ..default()
    }
    .with_aircraft(config);
    let mut recorder = flightsim_sim::Recorder::new(conditions);
    for (index, frame) in recording.frames().iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        let keyframe = recording.keyframe_exactly_at(index);
        recorder.record(
            frame.frame_time,
            frame.controls,
            keyframe.as_ref().map(|key| &key.state),
        );
    }
    match version {
        1 => ReplayFile::V1(recorder.finish()),
        2 => ReplayFile::V2(recorder.finish()),
        _ => unreachable!(),
    }
}

fn bytes(file: &ReplayFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    file.write_to(&mut bytes).unwrap();
    bytes
}

fn replay_app(id: &str, recording: ReplayFile) -> (App, Entity) {
    let epoch = recording.environment().start_epoch;
    let mut app = control_app(id);
    app.insert_resource(ReplayPlayback::new(recording))
        .insert_resource(flightsim_render::TimeOfDay {
            utc: flightsim_render::JulianDate(epoch),
            rate: flightsim_render::TimeRate::REAL_TIME,
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<flightsim_ui::LandingReportState>()
        .init_resource::<flightsim_ui::ReplayStatus>()
        .init_resource::<flightsim_audio::AircraftSound>()
        .init_resource::<StallWarningStatus>()
        .init_resource::<CameraRig>()
        .add_systems(Update, control_replay.before(advance_simulation))
        .add_systems(
            Update,
            (
                sync_replay_clock,
                publish_replay_status,
                flightsim_ui::replay::update_replay_banner,
            )
                .chain()
                .after(advance_simulation),
        )
        .add_systems(Update, publish_sound.after(advance_simulation));
    let banner = app
        .world_mut()
        .spawn((
            Text::default(),
            Visibility::Hidden,
            flightsim_ui::replay::ReplayBanner,
        ))
        .id();
    (app, banner)
}

fn key(app: &mut App, key: KeyCode) {
    *app.world_mut().resource_mut::<ButtonInput<KeyCode>>() = ButtonInput::default();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    tick(app, Duration::ZERO, PilotKeys::default());
    *app.world_mut().resource_mut::<ButtonInput<KeyCode>>() = ButtonInput::default();
}

fn assert_clock_notice_and_source(app: &App, banner: Entity, original: &[u8], version: u16) {
    let replay = app.world().resource::<ReplayPlayback>();
    assert!(replay.fault.is_none(), "{:?}", replay.fault);
    assert_eq!(bytes(replay.player.recording()), original);
    assert_eq!(replay.player.recording().format_version(), version);
    let clock = app.world().resource::<flightsim_render::TimeOfDay>();
    let expected = flightsim_render::JulianDate::J2000.advanced_by(replay.elapsed * 60.0);
    assert_eq!(clock.utc, expected);
    assert_eq!(clock.rate, flightsim_render::TimeRate::PAUSED);
    let status = app.world().resource::<flightsim_ui::ReplayStatus>();
    assert_eq!(
        status.notice.as_deref(),
        (version < 3).then_some(replay_policy::LEGACY_NOTICE)
    );
    assert_eq!(
        *app.world().get::<Visibility>(banner).unwrap(),
        Visibility::Visible
    );
    let text = app.world().get::<Text>(banner).unwrap();
    assert_eq!(
        text.contains("historical yaw_rate_p was not recorded or verified"),
        version < 3
    );
}

#[test]
fn light_and_swift_all_versions_keep_exact_controls_clock_rewind_and_export() {
    for id in ["light-single", "swift-sport"] {
        let live = live_flight(id);
        let reference = &live.world().resource::<FlightSimulation>().0;
        let current = live.world().resource::<FlightRecorder>().0.recording();
        for version in [1, 2, 3] {
            let source = fixture(current, reference.config(), version);
            let original = bytes(&source);
            let source = ReplayFile::read_from(&mut original.as_slice()).unwrap();
            assert_eq!(
                source.check_compatibility_with(reference.config()).unwrap(),
                if version == 3 {
                    AircraftCompatibility::CompleteMatch
                } else {
                    AircraftCompatibility::LegacyPartialMatch
                }
            );
            assert_eq!(
                replay_policy::validate_playback(&source, reference.config(), true, false).unwrap(),
                (version < 3).then_some(replay_policy::LEGACY_NOTICE)
            );
            let (mut app, banner) = replay_app(id, source);
            tick(&mut app, Duration::ZERO, PilotKeys::default());
            key(&mut app, KeyCode::F5);
            let initial = *app.world().resource::<FlightSimulation>().0.state();
            tick(
                &mut app,
                Duration::from_secs(90),
                PilotKeys {
                    pitch_down: true,
                    ..default()
                },
            );
            assert_eq!(
                *app.world().resource::<FlightSimulation>().0.state(),
                initial
            );
            assert!(
                app.world()
                    .resource::<flightsim_audio::AircraftSound>()
                    .muted
            );
            assert_clock_notice_and_source(&app, banner, &original, version);
            key(&mut app, KeyCode::F7);
            key(&mut app, KeyCode::F6);
            assert_eq!(
                app.world()
                    .resource::<ReplayPlayback>()
                    .player
                    .speed()
                    .to_bits(),
                1.0_f64.to_bits()
            );
            key(&mut app, KeyCode::F7);
            key(&mut app, KeyCode::F5);
            // Pause discarded the long wall-time budget: resuming at dt=0 cannot jump.
            assert_eq!(app.world().resource::<ReplayPlayback>().player.cursor(), 0);
            for pass in 0..2 {
                for _ in 0..300 {
                    if app
                        .world()
                        .resource::<ReplayPlayback>()
                        .player
                        .is_finished()
                    {
                        break;
                    }
                    tick(
                        &mut app,
                        Duration::from_millis(37),
                        PilotKeys {
                            pitch_down: true,
                            throttle_down: true,
                            yaw_right: true,
                            ..default()
                        },
                    );
                }
                let replay = app.world().resource::<ReplayPlayback>();
                assert!(replay.player.is_finished(), "{id} v{version} pass {pass}");
                assert_eq!(
                    replay.last_controls,
                    current.frames().last().unwrap().controls
                );
                let actual = &app.world().resource::<FlightSimulation>().0;
                assert_eq!(actual.state(), reference.state());
                assert_eq!(actual.elapsed(), reference.elapsed());
                assert_eq!(actual.log(), reference.log());
                assert_eq!(actual.crash(), reference.crash());
                assert_eq!(actual.last_touchdown(), reference.last_touchdown());
                assert!(
                    app.world()
                        .resource::<FlightRecorder>()
                        .0
                        .recording()
                        .frames()
                        .is_empty()
                );
                assert!(
                    app.world()
                        .resource::<flightsim_audio::AircraftSound>()
                        .muted
                );
                assert_clock_notice_and_source(&app, banner, &original, version);
                if pass == 0 {
                    key(&mut app, KeyCode::F8);
                    assert_eq!(
                        app.world().resource::<ReplayPlayback>().player.cursor(),
                        240
                    );
                    assert!(app.world().resource::<ReplayPlayback>().is_seeking());
                    assert!(
                        app.world()
                            .resource::<flightsim_audio::AircraftSound>()
                            .muted
                    );
                    assert_clock_notice_and_source(&app, banner, &original, version);
                    key(&mut app, KeyCode::F5);
                    while app.world().resource::<ReplayPlayback>().is_seeking() {
                        tick(&mut app, Duration::from_secs(90), PilotKeys::default());
                    }
                    let cursor = app.world().resource::<ReplayPlayback>().player.cursor();
                    assert!(
                        cursor > 240 && cursor < u32::try_from(current.frames().len()).unwrap()
                    );
                    let mut prefix = control_app(id);
                    for frame in current.frames().iter().take(cursor as usize) {
                        prefix
                            .world_mut()
                            .resource_mut::<FlightSimulation>()
                            .0
                            .legacy_mut()
                            .advance(frame.frame_time, frame.controls);
                    }
                    let prefix = &prefix.world().resource::<FlightSimulation>().0;
                    let actual = &app.world().resource::<FlightSimulation>().0;
                    assert_eq!(actual.state(), prefix.state());
                    assert_eq!(actual.elapsed(), prefix.elapsed());
                    assert_eq!(actual.log(), prefix.log());
                    assert_clock_notice_and_source(&app, banner, &original, version);
                    key(&mut app, KeyCode::F5);
                }
            }
        }
    }
}

#[test]
fn startup_requires_opt_in_without_changing_original_file_or_conditions_on_rejection() {
    let directory =
        std::env::temp_dir().join(format!("flightsim-replay-policy-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for id in ["light-single", "swift-sport"] {
        let live = live_flight(id);
        let current = live.world().resource::<FlightRecorder>().0.recording();
        let config = live.world().resource::<FlightSimulation>().0.config();
        for version in [1, 2, 3] {
            let original = bytes(&fixture(current, config, version));
            let path = directory.join(format!("{id}-v{version}.fsreplay"));
            std::fs::write(&path, &original).unwrap();
            for opt_in in [false, true] {
                let mut args = vec![
                    "--aircraft".into(),
                    id.into(),
                    "--replay".into(),
                    path.to_str().unwrap().into(),
                ];
                if opt_in {
                    args.push("--legacy-replay-compatibility".into());
                }
                let (mut startup, mut diagnostics) = parse_arguments_from(args);
                assert!(diagnostics.0.is_empty());
                let before = recording_conditions(&startup, &startup_clock(&startup));
                let result = resolve_flight_sources(&mut startup, &mut diagnostics);
                if version == 3 || opt_in {
                    let result = result.unwrap();
                    assert_eq!(bytes(&result), original);
                    assert_eq!(startup.start, current.conditions().environment.start);
                    assert_eq!(
                        diagnostics
                            .0
                            .iter()
                            .any(|line| line.contains("LEGACY PARTIAL IDENTITY")),
                        version < 3
                    );
                } else {
                    assert!(result.is_none());
                    assert!(
                        diagnostics
                            .0
                            .iter()
                            .any(|line| line.contains("--legacy-replay-compatibility"))
                    );
                    assert_eq!(
                        recording_conditions(&startup, &startup_clock(&startup)),
                        before
                    );
                }
                assert_eq!(std::fs::read(&path).unwrap(), original);
            }
            // A profile's missing legacy yaw coefficient cannot be supplied by the flag.
            let mut startup = Startup {
                aircraft: aircraft_profile::AircraftProfile::builtin(id)
                    .unwrap()
                    .into(),
                replay: Some(path),
                legacy_replay_compatibility: true,
                ..default()
            };
            let aircraft_profile::SelectedAircraftProfile::Legacy(profile) = &mut startup.aircraft
            else {
                unreachable!()
            };
            profile.dynamics.aero.yaw_rate_p += 0.01;
            let before = recording_conditions(&startup, &startup_clock(&startup));
            let mut diagnostics = StartupDiagnostics::default();
            assert!(resolve_flight_sources(&mut startup, &mut diagnostics).is_none());
            assert_eq!(
                recording_conditions(&startup, &startup_clock(&startup)),
                before
            );
            assert!(
                diagnostics
                    .0
                    .iter()
                    .any(|line| line.contains(if version == 3 {
                        "mismatch"
                    } else {
                        "custom legacy dynamics"
                    }))
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn supported_weather_loads_exactly_and_manual_cloud_flags_fail_before_mutation() {
    let directory = std::env::temp_dir().join(format!(
        "flightsim-replay-weather-policy-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("weather.fsreplay");
    for mut fixture in [
        include_bytes!("../../flightsim-sim/tests/fixtures/v3_clear.fsreplay").as_slice(),
        include_bytes!("../../flightsim-sim/tests/fixtures/v3_rain.fsreplay").as_slice(),
        include_bytes!("../../flightsim-sim/tests/fixtures/v3_fog.fsreplay").as_slice(),
        include_bytes!("../../flightsim-sim/tests/fixtures/v3_custom_both.fsreplay").as_slice(),
    ] {
        std::fs::write(&path, fixture).unwrap();
        // These independently encoded weather fixtures identify Light Single,
        // regardless of the current distribution's default aircraft.
        let mut startup = Startup {
            aircraft: aircraft_profile::SelectedAircraftProfile::builtin("light-single").unwrap(),
            replay: Some(path.clone()),
            ..default()
        };
        let expected = ReplayFile::read_from(&mut fixture).unwrap().weather();
        let mut diagnostics = StartupDiagnostics::default();
        let loaded = resolve_flight_sources(&mut startup, &mut diagnostics).unwrap();
        assert_eq!(loaded.weather(), expected);
        assert_eq!(startup.weather.selection, expected);
        assert_eq!(
            recording_conditions(&startup, &startup_clock(&startup)).weather,
            expected
        );
        for arguments in [
            vec!["--weather", "clear"],
            vec!["--weather-seed", "5"],
            vec!["--weather", "legacy"],
        ] {
            let args = [vec!["--replay", path.to_str().unwrap()], arguments].concat();
            let (mut startup, mut diagnostics) =
                parse_arguments_from(args.into_iter().map(str::to_owned));
            let before = recording_conditions(&startup, &startup_clock(&startup));
            assert!(resolve_flight_sources(&mut startup, &mut diagnostics).is_none());
            assert_eq!(
                recording_conditions(&startup, &startup_clock(&startup)),
                before
            );
            assert!(
                diagnostics
                    .0
                    .iter()
                    .any(|line| line.contains("recorded replay weather"))
            );
        }
    }
    let live = live_flight("light-single");
    let current = live.world().resource::<FlightRecorder>().0.recording();
    for version in [1, 2, 3] {
        std::fs::write(
            &path,
            bytes(&fixture(current, &AircraftConfig::light_single(), version)),
        )
        .unwrap();
        for (flag, value) in [
            ("--cloud-cover", "0.5"),
            ("--cloud-base", "900"),
            ("--cloud-top", "2000"),
            ("--cloud-visibility", "300"),
        ] {
            let (mut startup, mut diagnostics) = parse_arguments_from(
                [
                    "--aircraft",
                    "light-single",
                    "--replay",
                    path.to_str().unwrap(),
                    "--legacy-replay-compatibility",
                    flag,
                    value,
                ]
                .map(str::to_owned),
            );
            assert!(startup.clouds_were_given && diagnostics.0.is_empty());
            let before = recording_conditions(&startup, &startup_clock(&startup));
            assert!(resolve_flight_sources(&mut startup, &mut diagnostics).is_none());
            assert_eq!(
                recording_conditions(&startup, &startup_clock(&startup)),
                before
            );
            assert!(
                diagnostics
                    .0
                    .iter()
                    .any(|line| line.contains("manual cloud overrides"))
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn live_manual_clouds_keep_flight_controls_but_disable_recording_and_show_reason() {
    for (flag, value) in [
        ("--cloud-cover", "0.5"),
        ("--cloud-base", "900"),
        ("--cloud-top", "2000"),
        ("--cloud-visibility", "300"),
    ] {
        let (mut startup, mut diagnostics) = parse_arguments_from([flag, value].map(str::to_owned));
        assert!(startup.clouds_were_given && diagnostics.0.is_empty());
        assert!(resolve_flight_sources(&mut startup, &mut diagnostics).is_none());
        assert!(
            diagnostics
                .0
                .iter()
                .any(|line| line == replay_policy::MANUAL_CLOUD_DIAGNOSTIC)
        );
        let mut app = control_app("light-single");
        let initial = *app.world().resource::<FlightSimulation>().0.state();
        app.insert_resource(startup)
            .init_resource::<flightsim_ui::ReplayStatus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<flightsim_ui::LandingReportState>()
            .init_resource::<CameraRig>()
            .add_systems(Update, control_replay.before(advance_simulation))
            .add_systems(
                Update,
                (
                    publish_replay_status,
                    flightsim_ui::replay::update_replay_banner,
                )
                    .chain()
                    .after(advance_simulation),
            );
        let banner = app
            .world_mut()
            .spawn((
                Text::default(),
                Visibility::Hidden,
                flightsim_ui::replay::ReplayBanner,
            ))
            .id();
        tick(
            &mut app,
            Duration::from_millis(100),
            PilotKeys {
                pitch_up: true,
                ..default()
            },
        );
        key(&mut app, KeyCode::F9);
        assert_ne!(
            *app.world().resource::<FlightSimulation>().0.state(),
            initial
        );
        assert!(app.world().resource::<PilotControls>().elevator.value() > 0.0);
        assert!(
            app.world()
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );
        assert!(!app.world().resource::<flightsim_ui::ReplayStatus>().active);
        assert_eq!(
            app.world().get::<Text>(banner).unwrap().as_str(),
            replay_policy::MANUAL_CLOUD_NOTICE
        );
        assert_eq!(
            *app.world().get::<Visibility>(banner).unwrap(),
            Visibility::Visible
        );
    }
    // Render quality is not weather data and must not disable otherwise valid recording.
    let (startup, diagnostics) =
        parse_arguments_from(["--cloud-quality", "high"].map(str::to_owned));
    assert!(!startup.clouds_were_given && diagnostics.0.is_empty());
    let mut app = control_app("light-single");
    app.insert_resource(startup);
    tick(&mut app, Duration::from_millis(100), PilotKeys::default());
    assert!(
        !app.world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .frames()
            .is_empty()
    );
}

#[test]
fn real_replay_notices_fit_narrow_resizes_and_keep_live_tutorial_clear() {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::image::TextureAtlasPlugin,
        bevy::text::TextPlugin,
        bevy::input::InputPlugin,
        bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        },
        bevy::transform::TransformPlugin,
        bevy::ui::UiPlugin,
    ))
    .insert_resource(flightsim_ui::ReplayStatus {
        notice: Some(replay_policy::MANUAL_CLOUD_NOTICE.into()),
        ..default()
    })
    .insert_resource(HudState {
        on_ground: true,
        ..default()
    })
    .insert_resource(flightsim_ui::TutorialVisibility(true))
    .init_resource::<flightsim_ui::TutorialState>()
    .init_resource::<flightsim_ui::Paused>()
    .init_resource::<flightsim_ui::CrashNotice>()
    .add_systems(
        Startup,
        (
            flightsim_ui::replay::spawn_replay_banner,
            flightsim_ui::spawn_tutorial_prompt,
        ),
    )
    .add_systems(
        Update,
        (
            flightsim_ui::replay::update_replay_banner,
            flightsim_ui::update_tutorial_prompt,
        ),
    );
    let size = UVec2::new(1280, 720);
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: size,
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    app.finish();
    app.cleanup();
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let banner = world
        .query_filtered::<Entity, With<flightsim_ui::ReplayBanner>>()
        .single(world)
        .unwrap();
    let panel = world.get::<ChildOf>(banner).unwrap().parent();
    let tutorial = world
        .query_filtered::<Entity, With<flightsim_ui::TutorialPrompt>>()
        .single(world)
        .unwrap();
    for replay_state in [None, Some(0), Some(1), Some(2), Some(3), Some(4), None] {
        *app.world_mut().resource_mut::<flightsim_ui::ReplayStatus>() = replay_state.map_or_else(
            || flightsim_ui::ReplayStatus {
                notice: Some(replay_policy::MANUAL_CLOUD_NOTICE.into()),
                ..default()
            },
            |state| flightsim_ui::ReplayStatus {
                active: true,
                speed: 2.0,
                elapsed: Seconds(65.0),
                total: Seconds(195.0),
                paused: state == 1,
                seeking: state == 2,
                finished: state == 3,
                fault: (state == 4).then(|| "REPLAY STOPPED: test".into()),
                notice: Some(replay_policy::LEGACY_NOTICE.into()),
            },
        );
        // Production replay suppresses live takeoff prompts. Manual weather must
        // coexist with the enabled tutorial; switching back must restore both.
        app.world_mut()
            .resource_mut::<flightsim_ui::TutorialVisibility>()
            .0 = replay_state.is_none();
        for width in [1280, 640, 320, 1280] {
            let size = UVec2::new(width, 720);
            let mut camera = app.world_mut().get_mut::<Camera>(camera).unwrap();
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
            app.update();
            let world = app.world();
            let rect = |entity| {
                let node = world.get::<ComputedNode>(entity).unwrap();
                let transform = world.get::<bevy::ui::UiGlobalTransform>(entity).unwrap();
                Rect::from_center_size(transform.translation, node.size())
            };
            let notice_rect = rect(panel);
            let text_rect = rect(banner);
            assert!(text_rect.min.x >= notice_rect.min.x + 7.0);
            assert!(text_rect.max.x <= notice_rect.max.x - 7.0);
            assert!(text_rect.min.y >= notice_rect.min.y + 3.0);
            assert!(text_rect.max.y <= notice_rect.max.y - 3.0);
            if replay_state.is_none() {
                let tutorial_rect = rect(tutorial);
                assert!(
                    notice_rect.max.y + 4.0 <= tutorial_rect.min.y,
                    "{width}: {notice_rect:?}, {tutorial_rect:?}"
                );
            }
            assert!(notice_rect.min.x >= 0.0 && notice_rect.max.x <= size.as_vec2().x);
            assert!(notice_rect.min.y >= 0.0 && notice_rect.max.y <= size.as_vec2().y);
            let glyphs = world.get::<bevy::text::TextLayoutInfo>(banner).unwrap();
            assert!(!glyphs.glyphs.is_empty());
            assert!(
                glyphs.size.x <= text_rect.width() + 1.0,
                "width={width}, state={replay_state:?}, glyphs={:?}, rect={notice_rect:?}",
                glyphs.size
            );
            assert!(
                glyphs.size.y <= text_rect.height() + 1.0,
                "width={width}, state={replay_state:?}, glyphs={:?}, rect={notice_rect:?}",
                glyphs.size
            );
            let text = world.get::<Text>(banner).unwrap().as_str();
            if replay_state.is_some() {
                assert!(text.contains(replay_policy::LEGACY_NOTICE) && text.contains("F8"));
            } else {
                assert_eq!(text, replay_policy::MANUAL_CLOUD_NOTICE);
            }
            assert_eq!(
                *world.get::<Visibility>(banner).unwrap(),
                Visibility::Visible
            );
            assert_eq!(
                *world.get::<Visibility>(tutorial).unwrap(),
                if replay_state.is_some() {
                    Visibility::Hidden
                } else {
                    Visibility::Visible
                }
            );
        }
    }
    *app.world_mut().resource_mut::<flightsim_ui::ReplayStatus>() = default();
    app.update();
    for entity in [panel, banner] {
        assert_eq!(
            *app.world().get::<Visibility>(entity).unwrap(),
            Visibility::Hidden
        );
    }
    assert_eq!(
        *app.world().get::<Visibility>(tutorial).unwrap(),
        Visibility::Visible
    );
}

#[test]
fn authored_weather_recording_replays_pauses_seeks_and_restarts_with_executed_time() {
    use flightsim_render::RenderWeather;
    use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
    for preset in [
        WeatherPreset::Clear,
        WeatherPreset::Cloud,
        WeatherPreset::Fog,
        WeatherPreset::Rain,
        WeatherPreset::Snow,
        WeatherPreset::Storm,
    ] {
        let mut live = control_app("light-single");
        let initial = *live.world().resource::<FlightSimulation>().0.state();
        let selection = WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                preset,
                Geodetic::from_degrees(35.55, 139.78, -100.0),
                9876,
            )
            .unwrap(),
        );
        let mut startup = Startup::default();
        startup.weather.selection = selection;
        let mut conditions = live
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .conditions()
            .clone();
        conditions.weather = selection;
        conditions.environment.start_epoch = flightsim_render::JulianDate::J2000.get();
        live.insert_resource(startup)
            .insert_resource(FlightRecorder(flightsim_sim::CurrentRecorder::new(
                conditions,
            )))
            .init_resource::<RenderWeather>()
            .add_systems(
                Update,
                weather_runtime::publish_weather.after(advance_simulation),
            );
        for _ in 0..450 {
            tick(
                &mut live,
                Duration::from_secs_f64(1.0 / 30.0),
                PilotKeys::default(),
            );
        }
        let expected_state = *live.world().resource::<FlightSimulation>().0.state();
        let expected_elapsed = live.world().resource::<FlightSimulation>().0.elapsed();
        let current = live
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .clone();
        let original = bytes(&ReplayFile::V3(current.clone()));
        let decoded = ReplayFile::read_from(&mut original.as_slice()).unwrap();
        assert_eq!(decoded.weather(), selection);
        let (mut app, _) = replay_app("light-single", decoded);
        app.insert_resource(Startup::default())
            .init_resource::<RenderWeather>()
            .add_systems(
                Update,
                weather_runtime::publish_weather.after(advance_simulation),
            );
        key(&mut app, KeyCode::F5);
        tick(&mut app, Duration::from_secs(30), PilotKeys::default());
        assert_eq!(
            *app.world().resource::<RenderWeather>(),
            RenderWeather {
                selection,
                elapsed: Seconds::ZERO
            }
        );
        key(&mut app, KeyCode::F5);
        for pass in 0..2 {
            for _ in 0..500 {
                if app
                    .world()
                    .resource::<ReplayPlayback>()
                    .player
                    .is_finished()
                {
                    break;
                }
                tick(
                    &mut app,
                    Duration::from_millis(37),
                    PilotKeys {
                        throttle_down: true,
                        ..default()
                    },
                );
                assert_eq!(
                    *app.world().resource::<RenderWeather>(),
                    RenderWeather {
                        selection,
                        elapsed: app.world().resource::<FlightSimulation>().0.elapsed()
                    }
                );
            }
            assert!(app.world().resource::<ReplayPlayback>().fault.is_none());
            assert_eq!(
                *app.world().resource::<FlightSimulation>().0.state(),
                expected_state
            );
            assert_eq!(
                app.world().resource::<RenderWeather>().elapsed,
                expected_elapsed
            );
            assert_eq!(
                bytes(app.world().resource::<ReplayPlayback>().player.recording()),
                original
            );
            if pass == 0 {
                key(&mut app, KeyCode::F8);
                assert_eq!(
                    app.world().resource::<ReplayPlayback>().player.cursor(),
                    240
                );
                assert_eq!(
                    app.world().resource::<RenderWeather>().elapsed,
                    app.world().resource::<FlightSimulation>().0.elapsed()
                );
                key(&mut app, KeyCode::F5);
                while app.world().resource::<ReplayPlayback>().is_seeking() {
                    tick(&mut app, Duration::from_secs(90), PilotKeys::default());
                }
                let prefix_time = app.world().resource::<RenderWeather>().elapsed;
                tick(&mut app, Duration::from_secs(90), PilotKeys::default());
                assert_eq!(app.world().resource::<RenderWeather>().elapsed, prefix_time);
                key(&mut app, KeyCode::F5);
            }
        }
        // Live restart preserves the exact initial block and resets only time/state.
        let mut simulation = live
            .world_mut()
            .remove_resource::<FlightSimulation>()
            .unwrap();
        let mut controls = live.world_mut().remove_resource::<PilotControls>().unwrap();
        let mut recorder = live
            .world_mut()
            .remove_resource::<FlightRecorder>()
            .unwrap();
        restart_flight(
            &StartCondition::InFlight(initial),
            &mut simulation,
            &mut controls,
            &mut recorder,
            &mut flightsim_ui::TutorialState::default(),
            &mut flightsim_ui::LandingReportState::default(),
        );
        assert_eq!(recorder.0.recording().conditions().weather, selection);
        live.insert_resource(simulation)
            .insert_resource(controls)
            .insert_resource(recorder);
        tick(&mut live, Duration::ZERO, PilotKeys::default());
        assert_eq!(
            *live.world().resource::<RenderWeather>(),
            RenderWeather {
                selection,
                elapsed: Seconds::ZERO
            }
        );
    }
}
