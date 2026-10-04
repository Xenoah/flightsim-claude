//! 音声デバイスや renderer を作らず、機体切替の所有権と出力を検査する。

use super::*;
use bevy::audio::Decodable;

fn audio_app(settings: AudioSettings) -> App {
    let mut app = App::new();
    app.insert_resource(settings)
        .init_resource::<AircraftSound>()
        .init_resource::<Assets<FlightAudio>>()
        .insert_resource(SoundBridge(Arc::new(SharedSound::default())))
        .add_systems(Startup, spawn_sound_source);
    app.update();
    app
}

fn only_source(world: &mut World) -> (Entity, Handle<FlightAudio>) {
    let mut query =
        world.query_filtered::<(Entity, &AudioPlayer<FlightAudio>), With<FlightSoundSource>>();
    let (entity, player) = query.single(world).expect("exactly one owned player");
    (entity, player.0.clone())
}

#[test]
fn startup_keeps_the_selected_engine_and_volume() {
    let settings = AudioSettings {
        engine: EngineKind::Piston(EngineSpec::default()),
        master: 0.25,
        enabled: true,
    };
    let mut app = audio_app(settings);
    let (_, handle) = only_source(app.world_mut());
    let world = app.world();
    let source = world
        .resource::<Assets<FlightAudio>>()
        .get(&handle)
        .unwrap();
    assert_eq!(source.kind(), settings.engine);
    assert!(Arc::ptr_eq(
        source.shared(),
        &world.resource::<SoundBridge>().0
    ));
    assert!((source.shared().master() - settings.master).abs() < 1e-6);
    assert_eq!(*world.resource::<AudioSettings>(), settings);
}

#[test]
fn replacement_switches_piston_to_turbine_and_back_without_retaining_old_assets() {
    let piston = EngineKind::Piston(EngineSpec::default());
    let turbine = EngineKind::Turbine(TurbineSpec::default());
    let mut app = audio_app(AudioSettings {
        engine: piston,
        master: 0.25,
        ..default()
    });

    for expected in [turbine, piston] {
        let world = app.world_mut();
        let (old_entity, old_handle) = only_source(world);
        let old_bridge = Arc::downgrade(&world.resource::<SoundBridge>().0);

        replace_sound_source(world, expected);

        let (new_entity, new_handle) = only_source(world);
        assert_ne!(new_entity, old_entity);
        assert_ne!(new_handle.id(), old_handle.id());
        assert!(world.get_entity(old_entity).is_err());
        let assets = world.resource::<Assets<FlightAudio>>();
        assert!(assets.get(&old_handle).is_none());
        assert_eq!(assets.len(), 1);
        let new_source = assets.get(&new_handle).unwrap();
        assert_eq!(new_source.kind(), expected);
        assert!(Arc::ptr_eq(
            new_source.shared(),
            &world.resource::<SoundBridge>().0
        ));
        assert!(
            old_bridge.upgrade().is_none(),
            "retired bridge was retained"
        );
        assert_eq!(world.resource::<AudioSettings>().engine, expected);
        assert!((world.resource::<AudioSettings>().master - 0.25).abs() < 1e-12);
        assert!(world.resource::<AudioSettings>().enabled);
        assert!(new_source.shared().take_reset());
        assert!(!new_source.shared().take_reset());
    }
}

#[test]
fn repeated_replacements_always_leave_one_player_and_one_asset() {
    let mut world = World::new();
    for _ in 0..8 {
        for engine in [
            EngineKind::Piston(EngineSpec::default()),
            EngineKind::Turbine(TurbineSpec::default()),
            EngineKind::Turbine(TurbineSpec::default()),
        ] {
            replace_sound_source(&mut world, engine);
            only_source(&mut world);
            assert_eq!(
                world
                    .query::<&AudioPlayer<FlightAudio>>()
                    .iter(&world)
                    .count(),
                1
            );
            assert_eq!(world.resource::<Assets<FlightAudio>>().len(), 1);
        }
    }
}

#[test]
fn replacement_silences_the_old_decoder_and_discards_the_previous_flight_state() {
    let mut app = audio_app(AudioSettings::default());
    let world = app.world_mut();
    let (_, handle) = only_source(world);
    let old_bridge = world.resource::<SoundBridge>().0.clone();
    let mut old_stream = world
        .resource::<Assets<FlightAudio>>()
        .get(&handle)
        .unwrap()
        .decoder();
    let loud = AircraftSound {
        throttle: 1.0,
        airspeed: MetersPerSecond(60.0),
        stall_warning: true,
        muted: false,
    };
    world.insert_resource(loud);
    old_bridge.set(loud.to_flight_sound());
    let peak = old_stream
        .by_ref()
        .take(48_000)
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.01, "the old stream never sounded");

    replace_sound_source(world, EngineKind::Piston(EngineSpec::default()));

    assert!(old_bridge.get().muted);
    assert!(old_bridge.master().abs() < f64::EPSILON);
    assert!(
        old_stream
            .take(4_800)
            .all(|sample| sample.abs() < f32::EPSILON)
    );
    let quiet = AircraftSound {
        muted: true,
        ..default()
    };
    assert_eq!(*world.resource::<AircraftSound>(), quiet);
    let (_, new_handle) = only_source(world);
    let new_source = world
        .resource::<Assets<FlightAudio>>()
        .get(&new_handle)
        .unwrap();
    assert_eq!(new_source.shared().get(), quiet.to_flight_sound());
    assert!(
        new_source
            .decoder()
            .take(4_800)
            .all(|sample| sample.abs() < f32::EPSILON)
    );
}

#[test]
fn disabled_replacements_keep_the_preference_and_never_create_a_player() {
    let settings = AudioSettings {
        enabled: false,
        master: 0.125,
        ..default()
    };
    let mut app = audio_app(settings);
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&AudioPlayer<FlightAudio>>()
            .iter(world)
            .count(),
        0
    );

    for engine in [
        EngineKind::Piston(EngineSpec::default()),
        EngineKind::default(),
    ] {
        replace_sound_source(world, engine);
        assert_eq!(world.query::<&FlightSoundSource>().iter(world).count(), 0);
        assert_eq!(
            world
                .query::<&AudioPlayer<FlightAudio>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(world.resource::<Assets<FlightAudio>>().len(), 0);
        assert_eq!(
            *world.resource::<AudioSettings>(),
            AudioSettings { engine, ..settings }
        );
        assert!(world.resource::<SoundBridge>().0.master().abs() < f64::EPSILON);
        assert!(world.resource::<SoundBridge>().0.take_reset());
    }
}

#[test]
fn disabling_before_replacement_retires_the_existing_player() {
    let mut app = audio_app(AudioSettings {
        master: 0.25,
        ..default()
    });
    let world = app.world_mut();
    let (entity, handle) = only_source(world);
    let old_bridge = world.resource::<SoundBridge>().0.clone();
    // rodio の idle sink は出力機器を開かず、実際の停止経路を通せる。
    let (sink, _output) = rodio::Sink::new_idle();
    sink.append(
        world
            .resource::<Assets<FlightAudio>>()
            .get(&handle)
            .unwrap()
            .decoder(),
    );
    world.entity_mut(entity).insert(AudioSink::new(sink));
    world.resource_mut::<AudioSettings>().enabled = false;

    replace_sound_source(world, EngineKind::Piston(EngineSpec::default()));

    assert!(world.get_entity(entity).is_err());
    assert_eq!(
        world
            .query::<&AudioPlayer<FlightAudio>>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(world.resource::<Assets<FlightAudio>>().len(), 0);
    assert!(old_bridge.get().muted);
    assert!(old_bridge.master().abs() < f64::EPSILON);
    assert!(!world.resource::<AudioSettings>().enabled);
    assert!((world.resource::<AudioSettings>().master - 0.25).abs() < 1e-12);
}

#[test]
fn replacement_retires_all_owned_sources_and_preserves_unrelated_audio() {
    let mut app = audio_app(AudioSettings::default());
    let world = app.world_mut();
    let extra_bridge = Arc::new(SharedSound::default());
    let extra_handle = world
        .resource_mut::<Assets<FlightAudio>>()
        .add(FlightAudio::new(
            Arc::clone(&extra_bridge),
            EngineKind::default(),
        ));
    let extra_entity = world
        .spawn((
            FlightSoundSource {
                shared: Arc::clone(&extra_bridge),
                handle: extra_handle.clone(),
            },
            AudioPlayer(extra_handle.clone()),
        ))
        .id();
    let unrelated_bridge = Arc::new(SharedSound::default());
    let unrelated_handle = world
        .resource_mut::<Assets<FlightAudio>>()
        .add(FlightAudio::new(
            Arc::clone(&unrelated_bridge),
            EngineKind::default(),
        ));
    let unrelated_entity = world.spawn(AudioPlayer(unrelated_handle.clone())).id();

    replace_sound_source(world, EngineKind::Piston(EngineSpec::default()));

    only_source(world);
    assert!(world.get_entity(extra_entity).is_err());
    assert!(
        world
            .resource::<Assets<FlightAudio>>()
            .get(&extra_handle)
            .is_none()
    );
    assert!(extra_bridge.get().muted);
    assert!(extra_bridge.master().abs() < f64::EPSILON);
    assert!(world.get_entity(unrelated_entity).is_ok());
    assert!(
        world
            .resource::<Assets<FlightAudio>>()
            .get(&unrelated_handle)
            .is_some()
    );
    assert!(!unrelated_bridge.get().muted);
    assert!((unrelated_bridge.master() - DEFAULT_MASTER).abs() < 1e-6);
    assert_eq!(world.resource::<Assets<FlightAudio>>().len(), 2);
}
