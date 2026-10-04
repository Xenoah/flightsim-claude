//! GPU-free app transactions for explicit profile 4 / law 2. The numerical
//! profile and a separately selected Swift model do not qualify a Cedar preset.
#![allow(clippy::float_cmp)]
use super::*;
#[cfg(not(feature = "commercial-staging"))]
use flightsim_fdm::{ControlInputs, turboprop::TurbopropState};
use flightsim_sim::aircraft_profile_v4::AircraftProfileV4;
#[cfg(not(feature = "commercial-staging"))]
use flightsim_sim::{
    near_static_turboprop_simulation::{
        NEAR_STATIC_TURBOPROP_FIXED_DT, NearStaticTurbopropEnvironment,
        NearStaticTurbopropSimulation,
    },
    replay_v6::{NearStaticTurbopropRecorder, NearStaticTurbopropRecording, state_bits_equal},
};

pub(crate) const PROFILE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
);

pub(crate) fn startup() -> Startup {
    let profile = AircraftProfileV4::parse(include_str!(
        "../../../docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json"
    ))
    .unwrap();
    let aircraft = aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(profile);
    let mut startup = Startup {
        model_fit: aircraft.model_fit(),
        engine_sound: aircraft.engine_kind(),
        aircraft,
        aircraft_choice: Some(PROFILE_PATH.into()),
        model: Some("aircraft/swift_sport.glb".into()),
        assets: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")),
        wind: flightsim_sim::Wind::CALM,
        turbulence: flightsim_fdm::Turbulence::CALM,
        ..default()
    };
    startup.world.global_terrain = false;
    startup.world.climate_enabled = false;
    startup
}

/// A numerical ballistic profile gives a long, repeatable transport witness.
/// Its retained components are the existing law-1 numerical fixture; the law-2
/// wrapper and negative rows are explicit authored data, never a runtime repair.
#[cfg(not(feature = "commercial-staging"))]
pub(crate) fn runtime_startup() -> Startup {
    let mut startup = startup();
    let mut profile: serde_json::Value = serde_json::from_str(
        &startup
            .aircraft
            .near_static_turboprop()
            .unwrap()
            .to_json()
            .unwrap(),
    )
    .unwrap();
    let mut forward: serde_json::Value = serde_json::from_str(include_str!(
        "../../flightsim-fdm/tests/fixtures/turboprop-numerical.json"
    ))
    .unwrap();
    let mut propeller = profile["dynamics"]["propeller"].clone();
    propeller["forward"] = forward["propeller"].take();
    propeller["negative_rows"] = serde_json::json!([
        [{"ct": 0.01, "cp": 0.03}, {"ct": 0.01, "cp": 0.3}],
        [{"ct": 0.01, "cp": 0.03}, {"ct": 0.01, "cp": 0.3}]
    ]);
    forward["propeller"] = propeller;
    forward["kind"] = "running_turboprop_table".into();
    forward["revision"] = 2.into();
    forward["running_start"] = serde_json::json!({
        "turbine_fraction": 0.4, "shaft_rad_s": 180.0, "blade_pitch_rad": 0.3
    });
    profile["dynamics"] = forward;
    profile["controls"]["default_trim"] = 0.0.into();
    startup.aircraft = aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(
        AircraftProfileV4::parse(&profile.to_string()).unwrap(),
    );
    startup
}

#[cfg(not(feature = "commercial-staging"))]
pub(crate) fn start() -> StartCondition {
    crate::turboprop_lifecycle_tests::start()
}

#[cfg(not(feature = "commercial-staging"))]
pub(crate) fn recording(steps: u32) -> NearStaticTurbopropRecording {
    let startup = runtime_startup();
    let session =
        FlightSession::prepare_bounded(&startup, &flightsim_render::TimeOfDay::default(), start())
            .unwrap();
    let mut environment = NearStaticTurbopropEnvironment::default();
    environment.conditions.start_epoch = 2_451_545.0;
    environment.conditions.time_rate = 60.0;
    let input = ControlInputs::new(0.0, 0.0, 0.0, 0.4, 0.1);
    let mut simulation = NearStaticTurbopropSimulation::from_supported_state(
        session.near_static_turboprop().unwrap().config().clone(),
        *session.near_static_turboprop().unwrap().state(),
        environment,
        input,
    )
    .unwrap();
    let mut recorder = NearStaticTurbopropRecorder::new(&simulation).unwrap();
    for _ in 0..steps {
        let report = simulation.advance(NEAR_STATIC_TURBOPROP_FIXED_DT, input);
        assert!(report.terminal().is_none(), "{:?}", report.terminal());
        recorder.record(&report).unwrap();
    }
    recorder.finish()
}

#[test]
fn profile4_is_explicit_inspectable_and_never_a_catalog_or_commercial_preset() {
    let startup = startup();
    let selected = aircraft_profile::SelectedAircraftProfile::load(PROFILE_PATH).unwrap();
    assert!(selected.is_turboprop() && selected.is_near_static_turboprop());
    assert!(selected.turboprop().is_none() && selected.jet().is_none());
    let picker = AircraftPicker::new(&startup);
    assert!(
        picker
            .entries
            .iter()
            .all(|entry| !entry.view.label.contains("Cedar"))
    );
    if cfg!(feature = "commercial-staging") {
        assert_eq!(picker.entries.len(), 1);
        assert_eq!(picker.entries[0].view.label, "Swift Sport");
        assert!(
            picker.entries[0]
                .choice
                .as_ref()
                .unwrap()
                .profile
                .is_legacy()
        );
    } else {
        assert_eq!(picker.entries.len(), 4);
        assert!(
            picker.entries[0]
                .choice
                .as_ref()
                .unwrap()
                .profile
                .is_near_static_turboprop()
        );
        assert!(picker.target_uses_bounded_model(0));
        let mut implicit = startup.clone();
        implicit.aircraft_choice = None;
        assert!(!AircraftPicker::new(&implicit).available(0));
    }
    for replay in [false, true] {
        let mut args = vec![
            "--aircraft".to_owned(),
            PROFILE_PATH.to_owned(),
            "--no-model".to_owned(),
        ];
        if replay {
            args.extend(["--replay".into(), "must-not-be-opened.fsreplay".into()]);
        }
        let (parsed, _) = parse_arguments_from(args);
        if cfg!(feature = "commercial-staging") {
            assert_eq!(parsed.aircraft.id(), "swift-sport");
            assert!(
                parsed
                    .aircraft_error
                    .as_ref()
                    .unwrap()
                    .contains("commercial-staging")
            );
        } else {
            assert!(parsed.aircraft.is_near_static_turboprop());
            assert!(parsed.aircraft_error.is_none());
        }
    }
}

#[test]
#[cfg(feature = "commercial-staging")]
fn commercial_profile4_gates_precede_replay_or_terrain_reads() {
    let mut startup = startup();
    startup.replay = Some("must-not-be-opened.fsreplay".into());
    startup.tiles = Some("must-not-be-read".into());
    let before = format!("{startup:?}");
    let error = crate::near_static_turboprop_session::resolve_sources(&mut startup, &mut default())
        .unwrap_err();
    assert!(error.contains("commercial-staging"));
    let result = FlightSession::prepare_bounded(
        &startup,
        &flightsim_render::TimeOfDay::default(),
        StartCondition::Parked {
            position: startup.start,
            heading: startup.heading,
        },
    );
    assert!(matches!(result, Err(error) if error.contains("commercial-staging")));
    assert_eq!(format!("{startup:?}"), before);
}

#[cfg(not(feature = "commercial-staging"))]
mod transactions {
    use super::*;
    use bevy::app::TaskPoolPlugin;
    use bevy::asset::AssetPlugin;
    use bevy::camera::visibility::VisibilityPlugin;
    use bevy::gltf::GltfPlugin;
    use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
    use bevy::mesh::MeshPlugin;
    use bevy::scene::ScenePlugin;
    use bevy::transform::TransformPlugin;
    use flightsim_sim::replay_v5::{TurbopropRecording, TurbopropReplayPlayer};
    use flightsim_sim::replay_v6::NearStaticTurbopropReplayPlayer;
    use std::time::{Duration, Instant};

    fn old_startup() -> Startup {
        let mut old = startup();
        old.aircraft = aircraft_profile::SelectedAircraftProfile::Turboprop(
            flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(include_str!(
                "../../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
            ))
            .unwrap(),
        );
        old.aircraft_choice = Some(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../docs/examples/aircraft-profiles-v3/numerical-turboprop.json"
            )
            .into(),
        );
        old.model_fit = old.aircraft.model_fit();
        old.engine_sound = old.aircraft.engine_kind();
        old
    }
    fn request(generation: u64) -> WorldMapStart {
        WorldMapStart {
            position: Geodetic::from_degrees(0.0, -140.0, 0.0),
            month: 7,
            aircraft_choice: 0,
            generation,
        }
    }
    fn app(initial: Startup) -> App {
        let mut app = App::new();
        app.insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
            .add_plugins((
                TaskPoolPlugin::default(),
                AssetPlugin {
                    file_path: initial.assets.as_ref().unwrap().display().to_string(),
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
            .init_resource::<conditions_runtime::PendingConditions>()
            .init_resource::<flightsim_ui::CrashNotice>()
            .init_resource::<flightsim_ui::HudSmoothing>()
            .init_resource::<HudState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(initial.view)
            .insert_resource(world_runtime::WorldRuntime::new(&initial).unwrap())
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
        let prepared = world_runtime::prepare_world_map_flight(initial, request(0), None).unwrap();
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
                "initial numerical scene did not load"
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
    fn submit(app: &mut App, target: &Startup) -> WorldMapStart {
        app.world_mut().resource_mut::<AircraftPicker>().entries[0].choice =
            Some(AircraftChoice::current(target));
        let generation = {
            let mut a = app.world_mut().resource_mut::<WorldMapActions>();
            a.invalidate_start();
            a.generation
        };
        let requested = request(generation);
        {
            let mut map = app.world_mut().resource_mut::<WorldMapState>();
            map.visible = true;
            map.selected = requested.position;
            map.month = requested.month;
            map.aircraft_choice = 0;
        }
        app.world_mut().resource_mut::<WorldMapActions>().start_at = Some(requested);
        requested
    }
    fn finish(app: &mut App, failure: bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.update();
            let failed = app
                .world()
                .resource::<AircraftPicker>()
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.phase, Phase::Failed));
            if failed || !app.world().resource::<WorldMapState>().visible {
                assert_eq!(
                    failed,
                    failure,
                    "{}",
                    app.world().resource::<WorldMapState>().navigation_note
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "staged numerical scene did not resolve"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn root(world: &mut World) -> Entity {
        world
            .query_filtered::<Entity, With<Aircraft>>()
            .single(world)
            .unwrap()
    }
    fn sound(world: &mut World) -> Entity {
        world
            .query_filtered::<Entity, With<flightsim_audio::FlightSoundSource>>()
            .single(world)
            .unwrap()
    }
    fn record_bytes(session: &FlightSession) -> Vec<u8> {
        let mut bytes = Vec::new();
        match session {
            FlightSession::TurbopropLive { recorder, .. } => {
                recorder.export().write_to(&mut bytes).unwrap()
            }
            FlightSession::NearStaticTurbopropLive { recorder, .. } => {
                recorder.export().write_to(&mut bytes).unwrap()
            }
            _ => panic!("typed live numerical family required"),
        }
        bytes
    }
    fn state(session: &FlightSession) -> TurbopropState {
        if let Some(sim) = session.near_static_turboprop() {
            *sim.state()
        } else {
            *session.turboprop().unwrap().state()
        }
    }
    enum LiveSnapshot {
        Old(flightsim_sim::turboprop_simulation::TurbopropSnapshot),
        Near(flightsim_sim::near_static_turboprop_simulation::NearStaticTurbopropSnapshot),
    }
    impl LiveSnapshot {
        fn capture(session: &FlightSession) -> Self {
            if let Some(sim) = session.near_static_turboprop() {
                Self::Near(sim.snapshot())
            } else {
                Self::Old(session.turboprop().unwrap().snapshot())
            }
        }
        fn assert_retained(&self, session: &FlightSession) {
            match self {
                Self::Old(expected) => assert!(flightsim_sim::replay_v5::snapshot_bits_equal(
                    expected,
                    &session.turboprop().unwrap().snapshot()
                )),
                Self::Near(expected) => assert!(flightsim_sim::replay_v6::snapshot_bits_equal(
                    expected,
                    &session.near_static_turboprop().unwrap().snapshot()
                )),
            }
        }
    }
    struct Frozen {
        state: TurbopropState,
        record: Vec<u8>,
        startup: String,
        controls: String,
        root: Entity,
        sound: Entity,
        bridge: std::sync::Arc<flightsim_audio::SharedSound>,
        source: *const (dyn TileSource + Send + Sync),
        clock: (f64, f64),
        camera: String,
        parking: Option<(bool, bool)>,
        last_controls: Option<ControlInputs>,
        snapshot: LiveSnapshot,
    }
    impl Frozen {
        fn capture(app: &mut App) -> Self {
            let world = app.world_mut();
            let root = root(world);
            let sound = sound(world);
            let session = &world.resource::<FlightSimulation>().0;
            let clock = world.resource::<flightsim_render::TimeOfDay>();
            let result = Self {
                state: state(session),
                record: record_bytes(session),
                startup: format!("{:?}", world.resource::<Startup>()),
                controls: format!("{:?}", world.resource::<PilotControls>()),
                root,
                sound,
                bridge: world.resource::<flightsim_audio::SoundBridge>().0.clone(),
                source: &*world.resource::<TerrainStreaming>().source,
                clock: (clock.utc.get(), clock.rate.get()),
                camera: format!("{:?}", world.resource::<CameraRig>()),
                parking: session.parking_brake(),
                last_controls: session.last_controls(),
                snapshot: LiveSnapshot::capture(session),
            };
            let _ = result.bridge.take_reset();
            result
        }
        fn assert_retained(&self, app: &mut App) {
            let world = app.world_mut();
            assert_eq!(root(world), self.root);
            assert_eq!(sound(world), self.sound);
            let session = &world.resource::<FlightSimulation>().0;
            assert!(state_bits_equal(&state(session), &self.state));
            self.snapshot.assert_retained(session);
            assert_eq!(session.parking_brake(), self.parking);
            assert_eq!(session.last_controls(), self.last_controls);
            assert_eq!(record_bytes(session), self.record);
            assert_eq!(format!("{:?}", world.resource::<Startup>()), self.startup);
            assert_eq!(
                format!("{:?}", world.resource::<PilotControls>()),
                self.controls
            );
            assert!(std::sync::Arc::ptr_eq(
                &world.resource::<flightsim_audio::SoundBridge>().0,
                &self.bridge
            ));
            assert!(!self.bridge.take_reset());
            assert!(std::ptr::eq(
                &*world.resource::<TerrainStreaming>().source,
                self.source
            ));
            let clock = world.resource::<flightsim_render::TimeOfDay>();
            assert_eq!((clock.utc.get(), clock.rate.get()), self.clock);
            assert_eq!(format!("{:?}", world.resource::<CameraRig>()), self.camera);
        }
    }
    fn dirty(app: &mut App) {
        let world = app.world_mut();
        let mut controls = world.resource_mut::<PilotControls>();
        controls.aileron_trim.set(0.01234);
        controls.rudder_trim.set(-0.02345);
        controls.throttle.set_absolute(0.67);
        controls.flaps.set_absolute(0.25);
        let mut controls = world.remove_resource::<PilotControls>().unwrap();
        world.resource_mut::<FlightSimulation>().0.advance_bounded(
            NEAR_STATIC_TURBOPROP_FIXED_DT,
            &mut controls,
            &SampledPilotInput::default(),
            true,
        );
        world.insert_resource(controls);
        assert_eq!(
            world.resource::<FlightSimulation>().0.elapsed(),
            NEAR_STATIC_TURBOPROP_FIXED_DT
        );
        world
            .resource_mut::<FlightSimulation>()
            .0
            .queue_parking_toggle();
        world.resource_mut::<flightsim_ui::Paused>().0 = true;
        world
            .resource_mut::<flightsim_ui::CrashNotice>()
            .set("previous terminal notice");
        world.resource_mut::<StallWarningStatus>().active = true;
    }

    #[test]
    fn both_law_directions_cancel_stale_candidates_without_changing_any_live_owner() {
        for initial_near in [false, true] {
            let mut app = app(if initial_near {
                startup()
            } else {
                old_startup()
            });
            let target = if initial_near {
                old_startup()
            } else {
                startup()
            };
            dirty(&mut app);
            let before = Frozen::capture(&mut app);
            for edit in 0..6 {
                submit(&mut app, &target);
                world_runtime::apply_world_map_start(app.world_mut());
                let staged = match &app
                    .world()
                    .resource::<AircraftPicker>()
                    .pending
                    .as_ref()
                    .unwrap()
                    .phase
                {
                    Phase::Model { scene, .. } => scene.root,
                    _ => panic!("candidate must stage before publishing"),
                };
                before.assert_retained(&mut app);
                match edit {
                    0 => app.world_mut().resource_mut::<WorldMapState>().visible = false,
                    1 => {
                        app.world_mut().resource_mut::<WorldMapState>().selected =
                            Geodetic::from_degrees(20.0, 10.0, 0.0)
                    }
                    2 => app.world_mut().resource_mut::<WorldMapState>().month = 1,
                    3 => {
                        app.world_mut()
                            .resource_mut::<weather_runtime::PendingWeather>()
                            .requested = Some(WeatherPreset::Rain)
                    }
                    4 => {
                        let mut changed = conditions_runtime::PhysicalConditions::from_startup(
                            app.world().resource::<Startup>(),
                        );
                        changed.wind.speed = flightsim_core::MetersPerSecond(7.0);
                        app.world_mut()
                            .resource_mut::<conditions_runtime::PendingConditions>()
                            .selection = Some(changed);
                    }
                    _ => app
                        .world_mut()
                        .resource_mut::<WorldMapActions>()
                        .invalidate_start(),
                }
                world_runtime::apply_world_map_start(app.world_mut());
                assert!(app.world().get_entity(staged).is_err());
                assert!(app.world().resource::<AircraftPicker>().pending.is_none());
                {
                    let mut map = app.world_mut().resource_mut::<WorldMapState>();
                    map.visible = true;
                    map.selected = request(0).position;
                    map.month = 7;
                }
                app.world_mut()
                    .resource_mut::<weather_runtime::PendingWeather>()
                    .requested = None;
                app.world_mut()
                    .resource_mut::<conditions_runtime::PendingConditions>()
                    .selection = None;
                // Let the old asynchronous GLB completion arrive after cancellation.
                for _ in 0..5 {
                    app.update();
                }
                assert!(app.world().resource::<AircraftPicker>().pending.is_none());
                before.assert_retained(&mut app);
            }
        }
    }

    #[test]
    fn both_law_directions_roll_back_bad_assets_and_sources_then_only_success_resets() {
        for initial_near in [false, true] {
            let mut app = app(if initial_near {
                startup()
            } else {
                old_startup()
            });
            let target = if initial_near {
                old_startup()
            } else {
                startup()
            };
            dirty(&mut app);
            let before = Frozen::capture(&mut app);
            for model in [
                "aircraft/missing-nearstatic-test.glb",
                "aircraft/swift_sport.json",
            ] {
                let mut broken = target.clone();
                broken.model = Some(model.into());
                let requested = submit(&mut app, &broken);
                finish(&mut app, true);
                assert_eq!(
                    app.world().resource::<WorldMapActions>().start_at,
                    Some(requested)
                );
                before.assert_retained(&mut app);
            }
            for raw in [false, true] {
                if raw {
                    app.world_mut().resource_mut::<Startup>().tiles =
                        Some("raw-test-terrain".into());
                } else {
                    app.world_mut()
                        .resource_mut::<WorldMapState>()
                        .regions
                        .selected = Some("test-region@1.0.0".into());
                }
                let unchanged = Frozen::capture(&mut app);
                let requested = submit(&mut app, &target);
                world_runtime::apply_world_map_start(app.world_mut());
                unchanged.assert_retained(&mut app);
                assert_eq!(
                    app.world().resource::<WorldMapActions>().start_at,
                    Some(requested)
                );
                app.world_mut().resource_mut::<Startup>().tiles = None;
                app.world_mut()
                    .resource_mut::<WorldMapState>()
                    .regions
                    .selected = None;
            }
            // A finite, schema-valid candidate can still fail physical admission.
            // Only an approach hint changes: 150 m/s is valid metadata but exceeds
            // the unchanged numerical model's Mach 0.3 envelope at this start.
            let mut unsupported = target.clone();
            unsupported.aircraft = match &target.aircraft {
                aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(profile) => {
                    let json = profile.to_json().unwrap();
                    assert!(json.contains("\"approach_speed_mps\": 35.0"));
                    let candidate = AircraftProfileV4::parse(&json.replace(
                        "\"approach_speed_mps\": 35.0",
                        "\"approach_speed_mps\": 150.0",
                    ))
                    .unwrap();
                    assert_eq!(flightsim_sim::near_static_turboprop_identity::canonical_near_static_turboprop_bytes(profile.configuration()), flightsim_sim::near_static_turboprop_identity::canonical_near_static_turboprop_bytes(candidate.configuration()));
                    aircraft_profile::SelectedAircraftProfile::NearStaticTurboprop(candidate)
                }
                aircraft_profile::SelectedAircraftProfile::Turboprop(profile) => {
                    let json = profile.to_json().unwrap();
                    assert!(json.contains("\"approach_speed_mps\": 35.0"));
                    let candidate = flightsim_sim::aircraft_profile_v3::AircraftProfileV3::parse(
                        &json.replace(
                            "\"approach_speed_mps\": 35.0",
                            "\"approach_speed_mps\": 150.0",
                        ),
                    )
                    .unwrap();
                    assert_eq!(
                        flightsim_sim::turboprop_identity::canonical_turboprop_bytes(
                            profile.configuration()
                        ),
                        flightsim_sim::turboprop_identity::canonical_turboprop_bytes(
                            candidate.configuration()
                        )
                    );
                    aircraft_profile::SelectedAircraftProfile::Turboprop(candidate)
                }
                _ => unreachable!(),
            };
            let requested = submit(&mut app, &unsupported);
            world_runtime::apply_world_map_start(app.world_mut());
            assert!(matches!(
                app.world()
                    .resource::<AircraftPicker>()
                    .pending
                    .as_ref()
                    .unwrap()
                    .phase,
                Phase::Failed
            ));
            assert!(
                app.world()
                    .resource::<WorldMapState>()
                    .navigation_note
                    .contains("mach: Above")
            );
            assert_eq!(
                app.world().resource::<WorldMapActions>().start_at,
                Some(requested)
            );
            before.assert_retained(&mut app);

            // A nonfinite pending force snapshot must reject before replacing the old flight.
            // Finite steady wind alone is not an unsupported-start witness: the
            // map deliberately constructs a wind-relative initial velocity.
            let mut conditions = conditions_runtime::PhysicalConditions::from_startup(
                app.world().resource::<Startup>(),
            );
            conditions.wind.speed = flightsim_core::MetersPerSecond(f64::INFINITY);
            app.world_mut()
                .resource_mut::<conditions_runtime::PendingConditions>()
                .selection = Some(conditions);
            let requested = submit(&mut app, &target);
            world_runtime::apply_world_map_start(app.world_mut());
            assert!(matches!(
                app.world()
                    .resource::<AircraftPicker>()
                    .pending
                    .as_ref()
                    .unwrap()
                    .phase,
                Phase::Failed
            ));
            assert_eq!(
                app.world().resource::<WorldMapActions>().start_at,
                Some(requested)
            );
            before.assert_retained(&mut app);
            conditions.wind = flightsim_sim::Wind {
                from: Radians(1.234_567_890_123_456_7),
                speed: flightsim_core::MetersPerSecond(2.25),
            };
            conditions.turbulence = flightsim_fdm::Turbulence::light(u64::MAX - 37);
            app.world_mut()
                .resource_mut::<conditions_runtime::PendingConditions>()
                .selection = Some(conditions);
            app.world_mut()
                .resource_mut::<weather_runtime::PendingWeather>()
                .requested = Some(WeatherPreset::Clear);
            submit(&mut app, &target);
            world_runtime::apply_world_map_start(app.world_mut());
            before.assert_retained(&mut app);
            finish(&mut app, false);
            let world = app.world_mut();
            assert_ne!(root(world), before.root);
            assert!(world.get_entity(before.root).is_err());
            assert_ne!(sound(world), before.sound);
            assert!(world.get_entity(before.sound).is_err());
            assert!(!std::sync::Arc::ptr_eq(
                &world.resource::<flightsim_audio::SoundBridge>().0,
                &before.bridge
            ));
            assert_eq!(before.bridge.master(), 0.0);
            assert!(
                world
                    .resource::<flightsim_audio::SoundBridge>()
                    .0
                    .take_reset()
            );
            let startup = world.resource::<Startup>();
            assert_eq!(startup.aircraft.id(), target.aircraft.id());
            assert_eq!(startup.model, target.model);
            assert_eq!(startup.model_fit, target.model_fit);
            assert_eq!(startup.engine_sound, target.engine_sound);
            assert_eq!(
                conditions_runtime::PhysicalConditions::from_startup(startup),
                conditions
            );
            assert!(matches!(
                startup.weather.selection,
                flightsim_sim::weather::WeatherSelection::Modeled(_)
            ));
            let controls = world.resource::<PilotControls>();
            assert_eq!(
                format!("{controls:?}"),
                format!("{:?}", world_runtime::initial_controls(startup))
            );
            assert_eq!(controls.aileron_trim.value().to_bits(), 0.0_f64.to_bits());
            assert_eq!(controls.rudder_trim.value().to_bits(), 0.0_f64.to_bits());
            assert!(!world.resource::<flightsim_ui::Paused>().0);
            assert!(!world.resource::<flightsim_ui::CrashNotice>().is_crashed());
            assert!(!world.resource::<StallWarningStatus>().active);
            assert!(!world.contains_resource::<FlightRecorder>());
            let session = &world.resource::<FlightSimulation>().0;
            assert_eq!(session.elapsed(), Seconds::ZERO);
            assert_eq!(session.parking_brake(), Some((false, false)));
            let actual = state(session);
            let engine = if let Some(profile) = startup.aircraft.near_static_turboprop() {
                profile.running_start()
            } else {
                startup.aircraft.turboprop().unwrap().running_start()
            };
            assert_eq!(
                actual.turbine_fraction.get().to_bits(),
                engine.turbine_fraction().get().to_bits()
            );
            assert_eq!(
                actual.shaft_rad_s.get().to_bits(),
                engine.shaft_speed().get().to_bits()
            );
            assert_eq!(
                actual.blade_pitch_rad.get().to_bits(),
                engine.blade_pitch().get().to_bits()
            );
            let bytes = record_bytes(session);
            if initial_near {
                assert!(session.near_static_turboprop().is_none());
                let record = TurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
                assert_eq!(record.conditions().identity.schema, 3);
                assert_eq!(record.conditions().identity.law_revision, 1);
                assert!(NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).is_err());
                assert!(
                    TurbopropReplayPlayer::new(
                        startup
                            .aircraft
                            .turboprop()
                            .unwrap()
                            .configuration()
                            .clone(),
                        record
                    )
                    .is_ok()
                );
            } else {
                assert!(session.turboprop().is_none());
                let record =
                    NearStaticTurbopropRecording::read_from(&mut bytes.as_slice()).unwrap();
                assert_eq!(record.conditions().identity.schema, 4);
                assert_eq!(record.conditions().identity.law_revision, 2);
                assert!(TurbopropRecording::read_from(&mut bytes.as_slice()).is_err());
                assert!(
                    NearStaticTurbopropReplayPlayer::new(
                        startup
                            .aircraft
                            .near_static_turboprop()
                            .unwrap()
                            .configuration()
                            .clone(),
                        record
                    )
                    .is_ok()
                );
            }
        }
    }
}
