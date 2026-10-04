use super::*;
use bevy::ecs::system::RunSystemOnce;
use flightsim_core::{Degrees, Knots, MetersPerSecond, Radians};

fn exact_startup() -> Startup {
    let mut startup = crate::parse_arguments_from([]).0;
    startup.wind = Wind {
        from: Radians(1.234_567_890_123_456_7),
        speed: MetersPerSecond(9.876_543_210_987_654),
    };
    startup.turbulence = Turbulence {
        intensity: MetersPerSecond(2.345_678_901_234_567),
        seed: u64::MAX - 17,
    };
    startup.wind_was_given = true;
    startup
}

fn edit() -> WorldMapConditionsEdit {
    WorldMapConditionsEdit {
        wind_from: None,
        wind_speed: None,
        turbulence: None,
    }
}

fn world(startup: Startup) -> World {
    let mut world = World::new();
    world.insert_resource(startup);
    world.init_resource::<PendingConditions>();
    world.init_resource::<WorldMapActions>();
    world.init_resource::<WorldMapState>();
    world.resource_mut::<WorldMapState>().visible = true;
    world.run_system_once(select_pending_conditions).unwrap();
    world
}

#[test]
fn no_edit_keeps_exact_nonrounded_values_seed_and_override_provenance() {
    let startup = exact_startup();
    let before = PhysicalConditions::from_startup(&startup);
    let mut world = world(startup);
    assert_eq!(
        world.resource::<WorldMapState>().wind_settings.wind_speed,
        "19.198"
    );
    world.resource_mut::<WorldMapActions>().conditions = Some(edit());
    world.run_system_once(select_pending_conditions).unwrap();
    assert_eq!(snapshot(&world, world.resource::<Startup>()), before);
    assert_eq!(
        PhysicalConditions::from_startup(world.resource::<Startup>()),
        before
    );
    let mut copy = world.resource::<Startup>().clone();
    snapshot(&world, &copy).apply(&mut copy);
    assert_eq!(PhysicalConditions::from_startup(&copy), before);
}

#[test]
fn physical_settings_note_is_complete_after_ui_formatting() {
    use flightsim_ui::world_map::{WindSettingsText, WorldMapText, format_world_map_text};
    let mut world = world(exact_startup());
    let mut map = world.resource_mut::<WorldMapState>();
    assert!(map.show_wind_settings());
    let rendered = format_world_map_text(
        WorldMapText::Wind(WindSettingsText::Note),
        &map,
        &flightsim_ui::WorldMapRaster::default(),
    );
    assert_eq!(rendered, "Visual presets do not change wind or turbulence.");
    assert!(!rendered.contains("..."));
}

#[test]
fn each_edited_component_preserves_other_exact_components_and_sets_only_its_override() {
    let startup = exact_startup();
    let before = PhysicalConditions::from_startup(&startup);
    let from = before
        .edited(WorldMapConditionsEdit {
            wind_from: Some(Degrees(270.0)),
            ..edit()
        })
        .unwrap();
    assert_eq!(
        from.wind.speed.get().to_bits(),
        before.wind.speed.get().to_bits()
    );
    assert_eq!(from.turbulence, before.turbulence);
    assert!((from.wind.from.get() - std::f64::consts::PI * 1.5).abs() < 1e-14);
    let speed = before
        .edited(WorldMapConditionsEdit {
            wind_speed: Some(Knots(20.0)),
            ..edit()
        })
        .unwrap();
    assert_eq!(
        speed.wind.from.get().to_bits(),
        before.wind.from.get().to_bits()
    );
    assert!((speed.wind.speed.get() - 1852.0 / 180.0).abs() < 1e-12);
    assert_eq!(speed.turbulence, before.turbulence);
    for (level, expected) in [
        (WorldMapTurbulence::Calm, Turbulence::CALM),
        (WorldMapTurbulence::Light, Turbulence::light(1)),
        (WorldMapTurbulence::Moderate, Turbulence::moderate(1)),
        (WorldMapTurbulence::Severe, Turbulence::severe(1)),
    ] {
        let next = before
            .edited(WorldMapConditionsEdit {
                turbulence: Some(level),
                ..edit()
            })
            .unwrap();
        assert_eq!(next.wind, before.wind);
        assert_eq!(next.turbulence.intensity, expected.intensity);
        assert_eq!(next.turbulence.seed, before.turbulence.seed);
        assert!(next.turbulence_was_given);
    }
}

#[test]
fn invalid_action_is_atomic_and_never_mutates_pending_or_active_flight() {
    let mut world = world(exact_startup());
    let before = snapshot(&world, world.resource::<Startup>());
    for invalid in [-1.0, 300.000_1, f64::NAN, f64::INFINITY] {
        world.resource_mut::<WorldMapActions>().conditions = Some(WorldMapConditionsEdit {
            wind_from: Some(Degrees(270.0)),
            wind_speed: Some(Knots(invalid)),
            turbulence: Some(WorldMapTurbulence::Severe),
        });
        world.run_system_once(select_pending_conditions).unwrap();
        assert_eq!(snapshot(&world, world.resource::<Startup>()), before);
        assert_eq!(
            PhysicalConditions::from_startup(world.resource::<Startup>()),
            before
        );
        assert!(
            !world
                .resource::<WorldMapState>()
                .wind_settings
                .error
                .is_empty()
        );
    }
    for invalid in [-1.0, 360.000_1, f64::NAN, f64::INFINITY] {
        assert!(
            before
                .edited(WorldMapConditionsEdit {
                    wind_from: Some(Degrees(invalid)),
                    ..edit()
                })
                .is_err()
        );
    }
}

#[test]
fn map_close_reopen_discards_pending_edit_and_preserves_active_values() {
    let mut world = world(exact_startup());
    let before = PhysicalConditions::from_startup(world.resource::<Startup>());
    world.resource_mut::<WorldMapActions>().conditions = Some(WorldMapConditionsEdit {
        wind_speed: Some(Knots(30.0)),
        ..edit()
    });
    let generation = world.resource::<WorldMapActions>().generation;
    world.run_system_once(select_pending_conditions).unwrap();
    assert_ne!(snapshot(&world, world.resource::<Startup>()), before);
    assert!(world.resource::<WorldMapActions>().generation > generation);
    assert_eq!(
        PhysicalConditions::from_startup(world.resource::<Startup>()),
        before
    );
    world.resource_mut::<WorldMapState>().visible = false;
    world.run_system_once(select_pending_conditions).unwrap();
    world.resource_mut::<WorldMapState>().visible = true;
    world.run_system_once(select_pending_conditions).unwrap();
    assert_eq!(snapshot(&world, world.resource::<Startup>()), before);
}

#[test]
fn replay_lan_and_child_modal_reject_injected_edits_but_manual_clouds_do_not() {
    for mode in 0..4 {
        let mut startup = exact_startup();
        if mode == 0 {
            startup.replay = Some("recorded.fsreplay".into());
        }
        if mode == 1 {
            startup.traffic.host = Some("127.0.0.1:39999".parse().unwrap());
        }
        if mode == 3 {
            startup.clouds_were_given = true;
        }
        let mut world = world(startup);
        let before = PhysicalConditions::from_startup(world.resource::<Startup>());
        if mode == 2 {
            world.resource_mut::<WorldMapState>().show_credits();
        }
        world.resource_mut::<WorldMapActions>().conditions = Some(WorldMapConditionsEdit {
            wind_speed: Some(Knots(30.0)),
            ..edit()
        });
        world.run_system_once(select_pending_conditions).unwrap();
        assert_eq!(
            PhysicalConditions::from_startup(world.resource::<Startup>()),
            before
        );
        if mode == 3 {
            assert_ne!(snapshot(&world, world.resource::<Startup>()), before);
        } else {
            assert_eq!(snapshot(&world, world.resource::<Startup>()), before);
        }
        assert_eq!(
            world.resource::<WorldMapState>().wind_settings.enabled,
            mode >= 2
        );
    }
}

#[test]
fn exact_snapshot_distinguishes_signed_zero_and_seed() {
    let mut first = PhysicalConditions::from_startup(&exact_startup());
    first.wind.from = Radians(0.0);
    let mut second = first;
    second.wind.from = Radians(-0.0);
    assert_ne!(first, second);
    second = first;
    second.turbulence.seed ^= 1;
    assert_ne!(first, second);
}

#[test]
fn explicit_field_choices_survive_later_difficulty_resolution() {
    let mut startup = exact_startup();
    let chosen = PhysicalConditions::from_startup(&startup)
        .edited(WorldMapConditionsEdit {
            wind_from: Some(Degrees(360.0)),
            wind_speed: Some(Knots(22.5)),
            turbulence: Some(WorldMapTurbulence::Calm),
        })
        .unwrap();
    chosen.apply(&mut startup);
    startup.difficulty = crate::Difficulty::Realistic;
    crate::apply_difficulty(&mut startup);
    assert_eq!(PhysicalConditions::from_startup(&startup), chosen);
    assert_eq!(startup.wind.from.get().to_bits(), 0.0_f64.to_bits());
    assert_eq!(startup.turbulence.seed, u64::MAX - 17);
    assert_eq!(
        startup.turbulence.intensity.get().to_bits(),
        0.0_f64.to_bits()
    );
}

#[test]
#[cfg(not(feature = "commercial-staging"))]
fn typed_v5_replay_rejects_wind_edits_without_a_legacy_replay_resource_or_cli_path() {
    let startup = crate::turboprop_lifecycle_tests::startup();
    let record = crate::turboprop_lifecycle_tests::recording(0);
    let player = flightsim_sim::replay_v5::TurbopropReplayPlayer::new(
        startup
            .aircraft
            .turboprop()
            .unwrap()
            .configuration()
            .clone(),
        record,
    )
    .unwrap();
    let mut world = world(startup);
    assert!(world.resource::<Startup>().replay.is_none());
    let before = PhysicalConditions::from_startup(world.resource::<Startup>());
    world.insert_resource(FlightSimulation(crate::FlightSession::replay_turboprop(
        player,
    )));
    world.resource_mut::<WorldMapActions>().conditions = Some(WorldMapConditionsEdit {
        wind_speed: Some(Knots(30.0)),
        turbulence: Some(WorldMapTurbulence::Severe),
        ..edit()
    });
    world.run_system_once(select_pending_conditions).unwrap();
    assert!(!world.resource::<WorldMapState>().wind_settings.enabled);
    assert_eq!(snapshot(&world, world.resource::<Startup>()), before);
    assert_eq!(
        PhysicalConditions::from_startup(world.resource::<Startup>()),
        before
    );
    assert!(world.resource::<WorldMapActions>().conditions.is_none());
}
