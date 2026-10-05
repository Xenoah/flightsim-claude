//! Actual app recorders and players retain authored Custom weather independently
//! of physical inputs/state. Numerical fixtures are transport witnesses only.
#![allow(clippy::float_cmp)]

use super::*;
use bevy::ecs::system::RunSystemOnce;
use controls_runtime_tests::{control_app, tick};
use flightsim_fdm::RigidBodyState;
use flightsim_input::PilotKeys;
use flightsim_render::RenderWeather;
use flightsim_sim::{
    ReplayFile,
    weather::{
        CloudMorphology, ModeledCloudLayer, ModeledFogLayer, PrecipitationKind,
        WaterEquivalentRate, WeatherPreset, WeatherScenario, WeatherSelection,
    },
};
use std::time::Duration;

fn custom_weather() -> [WeatherSelection; 2] {
    let mut parameters = WeatherScenario::from_preset(
        WeatherPreset::Storm,
        Geodetic::new(
            Radians(-0.0),
            Radians(1.234_567_890_123_456_7),
            Meters(-431.25),
        ),
        u64::MAX - 17,
    )
    .unwrap()
    .parameters();
    parameters.preset = WeatherPreset::Custom;
    parameters.ambient_visibility = Meters(8_765.432_109_876_543);
    parameters.cloud = Some(ModeledCloudLayer {
        morphology: CloudMorphology::Puffy,
        base: Meters(-0.0),
        top: Meters(2_123.456_789_012_345),
        coverage: 0.731_234_567_890_123_4,
        visibility: Meters(321.25),
    });
    parameters.fog = Some(ModeledFogLayer {
        bottom: Meters(-130.5),
        top: Meters(-0.0),
        visibility: Meters(1_357.75),
    });
    parameters.precipitation_kind = PrecipitationKind::Snow;
    parameters.precipitation_rate = WaterEquivalentRate(0.000_012_345_678_901_234);
    let snowy = WeatherSelection::Modeled(WeatherScenario::try_from(parameters).unwrap());
    parameters.seed = u64::MAX;
    parameters.precipitation_kind = PrecipitationKind::None;
    parameters.precipitation_rate = WaterEquivalentRate(-0.0);
    parameters.fog = None;
    let dry = WeatherSelection::Modeled(WeatherScenario::try_from(parameters).unwrap());
    [snowy, dry]
}

// PartialEq alone would silently accept a signed-zero change. Compare every
// scalar bit as well as all tags, the exact u64 seed and optional-layer shape.
fn assert_weather_bits(actual: WeatherSelection, expected: WeatherSelection) {
    assert_eq!(actual, expected);
    let (WeatherSelection::Modeled(actual), WeatherSelection::Modeled(expected)) =
        (actual, expected)
    else {
        panic!("Custom weather must stay modeled");
    };
    let actual = actual.parameters();
    let expected = expected.parameters();
    assert_eq!(actual.preset, WeatherPreset::Custom);
    assert_eq!(actual.seed, expected.seed);
    let scalars = |p: flightsim_sim::weather::WeatherParameters| {
        let mut values = vec![
            p.departure_reference.latitude.get(),
            p.departure_reference.longitude.get(),
            p.departure_reference.altitude.get(),
            p.ambient_visibility.get(),
            p.precipitation_rate.0,
        ];
        if let Some(cloud) = p.cloud {
            values.extend([
                cloud.base.get(),
                cloud.top.get(),
                cloud.coverage,
                cloud.visibility.get(),
            ]);
        }
        if let Some(fog) = p.fog {
            values.extend([fog.bottom.get(), fog.top.get(), fog.visibility.get()]);
        }
        values.into_iter().map(f64::to_bits).collect::<Vec<_>>()
    };
    assert_eq!(scalars(actual), scalars(expected));
}

fn assert_published(app: &App, selection: WeatherSelection) {
    let published = app.world().resource::<RenderWeather>();
    assert_weather_bits(published.selection, selection);
    assert_eq!(
        published.elapsed.get().to_bits(),
        app.world()
            .resource::<FlightSimulation>()
            .0
            .elapsed()
            .get()
            .to_bits()
    );
}

fn legacy_bytes(recording: &ReplayFile) -> Vec<u8> {
    let mut bytes = Vec::new();
    recording.write_to(&mut bytes).unwrap();
    bytes
}

fn legacy_custom_app(selection: WeatherSelection) -> App {
    let mut app = control_app("light-single");
    let mut startup = Startup::default();
    startup.weather.selection = selection;
    let mut conditions = app
        .world()
        .resource::<FlightRecorder>()
        .0
        .recording()
        .conditions()
        .clone();
    conditions.weather = selection;
    app.insert_resource(startup)
        .insert_resource(FlightRecorder(flightsim_sim::CurrentRecorder::new(
            conditions,
        )))
        .init_resource::<RenderWeather>()
        .add_systems(
            Update,
            weather_runtime::publish_weather.after(advance_simulation),
        );
    app
}

#[test]
fn v3_custom_weather_preserves_bytes_physics_pause_rewind_seek_and_live_restart() {
    for selection in custom_weather() {
        let mut live = legacy_custom_app(selection);
        let mut baseline = control_app("light-single");
        let initial = *live.world().resource::<FlightSimulation>().0.state();
        let mut previous = Duration::ZERO;
        for frame in 0..450 {
            let now = Duration::from_secs_f64(f64::from(frame + 1) / 30.0);
            let input = PilotKeys {
                throttle_up: frame < 30,
                trim_down: (30..40).contains(&frame),
                ..default()
            };
            for app in [&mut live, &mut baseline] {
                tick(app, now - previous, input);
            }
            previous = now;
            assert_published(&live, selection);
        }
        let expected = *live.world().resource::<FlightSimulation>().0.state();
        let elapsed = live.world().resource::<FlightSimulation>().0.elapsed();
        assert!(flightsim_sim::replay_v4::state_bits_equal(
            &expected,
            baseline.world().resource::<FlightSimulation>().0.state()
        ));
        let record = live
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .clone();
        let baseline_record = baseline.world().resource::<FlightRecorder>().0.recording();
        assert_eq!(record.frames(), baseline_record.frames());
        assert_eq!(record.keyframes(), baseline_record.keyframes());
        assert_eq!(record.frames().len(), 1800);
        assert!(record.keyframes().len() > 2);
        let original = legacy_bytes(&ReplayFile::V3(record.clone()));
        let decoded = ReplayFile::read_from(&mut original.as_slice()).unwrap();
        assert_eq!(decoded.format_version(), 3);
        assert_weather_bits(decoded.weather(), selection);
        assert_eq!(legacy_bytes(&decoded), original);
        assert_eq!(decoded.frames(), record.frames());
        assert_eq!(decoded.keyframes(), record.keyframes());

        let mut playback = legacy_custom_app(WeatherSelection::Legacy);
        playback.insert_resource(ReplayPlayback::new(decoded));
        playback
            .world_mut()
            .resource_mut::<ReplayPlayback>()
            .player
            .set_paused(true);
        tick(&mut playback, Duration::from_secs(50), PilotKeys::default());
        assert_published(&playback, selection);
        assert_eq!(
            playback.world().resource::<RenderWeather>().elapsed,
            Seconds::ZERO
        );
        assert!(flightsim_sim::replay_v4::state_bits_equal(
            playback.world().resource::<FlightSimulation>().0.state(),
            &initial
        ));
        playback
            .world_mut()
            .resource_mut::<ReplayPlayback>()
            .player
            .set_paused(false);
        for pass in 0..2 {
            for _ in 0..500 {
                if playback
                    .world()
                    .resource::<ReplayPlayback>()
                    .player
                    .is_finished()
                {
                    break;
                }
                tick(
                    &mut playback,
                    Duration::from_millis(37),
                    PilotKeys {
                        pitch_down: true,
                        throttle_down: true,
                        ..default()
                    },
                );
                assert_published(&playback, selection);
            }
            let replay = playback.world().resource::<ReplayPlayback>();
            assert!(replay.player.is_finished());
            assert!(replay.fault.is_none(), "{:?}", replay.fault);
            assert_eq!(legacy_bytes(replay.player.recording()), original);
            assert_eq!(
                replay.last_controls,
                record.frames().last().unwrap().controls
            );
            assert!(flightsim_sim::replay_v4::state_bits_equal(
                playback.world().resource::<FlightSimulation>().0.state(),
                &expected
            ));
            assert_eq!(
                playback.world().resource::<RenderWeather>().elapsed,
                elapsed
            );
            if pass == 0 {
                playback
                    .world_mut()
                    .resource_scope(|world, mut replay: Mut<ReplayPlayback>| {
                        replay.player.set_paused(true);
                        replay.rewind(world.resource_mut::<FlightSimulation>().0.legacy_mut());
                    });
                tick(&mut playback, Duration::ZERO, PilotKeys::default());
                assert_eq!(
                    playback
                        .world()
                        .resource::<ReplayPlayback>()
                        .player
                        .cursor(),
                    240
                );
                assert!(playback.world().resource::<ReplayPlayback>().is_seeking());
                assert_published(&playback, selection);
                for _ in 0..10 {
                    if !playback.world().resource::<ReplayPlayback>().is_seeking() {
                        break;
                    }
                    tick(&mut playback, Duration::ZERO, PilotKeys::default());
                    assert_published(&playback, selection);
                }
                assert!(!playback.world().resource::<ReplayPlayback>().is_seeking());
                let paused = *playback.world().resource::<FlightSimulation>().0.state();
                let paused_time = playback.world().resource::<RenderWeather>().elapsed;
                tick(&mut playback, Duration::from_secs(50), PilotKeys::default());
                assert_eq!(
                    playback.world().resource::<RenderWeather>().elapsed,
                    paused_time
                );
                assert!(flightsim_sim::replay_v4::state_bits_equal(
                    playback.world().resource::<FlightSimulation>().0.state(),
                    &paused
                ));
                playback
                    .world_mut()
                    .resource_mut::<ReplayPlayback>()
                    .player
                    .set_paused(false);
            }
        }
        assert!(
            playback
                .world()
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );

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
        assert_weather_bits(recorder.0.recording().conditions().weather, selection);
        assert_eq!(recorder.0.recording().conditions(), record.conditions());
        assert!(recorder.0.recording().frames().is_empty());
        live.insert_resource(simulation)
            .insert_resource(controls)
            .insert_resource(recorder);
        tick(&mut live, Duration::ZERO, PilotKeys::default());
        assert_published(&live, selection);
        assert_eq!(
            live.world().resource::<RenderWeather>().elapsed,
            Seconds::ZERO
        );
        assert!(flightsim_sim::replay_v4::state_bits_equal(
            live.world().resource::<FlightSimulation>().0.state(),
            &initial
        ));
    }
}

fn jet_startup() -> Startup {
    let mut startup = Startup {
        aircraft: aircraft_profile::SelectedAircraftProfile::Jet(
            flightsim_sim::aircraft_profile::AircraftProfileV2::parse(include_str!(
                "../../../assets/aircraft/kestrel_jet_trainer.json"
            ))
            .unwrap(),
        ),
        wind: flightsim_sim::Wind::CALM,
        turbulence: flightsim_fdm::Turbulence::CALM,
        ..default()
    };
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    startup
}

fn jet_start() -> StartCondition {
    StartCondition::InFlight(RigidBodyState::from_geodetic(
        Geodetic::from_degrees(35.55, 139.78, 1000.0),
        Attitude::from_degrees(0.0, 2.0, 0.0),
        Ned::new(50.0, 0.0, 0.0),
    ))
}

fn bounded_app(startup: Startup, session: FlightSession) -> App {
    let mut controls = world_runtime::initial_controls(&startup);
    controls.throttle.set_absolute(0.4);
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(controls)
        .insert_resource(startup)
        .insert_resource(FlightSimulation(session))
        .init_resource::<SampledPilotInput>()
        .init_resource::<flightsim_ui::Paused>()
        .init_resource::<world_runtime::MapCapture>()
        .init_resource::<RenderWeather>()
        .add_systems(
            Update,
            advance_simulation.run_if(world_runtime::flight_controls_active),
        )
        .add_systems(
            Update,
            weather_runtime::publish_weather.after(advance_simulation),
        );
    configure_live_input_scheduling(&mut app);
    app
}

// Each expansion invokes the family's actual app recorder, codec and player.
// The shared flow deliberately exceeds both periodic checkpoint and seek limits.
macro_rules! bounded_custom_weather_test {
    ($name:ident, $startup:path, $start:path, $live:ident, $replay:ident,
     $constructor:ident, $simulation:ident, $recording:path, $player:path,
     $state_equal:path, $version:literal) => {
        #[test]
        fn $name() {
            for selection in custom_weather() {
                let mut startup = $startup();
                startup.weather.selection = selection;
                let clock = flightsim_render::TimeOfDay::default();
                let session = FlightSession::prepare_bounded(&startup, &clock, $start()).unwrap();
                let initial = session.$simulation().unwrap().snapshot();
                let config = session.$simulation().unwrap().config().clone();
                let mut baseline_startup = startup.clone();
                baseline_startup.weather.selection = WeatherSelection::Legacy;
                let baseline =
                    FlightSession::prepare_bounded(&baseline_startup, &clock, $start()).unwrap();
                let mut baseline = bounded_app(baseline_startup, baseline);
                let mut live = bounded_app(startup.clone(), session);
                for frame in 0..361 {
                    let input = PilotKeys {
                        throttle_up: frame < 30,
                        brakes: frame >= 240,
                        ..default()
                    };
                    for app in [&mut live, &mut baseline] {
                        tick(app, Duration::from_nanos(8_333_334), input);
                    }
                    assert_published(&live, selection);
                }
                let expected = live
                    .world()
                    .resource::<FlightSimulation>()
                    .0
                    .$simulation()
                    .unwrap()
                    .snapshot();
                assert_eq!(
                    expected,
                    baseline
                        .world()
                        .resource::<FlightSimulation>()
                        .0
                        .$simulation()
                        .unwrap()
                        .snapshot()
                );
                let FlightSession::$live {
                    recorder,
                    recording_error,
                    ..
                } = &live.world().resource::<FlightSimulation>().0
                else {
                    panic!("live family");
                };
                assert!(recording_error.is_none(), "{recording_error:?}");
                let record = recorder.export();
                assert!(record.terminal().is_none(), "{:?}", record.terminal());
                assert_eq!(record.controls().len(), 361);
                assert_ne!(record.controls().first(), record.controls().last());
                assert_eq!(
                    record
                        .checkpoints()
                        .iter()
                        .map(|checkpoint| checkpoint.frame)
                        .collect::<Vec<_>>(),
                    vec![120, 240, 360, 361]
                );
                let FlightSession::$live {
                    recorder: baseline_recorder,
                    ..
                } = &baseline.world().resource::<FlightSimulation>().0
                else {
                    panic!("baseline family");
                };
                let baseline_record = baseline_recorder.export();
                assert_eq!(record.controls(), baseline_record.controls());
                assert_eq!(record.checkpoints(), baseline_record.checkpoints());
                assert!($state_equal(record.final_state(), &expected.state));
                let mut original = Vec::new();
                record.write_to(&mut original).unwrap();
                assert_eq!(
                    u16::from_le_bytes(original[8..10].try_into().unwrap()),
                    $version
                );
                let decoded = <$recording>::read_from(&mut original.as_slice()).unwrap();
                assert_weather_bits(decoded.conditions().environment.weather, selection);
                assert_eq!(decoded.controls(), record.controls());
                assert_eq!(decoded.checkpoints(), record.checkpoints());
                let mut roundtrip = Vec::new();
                decoded.write_to(&mut roundtrip).unwrap();
                assert_eq!(roundtrip, original);
                let player = <$player>::new(config, decoded).unwrap();
                // Contradictory launch weather must never override recorded Custom.
                let playback_session = FlightSession::$constructor(player);
                let mut launch = startup.clone();
                launch.weather.selection = WeatherSelection::Legacy;
                let mut playback = bounded_app(launch, playback_session);
                {
                    let mut session = playback.world_mut().resource_mut::<FlightSimulation>();
                    let FlightSession::$replay { player, .. } = &mut session.0 else {
                        unreachable!()
                    };
                    player.set_paused(true);
                }
                tick(&mut playback, Duration::from_secs(30), PilotKeys::default());
                assert_published(&playback, selection);
                assert_eq!(
                    playback
                        .world()
                        .resource::<FlightSimulation>()
                        .0
                        .$simulation()
                        .unwrap()
                        .snapshot(),
                    initial
                );
                {
                    let mut session = playback.world_mut().resource_mut::<FlightSimulation>();
                    let FlightSession::$replay { player, .. } = &mut session.0 else {
                        unreachable!()
                    };
                    player.set_paused(false);
                }
                for pass in 0..2 {
                    for _ in 0..20 {
                        tick(
                            &mut playback,
                            Duration::from_millis(250),
                            PilotKeys {
                                throttle_down: true,
                                pitch_down: true,
                                ..default()
                            },
                        );
                        assert_published(&playback, selection);
                    }
                    let session = &playback.world().resource::<FlightSimulation>().0;
                    let FlightSession::$replay { player, fault, .. } = session else {
                        unreachable!()
                    };
                    assert!(fault.is_none(), "{fault:?}");
                    assert!(player.finished());
                    assert_eq!(player.cursor(), 361);
                    assert_eq!(player.simulation().snapshot(), expected);
                    assert!($state_equal(player.simulation().state(), &expected.state));
                    assert_eq!(player.last_controls(), *record.controls().last().unwrap());
                    let mut retained = Vec::new();
                    player.recording().write_to(&mut retained).unwrap();
                    assert_eq!(retained, original);
                    if pass == 0 {
                        {
                            let mut session =
                                playback.world_mut().resource_mut::<FlightSimulation>();
                            let FlightSession::$replay { player, .. } = &mut session.0 else {
                                unreachable!()
                            };
                            player.set_paused(true);
                            assert_eq!(player.seek_to(300).unwrap(), 240);
                            assert!(player.seeking());
                            assert_eq!(player.cursor(), 240);
                        }
                        playback
                            .world_mut()
                            .run_system_once(weather_runtime::publish_weather)
                            .unwrap();
                        assert_published(&playback, selection);
                        // App dispatch completes pending seeks even while paused.
                        tick(&mut playback, Duration::ZERO, PilotKeys::default());
                        assert_published(&playback, selection);
                        {
                            let session = playback.world().resource::<FlightSimulation>();
                            let FlightSession::$replay { player, .. } = &session.0 else {
                                unreachable!()
                            };
                            assert!(!player.seeking());
                            assert_eq!(player.cursor(), 300);
                            assert!(player.paused());
                        }
                        let paused = playback
                            .world()
                            .resource::<FlightSimulation>()
                            .0
                            .$simulation()
                            .unwrap()
                            .snapshot();
                        tick(&mut playback, Duration::from_secs(50), PilotKeys::default());
                        assert_published(&playback, selection);
                        assert_eq!(
                            playback
                                .world()
                                .resource::<FlightSimulation>()
                                .0
                                .$simulation()
                                .unwrap()
                                .snapshot(),
                            paused
                        );
                        {
                            let mut session =
                                playback.world_mut().resource_mut::<FlightSimulation>();
                            let FlightSession::$replay { player, .. } = &mut session.0 else {
                                unreachable!()
                            };
                            // A backward seek reconstructs the complete checkpoint.
                            assert_eq!(player.seek_to(120).unwrap(), 120);
                            assert!($state_equal(
                                player.simulation().state(),
                                &record.checkpoints()[0].state
                            ));
                            assert_eq!(player.last_controls(), record.controls()[119]);
                            player.restart().unwrap();
                            assert_eq!(player.cursor(), 0);
                            assert_eq!(player.simulation().snapshot(), initial);
                            assert!(player.paused());
                        }
                        tick(&mut playback, Duration::ZERO, PilotKeys::default());
                        assert_published(&playback, selection);
                        assert_eq!(
                            playback.world().resource::<RenderWeather>().elapsed,
                            Seconds::ZERO
                        );
                        let mut session = playback.world_mut().resource_mut::<FlightSimulation>();
                        let FlightSession::$replay { player, .. } = &mut session.0 else {
                            unreachable!()
                        };
                        player.set_paused(false);
                    }
                }
                let restarted = FlightSession::prepare_bounded(&startup, &clock, $start()).unwrap();
                assert_eq!(restarted.$simulation().unwrap().snapshot(), initial);
                let FlightSession::$live { recorder, .. } = &restarted else {
                    unreachable!()
                };
                let fresh = recorder.export();
                assert_weather_bits(fresh.conditions().environment.weather, selection);
                assert_eq!(fresh.conditions(), record.conditions());
                assert!(fresh.controls().is_empty());
                assert!($state_equal(fresh.final_state(), &initial.state));
            }
        }
    };
}

bounded_custom_weather_test!(
    v4_custom_weather_preserves_bytes_physics_pause_seek_rewind_and_restart,
    jet_startup,
    jet_start,
    JetLive,
    JetReplay,
    replay,
    jet,
    flightsim_sim::replay_v4::JetRecording,
    flightsim_sim::replay_v4::JetReplayPlayer,
    flightsim_sim::replay_v4::state_bits_equal,
    4
);

#[cfg(not(feature = "commercial-staging"))]
bounded_custom_weather_test!(
    v5_custom_weather_preserves_bytes_full_state_pause_seek_rewind_and_restart,
    turboprop_lifecycle_tests::startup,
    turboprop_lifecycle_tests::start,
    TurbopropLive,
    TurbopropReplay,
    replay_turboprop,
    turboprop,
    flightsim_sim::replay_v5::TurbopropRecording,
    flightsim_sim::replay_v5::TurbopropReplayPlayer,
    flightsim_sim::replay_v5::state_bits_equal,
    5
);

#[cfg(not(feature = "commercial-staging"))]
bounded_custom_weather_test!(
    v6_custom_weather_preserves_bytes_full_state_pause_seek_rewind_and_restart,
    aircraft_picker_runtime::nearstatic_lifecycle_tests::runtime_startup,
    aircraft_picker_runtime::nearstatic_lifecycle_tests::start,
    NearStaticTurbopropLive,
    NearStaticTurbopropReplay,
    replay_near_static_turboprop,
    near_static_turboprop,
    flightsim_sim::replay_v6::NearStaticTurbopropRecording,
    flightsim_sim::replay_v6::NearStaticTurbopropReplayPlayer,
    flightsim_sim::replay_v6::state_bits_equal,
    6
);
