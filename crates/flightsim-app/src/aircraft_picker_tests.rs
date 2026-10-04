//! Full new-flight transactions use the production GLB loader and scene spawner,
//! without a GPU or audio device. Native visual/handling acceptance is separate.
#![allow(clippy::float_cmp)]
use super::*;
use bevy::app::TaskPoolPlugin;
use bevy::asset::AssetPlugin;
use bevy::camera::visibility::VisibilityPlugin;
use bevy::ecs::system::RunSystemOnce;
use bevy::gltf::GltfPlugin;
use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
use bevy::mesh::MeshPlugin;
use bevy::scene::ScenePlugin;
use bevy::transform::TransformPlugin;
use flightsim_sim::replay::ReplayFile;
#[cfg(not(feature = "commercial-staging"))]
use flightsim_sim::replay::identity::AircraftCompatibility;
#[cfg(not(feature = "commercial-staging"))]
use flightsim_sim::replay_v4::{JetReplayPlayer, ModelReplayFile};
use std::time::{Duration, Instant};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}
fn request(choice: usize, generation: u64) -> WorldMapStart {
    WorldMapStart {
        position: Geodetic::from_degrees(0.0, -140.0, 0.0),
        month: 7,
        aircraft_choice: choice,
        generation,
    }
}
fn startup() -> Startup {
    let (mut startup, notes) = parse_arguments_from(["--aircraft".into(), "swift-sport".into()]);
    assert!(notes.0.is_empty());
    startup.assets = Some(assets());
    startup.turbulence = flightsim_fdm::Turbulence::CALM;
    startup
}
fn app(startup: Startup) -> App {
    let mut app = App::new();
    app.insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
        .add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin {
                file_path: startup.assets.as_ref().unwrap().display().to_string(),
                ..default()
            },
            ScenePlugin,
            MeshPlugin,
            GltfPlugin::default(),
            TransformPlugin,
            VisibilityPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .register_type::<MeshMaterial3d<StandardMaterial>>()
        .insert_resource(Time::<()>::default())
        .init_resource::<WorldMapActions>()
        .init_resource::<WorldMapState>()
        .init_resource::<CameraRig>()
        .init_resource::<TerrainTiles>()
        .init_resource::<weather_runtime::PendingWeather>()
        .init_resource::<flightsim_ui::CrashNotice>()
        .init_resource::<flightsim_ui::HudSmoothing>()
        .init_resource::<HudState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(startup.view)
        .insert_resource(world_runtime::WorldRuntime::new(&startup).unwrap())
        .insert_resource(TerrainStreaming {
            selector: LodSelector::new(
                16.0,
                720.0,
                Degrees(60.0).to_radians(),
                13,
                Meters(20_000.0),
            ),
            source: Box::new(EmptyTileSource) as BoxedSource,
            cache: TileCache::new(1024 * 1024),
            live: default(),
            material: default(),
        });
    let prepared = world_runtime::prepare_world_map_flight(startup, request(0, 0), None).unwrap();
    let mut scene = aircraft_scene::stage(app.world_mut(), &prepared).unwrap();
    app.finish();
    app.cleanup();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.update();
        if scene.poll(app.world_mut()).unwrap() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initial scene did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    scene.commit(app.world_mut(), &prepared.startup, &prepared.simulation);
    flightsim_audio::replace_sound_source(app.world_mut(), prepared.startup.engine_sound);
    world_runtime::commit_world_map_flight(app.world_mut(), prepared);
    initialize(app.world_mut());
    app.add_systems(Update, world_runtime::apply_world_map_start);
    app
}
fn submit(app: &mut App, index: usize) -> WorldMapStart {
    let generation = {
        let mut actions = app.world_mut().resource_mut::<WorldMapActions>();
        actions.invalidate_start();
        actions.generation
    };
    let request = request(index, generation);
    {
        let mut map = app.world_mut().resource_mut::<WorldMapState>();
        map.visible = true;
        map.selected = request.position;
        map.month = request.month;
        map.aircraft_choice = index;
    }
    app.world_mut().resource_mut::<WorldMapActions>().start_at = Some(request);
    request
}
fn finish(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.update();
        if !app.world().resource::<WorldMapState>().visible {
            return;
        }
        assert!(
            !app.world()
                .resource::<AircraftPicker>()
                .pending
                .as_ref()
                .is_some_and(|pending| matches!(pending.phase, Phase::Failed)),
            "new flight failed: {}",
            app.world().resource::<WorldMapState>().navigation_note
        );
        assert!(Instant::now() < deadline, "scene preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn active_root(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Aircraft>>()
        .single(world)
        .unwrap()
}
fn recorder_bytes(app: &App) -> Vec<u8> {
    let mut bytes = Vec::new();
    match &app.world().resource::<FlightSimulation>().0 {
        FlightSession::JetLive { recorder, .. } => recorder.export().write_to(&mut bytes).unwrap(),
        FlightSession::Legacy(_) => app
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .write_to(&mut bytes)
            .unwrap(),
        FlightSession::JetReplay { player, .. } => player.recording().write_to(&mut bytes).unwrap(),
    }
    bytes
}
fn advance_one(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(10));
    app.world_mut().run_system_once(advance_simulation).unwrap();
}
fn assert_target(app: &mut App, index: usize, old_root: Entity) {
    assert_ne!(active_root(app), old_root);
    assert!(app.world().get_entity(old_root).is_err());
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<flightsim_audio::FlightSoundSource>>()
            .iter(world)
            .count(),
        1
    );
    let startup = world.resource::<Startup>();
    let choice = world.resource::<AircraftPicker>().entries[index]
        .choice
        .as_ref()
        .unwrap();
    assert_eq!(startup.aircraft.id(), choice.profile.id());
    assert_eq!(startup.model, choice.model);
    assert_eq!(startup.model_fit, choice.fit);
    assert_eq!(startup.engine_sound, choice.sound);
    assert_eq!(
        world.resource::<CameraRig>().eye_offset,
        choice.profile.camera_eye()
    );
    assert_eq!(
        world.resource::<PilotControls>().to_control_inputs(),
        world_runtime::initial_controls(startup).to_control_inputs()
    );
    assert_eq!(
        world
            .resource::<flightsim_ui::FlightGuidance>()
            .tutorial_enabled,
        !choice.profile.is_jet()
    );
    assert!(!world.resource::<flightsim_ui::CrashNotice>().is_crashed());
    assert!(!world.resource::<StallWarningStatus>().active);
    assert!(world.resource::<JetHudReset>().0);
    assert_eq!(
        world.resource::<FlightSimulation>().0.elapsed(),
        Seconds::ZERO
    );
    assert_eq!(world.resource::<FlightSimulation>().0.log().landings, 0);
    let jet = choice.profile.is_jet();
    assert_eq!(world.contains_resource::<FlightRecorder>(), !jet);
    let interior = world
        .query_filtered::<Entity, With<InteriorModel>>()
        .iter(world)
        .count();
    assert_eq!(interior == 0, jet);
    assert!(!world.resource::<flightsim_ui::Paused>().is_paused());
    if jet {
        assert_eq!(
            world.resource::<FlightSimulation>().0.parking_brake(),
            Some((false, false))
        );
    }
}

#[test]
fn named_catalog_resolves_only_available_validated_assets_and_preserves_distribution_policy() {
    let mut startup = startup();
    let picker = AircraftPicker::new(&startup);
    let expected = if cfg!(feature = "commercial-staging") {
        vec!["Swift Sport"]
    } else {
        vec![
            "Launch: Swift Sport (generic)",
            "Swift Sport",
            "Meadow Trainer",
            "Kestrel Jet Trainer",
        ]
    };
    assert_eq!(
        picker
            .entries
            .iter()
            .map(|e| e.view.label.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(picker.entries.iter().all(|entry| entry.choice.is_some()));
    startup.assets = None;
    let missing = AircraftPicker::new(&startup);
    let authored = usize::from(!cfg!(feature = "commercial-staging"));
    assert!(
        missing.entries[authored..]
            .iter()
            .all(|e| !e.view.available && e.choice.is_none())
    );
    assert_eq!(
        distribution::info()["bundled_aircraft"],
        if cfg!(feature = "commercial-staging") {
            serde_json::json!(["swift-sport"])
        } else {
            serde_json::json!(["light-single", "swift-sport"])
        }
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn all_original_switch_directions_and_repeat_commit_complete_scene_controls_audio_and_replay() {
    let mut app = app(startup());
    for index in [2, 1, 3, 2, 3, 1, 1, 0] {
        let old_root = active_root(&mut app);
        app.world_mut().resource_mut::<StallWarningStatus>().active = true;
        app.world_mut()
            .resource_mut::<flightsim_ui::Paused>()
            .toggle();
        app.world_mut()
            .resource_mut::<flightsim_ui::CrashNotice>()
            .set("SIMULATION STOPPED: old flight");
        submit(&mut app, index);
        app.update();
        assert_eq!(
            active_root(&mut app),
            old_root,
            "stage must not activate before full readiness"
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        finish(&mut app);
        assert_target(&mut app, index, old_root);
        advance_one(&mut app);
        let bytes = recorder_bytes(&app);
        let startup = app.world().resource::<Startup>();
        if startup.aircraft.is_jet() {
            let ModelReplayFile::V4(recording) =
                ModelReplayFile::read_from(&mut bytes.as_slice()).unwrap()
            else {
                panic!("v4 required")
            };
            assert!(!recording.controls().is_empty());
            let mismatched = flightsim_sim::aircraft_profile::AircraftProfileV2::parse(
                &include_str!("../../../assets/aircraft/kestrel_jet_trainer.json")
                    .replace("\"mass_kg\": 1043.0", "\"mass_kg\": 1044.0"),
            )
            .unwrap();
            assert_eq!(
                mismatched
                    .configuration()
                    .airframe()
                    .mass_properties()
                    .mass()
                    .get(),
                1044.0
            );
            assert!(
                JetReplayPlayer::new(mismatched.configuration().clone(), recording.clone())
                    .is_err()
            );
            assert!(
                JetReplayPlayer::new(
                    startup.aircraft.jet().unwrap().configuration().clone(),
                    recording
                )
                .is_ok()
            );
        } else {
            let recording = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
            assert_eq!(recording.format_version(), 3);
            assert_eq!(
                recording
                    .check_compatibility_with(&startup.aircraft.configuration())
                    .unwrap(),
                AircraftCompatibility::CompleteMatch
            );
            let wrong = if startup.aircraft.id() == "swift-sport" {
                AircraftConfig::light_single()
            } else {
                aircraft_profile::AircraftProfile::builtin("swift-sport")
                    .unwrap()
                    .configuration()
            };
            assert_ne!(
                recording.check_compatibility_with(&wrong).unwrap(),
                AircraftCompatibility::CompleteMatch
            );
        }
    }
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn close_weather_destination_month_and_reselection_invalidate_delayed_jet_without_revival() {
    let mut app = app(startup());
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    let state = *app.world().resource::<FlightSimulation>().0.state();
    for change in 0..5 {
        submit(&mut app, 3);
        app.update();
        assert!(app.world().resource::<AircraftPicker>().preparing());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        match change {
            0 => app.world_mut().resource_mut::<WorldMapState>().visible = false,
            1 => {
                app.world_mut()
                    .resource_mut::<weather_runtime::PendingWeather>()
                    .requested = Some(WeatherPreset::Rain)
            }
            2 => {
                app.world_mut().resource_mut::<WorldMapState>().selected =
                    Geodetic::from_degrees(20.0, 10.0, 0.0)
            }
            3 => app.world_mut().resource_mut::<WorldMapState>().month = 1,
            _ => {
                app.world_mut()
                    .resource_mut::<WorldMapState>()
                    .aircraft_choice = 2
            }
        }
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .invalidate_start();
        app.update();
        {
            let mut map = app.world_mut().resource_mut::<WorldMapState>();
            map.visible = true;
            map.selected = request(3, 0).position;
            map.month = 7;
            map.aircraft_choice = 3;
        }
        app.world_mut()
            .resource_mut::<weather_runtime::PendingWeather>()
            .requested = None;
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(*app.world().resource::<FlightSimulation>().0.state(), state);
        assert_eq!(recorder_bytes(&app), before);
        assert!(app.world().resource::<AircraftPicker>().pending.is_none());
    }
    submit(&mut app, 3);
    finish(&mut app);
    assert_target(&mut app, 3, old_root);
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn raw_or_selected_regions_reject_target_jet_before_consuming_request_or_mutating_old_flight() {
    let mut app = app(startup());
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    let old_weather = app.world().resource::<Startup>().weather.selection;
    for raw in [false, true] {
        app.world_mut().resource_mut::<Startup>().tiles = raw.then(|| PathBuf::from("raw-terrain"));
        let selected = (!raw).then(|| "fixture.region@1.0.0".to_owned());
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .regions
            .selected = selected.clone();
        app.world_mut()
            .resource_mut::<weather_runtime::PendingWeather>()
            .requested = Some(WeatherPreset::Snow);
        let request = submit(&mut app, 3);
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapActions>().start_at,
            Some(request)
        );
        assert_eq!(
            app.world().resource::<WorldMapState>().regions.selected,
            selected
        );
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(recorder_bytes(&app), before);
        assert_eq!(
            app.world().resource::<Startup>().weather.selection,
            old_weather
        );
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .navigation_note
                .contains("Jet flights")
        );
    }
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn failed_glb_and_environment_keep_old_scene_recorder_and_request_retryable() {
    let mut app = app(startup());
    let old_conditions =
        conditions_runtime::PhysicalConditions::from_startup(app.world().resource::<Startup>());
    let mut changed = old_conditions;
    changed.wind.speed = flightsim_core::MetersPerSecond(13.25);
    changed.turbulence = flightsim_fdm::Turbulence::moderate(u64::MAX - 2);
    app.world_mut()
        .init_resource::<conditions_runtime::PendingConditions>();
    app.world_mut()
        .resource_mut::<conditions_runtime::PendingConditions>()
        .selection = Some(changed);
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    let original = app.world().resource::<AircraftPicker>().entries[3]
        .choice
        .clone()
        .unwrap();
    for model in [
        "aircraft/absent-picker.glb",
        "aircraft/kestrel_jet_trainer.json",
    ] {
        app.world_mut().resource_mut::<AircraftPicker>().entries[3]
            .choice
            .as_mut()
            .unwrap()
            .model = Some(model.into());
        let requested = submit(&mut app, 3);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.update();
            if app
                .world()
                .resource::<AircraftPicker>()
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.phase, Phase::Failed))
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(recorder_bytes(&app), before);
        assert_eq!(
            conditions_runtime::PhysicalConditions::from_startup(app.world().resource::<Startup>()),
            old_conditions
        );
        assert_eq!(
            app.world().resource::<WorldMapActions>().start_at,
            Some(requested)
        );
    }
    app.world_mut().resource_mut::<AircraftPicker>().entries[3].choice = Some(original);
    app.world_mut().resource_mut::<Startup>().time_rate = f64::NAN;
    let requested = submit(&mut app, 3);
    app.update();
    assert_eq!(active_root(&mut app), old_root);
    assert_eq!(recorder_bytes(&app), before);
    assert_eq!(
        conditions_runtime::PhysicalConditions::from_startup(app.world().resource::<Startup>()),
        old_conditions
    );
    assert_eq!(
        app.world().resource::<WorldMapActions>().start_at,
        Some(requested)
    );
    app.world_mut().resource_mut::<Startup>().time_rate = 1.0;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        active_root(&mut app),
        old_root,
        "correction alone does not authorize retry"
    );
    submit(&mut app, 3);
    finish(&mut app);
    assert_target(&mut app, 3, old_root);
    assert_eq!(
        conditions_runtime::PhysicalConditions::from_startup(app.world().resource::<Startup>()),
        changed
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn pending_weather_commits_with_target_and_flat_jet_cannot_switch_to_legacy() {
    let mut app = app(startup());
    app.world_mut()
        .resource_mut::<weather_runtime::PendingWeather>()
        .requested = Some(WeatherPreset::Rain);
    submit(&mut app, 3);
    finish(&mut app);
    let startup = app.world().resource::<Startup>();
    assert_eq!(startup.weather.requested, Some(WeatherPreset::Rain));
    assert_eq!(
        app.world()
            .resource::<FlightSimulation>()
            .0
            .jet_environment()
            .unwrap()
            .weather,
        startup.weather.selection
    );
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    app.world_mut()
        .resource_mut::<Startup>()
        .world
        .global_terrain = false;
    let requested = submit(&mut app, 2);
    app.update();
    assert_eq!(active_root(&mut app), old_root);
    assert_eq!(recorder_bytes(&app), before);
    assert_eq!(
        app.world().resource::<WorldMapActions>().start_at,
        Some(requested)
    );
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .navigation_note
            .contains("global terrain is off")
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn pending_forces_commit_with_each_family_and_visual_only_switch_keeps_exact_values() {
    use crate::conditions_runtime::{PendingConditions, PhysicalConditions};
    let mut app = app(startup());
    let mut desired = app.world().resource::<Startup>().clone();
    desired.wind = flightsim_sim::Wind {
        from: Radians(4.712_388_980_384_123),
        speed: flightsim_core::MetersPerSecond(10.288_065_843_621),
    };
    desired.turbulence = flightsim_fdm::Turbulence {
        intensity: flightsim_core::MetersPerSecond(2.125_123_456_789),
        seed: 0x1234_5678_9abc_def0,
    };
    desired.wind_was_given = true;
    desired.turbulence_was_given = true;
    let expected = PhysicalConditions::from_startup(&desired);
    app.world_mut()
        .insert_resource(PendingConditions::default());
    app.world_mut()
        .resource_mut::<PendingConditions>()
        .selection = Some(expected);
    for target in [3, 2, 1, 0] {
        app.world_mut()
            .resource_mut::<weather_runtime::PendingWeather>()
            .requested = Some(if target % 2 == 0 {
            WeatherPreset::Clear
        } else {
            WeatherPreset::Rain
        });
        let old_root = active_root(&mut app);
        let old_conditions = PhysicalConditions::from_startup(app.world().resource::<Startup>());
        submit(&mut app, target);
        world_runtime::apply_world_map_start(app.world_mut());
        // No active values or hierarchy are published during model preparation.
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(
            PhysicalConditions::from_startup(app.world().resource::<Startup>()),
            old_conditions
        );
        finish(&mut app);
        assert_eq!(
            PhysicalConditions::from_startup(app.world().resource::<Startup>()),
            expected
        );
        let mut calm_startup = app.world().resource::<Startup>().clone();
        calm_startup.wind = flightsim_sim::Wind::CALM;
        let calm = world_runtime::prepare_world_map_flight(calm_startup, request(target, 0), None)
            .unwrap();
        let actual = app.world().resource::<FlightSimulation>().0.state();
        let initial_wind = actual.velocity_ned().0 - calm.simulation.state().velocity_ned().0;
        assert!(
            (initial_wind - desired.wind.to_ned().0).length() < 1e-8,
            "forces must resolve before wind-aware airborne velocity construction"
        );
        for _ in 0..150 {
            advance_one(&mut app);
        }
        let final_state = *app.world().resource::<FlightSimulation>().0.state();
        let final_elapsed = app.world().resource::<FlightSimulation>().0.elapsed();
        let bytes = recorder_bytes(&app);
        if target == 3 {
            let ModelReplayFile::V4(recording) =
                ModelReplayFile::read_from(&mut bytes.as_slice()).unwrap()
            else {
                panic!()
            };
            assert_eq!(
                recording.conditions().environment.conditions.wind,
                desired.wind
            );
            assert_eq!(
                recording.conditions().environment.conditions.turbulence,
                desired.turbulence
            );
            let mut rewritten = Vec::new();
            recording.write_to(&mut rewritten).unwrap();
            assert_eq!(rewritten, bytes);
            let mut player = JetReplayPlayer::new(
                app.world()
                    .resource::<Startup>()
                    .aircraft
                    .jet()
                    .unwrap()
                    .configuration()
                    .clone(),
                recording,
            )
            .unwrap();
            for _ in 0..300 {
                player.advance(Seconds(1.0 / 144.0)).unwrap();
            }
            assert!(player.finished());
            assert!(flightsim_sim::replay_v4::state_bits_equal(
                player.simulation().state(),
                &final_state
            ));
            assert_eq!(player.simulation().elapsed(), final_elapsed);
            player.restart().unwrap();
            assert_eq!(player.simulation().elapsed(), Seconds::ZERO);
            for _ in 0..100 {
                player.advance(Seconds(1.0 / 30.0)).unwrap();
            }
            assert!(flightsim_sim::replay_v4::state_bits_equal(
                player.simulation().state(),
                &final_state
            ));
        } else {
            let recording = ReplayFile::read_from(&mut bytes.as_slice()).unwrap();
            assert_eq!(recording.environment().wind, desired.wind);
            assert_eq!(recording.environment().turbulence, desired.turbulence);
            let mut rewritten = Vec::new();
            recording.write_to(&mut rewritten).unwrap();
            assert_eq!(rewritten, bytes);
            let environment = recording.environment();
            let startup = app.world().resource::<Startup>();
            let mut replay_sim = Simulation::from_state(
                startup.aircraft.configuration(),
                recording.keyframe_exactly_at(0).unwrap().state,
                Terrain::new(
                    make_source(startup),
                    64 * 1024 * 1024,
                    world_runtime::terrain_levels(startup),
                ),
                GroundSampler::default(),
            );
            replay_sim.set_wind(environment.wind);
            replay_sim.set_turbulence(environment.turbulence);
            replay_sim.set_climate(environment.climate_date).unwrap();
            let mut playback = ReplayPlayback::new(recording);
            for _ in 0..300 {
                assert!(!playback.tick(&mut replay_sim, Seconds(1.0 / 144.0)));
            }
            assert!(playback.player.is_finished());
            assert!(flightsim_sim::replay_v4::state_bits_equal(
                replay_sim.state(),
                &final_state
            ));
            assert_eq!(replay_sim.elapsed(), final_elapsed);
            playback.rewind(&mut replay_sim);
            assert_eq!(replay_sim.elapsed(), Seconds::ZERO);
            for _ in 0..100 {
                assert!(!playback.tick(&mut replay_sim, Seconds(1.0 / 30.0)));
            }
            assert!(playback.player.is_finished());
            assert!(flightsim_sim::replay_v4::state_bits_equal(
                replay_sim.state(),
                &final_state
            ));
        }
        // Production restart uses the committed conditions, resetting gust time.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.world_mut().run_system_once(control_flight).unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert_eq!(
            app.world().resource::<FlightSimulation>().0.elapsed(),
            Seconds::ZERO
        );
        assert_eq!(
            PhysicalConditions::from_startup(app.world().resource::<Startup>()),
            expected
        );
    }
}

#[test]
fn changed_conditions_or_edit_restore_cannot_commit_an_admitted_scene() {
    use crate::conditions_runtime::{PendingConditions, PhysicalConditions};
    let mut app = app(startup());
    let original = PhysicalConditions::from_startup(app.world().resource::<Startup>());
    let mut changed = original;
    changed.wind.speed = flightsim_core::MetersPerSecond(11.0);
    app.world_mut()
        .insert_resource(PendingConditions::default());
    app.world_mut()
        .resource_mut::<PendingConditions>()
        .selection = Some(original);
    let old_root = active_root(&mut app);
    let old_bytes = recorder_bytes(&app);
    for restore in [false, true] {
        submit(&mut app, 0);
        world_runtime::apply_world_map_start(app.world_mut());
        assert!(app.world().resource::<AircraftPicker>().preparing());
        app.world_mut()
            .resource_mut::<PendingConditions>()
            .selection = Some(changed);
        if restore {
            // Editor mutation invalidates the generation immediately, before
            // Apply. Even restoring every value cannot resurrect this Start.
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .invalidate_start();
            app.world_mut()
                .resource_mut::<PendingConditions>()
                .selection = Some(original);
        }
        for _ in 0..5 {
            app.update();
        }
        assert!(!app.world().resource::<AircraftPicker>().preparing());
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(recorder_bytes(&app), old_bytes);
        assert_eq!(
            PhysicalConditions::from_startup(app.world().resource::<Startup>()),
            original
        );
        app.world_mut()
            .resource_mut::<PendingConditions>()
            .selection = Some(original);
    }
    submit(&mut app, 0);
    finish(&mut app);
    assert_eq!(
        PhysicalConditions::from_startup(app.world().resource::<Startup>()),
        original
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn replay_preview_cannot_stage_or_rewrite_recorded_aircraft() {
    let mut app = app(startup());
    submit(&mut app, 3);
    finish(&mut app);
    advance_one(&mut app);
    let (config, recording) = {
        let FlightSession::JetLive {
            simulation,
            recorder,
            ..
        } = &app.world().resource::<FlightSimulation>().0
        else {
            panic!()
        };
        (simulation.config().clone(), recorder.export())
    };
    app.world_mut()
        .insert_resource(FlightSimulation(FlightSession::replay(
            JetReplayPlayer::new(config, recording).unwrap(),
        )));
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    submit(&mut app, 2);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(active_root(&mut app), old_root);
    assert_eq!(recorder_bytes(&app), before);
    assert!(
        !app.world()
            .resource::<WorldMapState>()
            .aircraft_selection_enabled
    );
    let map = app.world().resource::<WorldMapState>();
    assert_eq!(
        map.active_aircraft,
        app.world().resource::<Startup>().aircraft.name()
    );
    let label = flightsim_ui::world_map::format_world_map_text(
        flightsim_ui::world_map::WorldMapText::AircraftChoice,
        map,
        &flightsim_ui::WorldMapRaster::default(),
    );
    assert!(label.starts_with("AIRCRAFT LOCKED:"));
    assert!(label.contains(&map.active_aircraft));
    assert!(app.world().resource::<AircraftPicker>().pending.is_none());
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn cancelled_before_spawn_and_after_ready_shared_scene_never_reappears() {
    let mut app = app(startup());
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    let meshes = app.world().resource::<Assets<Mesh>>().len();
    for ready in [false, true] {
        submit(&mut app, 1);
        world_runtime::apply_world_map_start(app.world_mut());
        let root = match &app
            .world()
            .resource::<AircraftPicker>()
            .pending
            .as_ref()
            .unwrap()
            .phase
        {
            Phase::Model { scene, .. } => scene.root,
            _ => panic!("staged scene required"),
        };
        let model = app.world().get::<Children>(root).unwrap()[0];
        assert!(
            app.world()
                .get::<bevy::scene::SceneInstance>(model)
                .is_none(),
            "candidate must not be instantiated before the scene schedule runs"
        );
        if ready {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                app.update();
                assert_eq!(active_root(&mut app), old_root);
                if app
                    .world()
                    .get::<bevy::scene::SceneInstance>(model)
                    .is_some_and(|instance| {
                        app.world()
                            .resource::<bevy::scene::SceneSpawner>()
                            .instance_is_ready(**instance)
                    })
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        app.world_mut().resource_mut::<WorldMapState>().visible = false;
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .invalidate_start();
        world_runtime::apply_world_map_start(app.world_mut());
        for _ in 0..5 {
            app.update();
        }
        assert!(app.world().get_entity(root).is_err());
        assert_eq!(active_root(&mut app), old_root);
        assert_eq!(recorder_bytes(&app), before);
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            meshes,
            "generated candidate meshes must be released; active shared GLB retained"
        );
    }
    submit(&mut app, 1);
    world_runtime::apply_world_map_start(app.world_mut());
    let first = match &app
        .world()
        .resource::<AircraftPicker>()
        .pending
        .as_ref()
        .unwrap()
        .phase
    {
        Phase::Model { scene, .. } => scene.root,
        _ => panic!(),
    };
    submit(&mut app, 1);
    world_runtime::apply_world_map_start(app.world_mut());
    assert!(app.world().get_entity(first).is_err());
    assert_eq!(active_root(&mut app), old_root);
    finish(&mut app);
    assert_target(&mut app, 1, old_root);
}

struct TemporaryAssets(PathBuf);
impl TemporaryAssets {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("flightsim-picker-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(root.join("aircraft")).unwrap();
        Self(root)
    }
    fn copy_originals(&self) {
        for name in ["swift_sport", "meadow_trainer", "kestrel_jet_trainer"] {
            for extension in ["json", "glb"] {
                let file = format!("aircraft/{name}.{extension}");
                std::fs::copy(assets().join(&file), self.0.join(file)).unwrap();
            }
        }
    }
}
impl Drop for TemporaryAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn malformed_missing_scene_empty_scene_and_scene_cameras_fail_without_visible_owners() {
    let temp = TemporaryAssets::new();
    temp.copy_originals();
    let mut startup = startup();
    startup.assets = Some(temp.0.clone());
    let mut app = app(startup);
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    for (file,bytes) in [
        ("corrupt.glb", b"this is not a GLB".as_slice()),
        ("no_scene.gltf", br#"{"asset":{"version":"2.0"},"scenes":[]}"#.as_slice()),
        ("empty.gltf", br#"{"asset":{"version":"2.0"},"scenes":[{}],"scene":0}"#.as_slice()),
        ("camera_light.gltf", br#"{"asset":{"version":"2.0"},"extensionsUsed":["KHR_lights_punctual"],"extensions":{"KHR_lights_punctual":{"lights":[{"type":"point"}]}},"cameras":[{"type":"perspective","perspective":{"yfov":0.7,"znear":0.1}}],"nodes":[{"camera":0},{"extensions":{"KHR_lights_punctual":{"light":0}}}],"scenes":[{"nodes":[0,1]}],"scene":0}"#.as_slice()),
    ] {
        std::fs::write(temp.0.join("aircraft").join(file),bytes).unwrap();
        app.world_mut().resource_mut::<AircraftPicker>().entries[3].choice.as_mut().unwrap().model=Some(format!("aircraft/{file}"));
        let requested=submit(&mut app,3);
        let deadline=Instant::now()+Duration::from_secs(10);
        loop {
            app.update();
            let world=app.world_mut();
            assert_eq!(world.query_filtered::<Entity,Or<(With<Camera>,With<PointLight>,With<DirectionalLight>,With<SpotLight>)>>().iter(world).count(),0,"staging must never instantiate cameras or lights");
            if world.resource::<AircraftPicker>().pending.as_ref().is_some_and(|p|matches!(p.phase,Phase::Failed)){break;}
            assert!(Instant::now()<deadline,"{file} did not fail visibly"); std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(active_root(&mut app),old_root); assert_eq!(recorder_bytes(&app),before);
        assert_eq!(app.world().resource::<WorldMapActions>().start_at,Some(requested));
    }
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn resolved_implicit_placeholder_launch_remains_usable_without_bundled_assets() {
    let temp = TemporaryAssets::new();
    // This is the actual Startup retained by setup after its documented implicit
    // developer fallback; named/explicit absent models still reject separately.
    let startup = Startup {
        assets: Some(temp.0.clone()),
        model: None,
        ..default()
    };
    let mut app = app(startup);
    let old_root = active_root(&mut app);
    assert!(
        app.world().resource::<AircraftPicker>().entries[0]
            .view
            .available
    );
    assert!(
        app.world().resource::<AircraftPicker>().entries[1..]
            .iter()
            .all(|e| !e.view.available)
    );
    submit(&mut app, 0);
    finish(&mut app);
    assert_target(&mut app, 0, old_root);
    assert!(app.world().resource::<Startup>().model.is_none());
}

#[test]
#[cfg(feature = "commercial-staging")]
fn commercial_picker_adopts_only_swift_and_keeps_cli_inspection_exception() {
    let temp = TemporaryAssets::new();
    temp.copy_originals();
    let mut startup = startup();
    startup.assets = Some(temp.0.clone());
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::builtin("light-single").unwrap();
    startup.aircraft_choice = Some("light-single".into());
    startup.model = None;
    let mut app = app(startup);
    let old_root = active_root(&mut app);
    let before = recorder_bytes(&app);
    submit(&mut app, 3);
    app.update();
    assert_eq!(active_root(&mut app), old_root);
    assert_eq!(recorder_bytes(&app), before);
    assert_eq!(
        app.world().resource::<Startup>().aircraft.id(),
        "light-single"
    );
    submit(&mut app, 0);
    finish(&mut app);
    assert_target(&mut app, 0, old_root);
    advance_one(&mut app);
    let bytes = recorder_bytes(&app);
    assert_eq!(
        ReplayFile::read_from(&mut bytes.as_slice())
            .unwrap()
            .format_version(),
        3
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn terminal_paused_jet_to_legacy_publishes_clean_notices_and_reseeded_hud_in_commit_update() {
    #[derive(Resource, Default)]
    struct Observed {
        expected: Option<bool>,
        frames: usize,
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "one observer checks all independently published flight resources in the commit update"
    )]
    fn observe(
        map: Res<WorldMapState>,
        simulation: Res<FlightSimulation>,
        paused: Res<flightsim_ui::Paused>,
        crash: Res<flightsim_ui::CrashNotice>,
        replay: Res<flightsim_ui::ReplayStatus>,
        hud: Res<HudState>,
        smoothing: Res<flightsim_ui::HudSmoothing>,
        mut observed: ResMut<Observed>,
    ) {
        if map.visible || observed.expected.is_none() {
            return;
        }
        let jet = observed.expected.unwrap();
        assert_eq!(simulation.0.is_jet(), jet);
        assert!(!paused.is_paused());
        assert!(!crash.is_crashed());
        assert_eq!(replay.notice.is_some(), jet);
        assert_eq!(hud.altitude, simulation.0.state().altitude());
        assert_eq!(hud.vertical_speed, simulation.0.state().vertical_speed());
        assert_eq!(smoothing.displayed().altitude, hud.altitude.to_feet());
        assert_eq!(
            smoothing.displayed().vertical_speed,
            hud.vertical_speed.to_feet_per_minute()
        );
        observed.frames += 1;
    }
    let mut app = app(startup());
    app.init_resource::<SunDirection>()
        .init_resource::<world_runtime::MapCapture>()
        .init_resource::<Observed>();
    configure_flight_presentation(&mut app);
    app.add_systems(
        Update,
        (
            // Preserve the production transitive Start -> advance -> sound ->
            // HUD ordering even though this fixture advances zero physical dt.
            world_runtime::capture_map_input.before(world_runtime::apply_world_map_start),
            advance_simulation
                .after(world_runtime::apply_world_map_start)
                .run_if(world_runtime::flight_controls_active),
            publish_crash.after(world_runtime::apply_world_map_start),
            publish_replay_status.after(world_runtime::apply_world_map_start),
            observe.in_set(flightsim_ui::FlightDisplaySystems),
        ),
    );
    submit(&mut app, 3);
    finish(&mut app);
    if let FlightSession::JetLive { fault, .. } =
        &mut app.world_mut().resource_mut::<FlightSimulation>().0
    {
        *fault = Some("SIMULATION STOPPED: fixture terminal".into());
    }
    app.world_mut()
        .resource_mut::<flightsim_ui::Paused>()
        .toggle();
    app.update();
    assert!(
        app.world()
            .resource::<flightsim_ui::CrashNotice>()
            .is_crashed()
    );
    // Make the pre-switch display deliberately different. Both ordinary map
    // starts can otherwise have the same 1000 m / zero-V/S values, allowing a
    // broken jet-only reset guard to pass an equality check accidentally.
    assert!(!app.world().resource::<JetHudReset>().0);
    let stale = HudState {
        altitude: Meters(5_000.0),
        vertical_speed: flightsim_core::MetersPerSecond(26.4),
        ..*app.world().resource::<HudState>()
    };
    app.world_mut()
        .resource_mut::<flightsim_ui::HudSmoothing>()
        .reset(&stale);
    let stale_display = app
        .world()
        .resource::<flightsim_ui::HudSmoothing>()
        .displayed();
    assert_eq!(stale_display.altitude, stale.altitude.to_feet());
    assert_eq!(
        stale_display.vertical_speed,
        stale.vertical_speed.to_feet_per_minute()
    );
    assert_ne!(
        stale_display.altitude,
        app.world().resource::<HudState>().altitude.to_feet()
    );
    assert_ne!(
        stale_display.vertical_speed,
        app.world()
            .resource::<HudState>()
            .vertical_speed
            .to_feet_per_minute()
    );
    app.world_mut().resource_mut::<Observed>().expected = Some(false);
    submit(&mut app, 2);
    app.update();
    assert!(
        app.world().resource::<WorldMapState>().visible,
        "candidate must still be preparing"
    );
    assert_eq!(
        app.world()
            .resource::<flightsim_ui::HudSmoothing>()
            .displayed(),
        stale_display
    );
    finish(&mut app);
    assert_eq!(app.world().resource::<Observed>().frames, 1);
    app.world_mut().resource_mut::<Observed>().expected = Some(true);
    submit(&mut app, 3);
    finish(&mut app);
    assert_eq!(app.world().resource::<Observed>().frames, 2);
}

#[test]
fn valid_wrong_preset_file_is_unavailable_without_reinterpreting_launch_profile() {
    let temp = TemporaryAssets::new();
    temp.copy_originals();
    std::fs::copy(
        temp.0.join("aircraft/kestrel_jet_trainer.json"),
        temp.0.join("aircraft/swift_sport.json"),
    )
    .unwrap();
    let mut startup = startup();
    startup.assets = Some(temp.0.clone());
    let picker = AircraftPicker::new(&startup);
    let index = usize::from(!cfg!(feature = "commercial-staging"));
    assert!(!picker.entries[index].view.available);
    assert!(picker.entries[index].view.note.contains("does not match"));
    assert!(picker.entries[index].choice.is_none());
    if !cfg!(feature = "commercial-staging") {
        let launch = picker.entries[0].choice.as_ref().unwrap();
        assert_eq!(launch.profile.id(), startup.aircraft.id());
        assert_eq!(launch.model, startup.model);
        assert!(!launch.profile.is_jet());
    }
}
