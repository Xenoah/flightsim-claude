use super::*;
use bevy::ecs::system::RunSystemOnce;
use flightsim_core::Meters;
use flightsim_ui::world_map::WorldMapWeatherEdit;

fn edit() -> WorldMapWeatherEdit {
    WorldMapWeatherEdit {
        background_visibility: None,
        cloud_base: None,
    }
}
fn world() -> World {
    let mut startup = crate::parse_arguments_from(
        [
            "--weather",
            "rain",
            "--weather-seed",
            "18446744073709551599",
        ]
        .map(str::to_owned),
    )
    .0;
    startup.wind = flightsim_sim::Wind {
        from: flightsim_core::Radians(1.2345678901234567),
        speed: flightsim_core::MetersPerSecond(9.876543210987654),
    };
    startup.turbulence = flightsim_fdm::Turbulence {
        intensity: flightsim_core::MetersPerSecond(2.345678901234567),
        seed: u64::MAX - 19,
    };
    let mut world = World::new();
    world.insert_resource(startup);
    world.init_resource::<PendingWeather>();
    world.init_resource::<WorldMapState>();
    world.resource_mut::<WorldMapState>().visible = true;
    world.init_resource::<WorldMapActions>();
    world.init_resource::<world_runtime::MapCapture>();
    world.init_resource::<ButtonInput<KeyCode>>();
    world.run_system_once(select_pending_weather).unwrap();
    world
}

#[test]
fn custom_pending_apply_is_atomic_and_retains_active_weather_and_exact_forces() {
    let mut world = world();
    let before = world.resource::<Startup>().clone();
    let forces = crate::conditions_runtime::PhysicalConditions::from_startup(&before);
    let generation = world.resource::<WorldMapActions>().generation;
    world.resource_mut::<WorldMapActions>().weather = Some(WorldMapWeatherEdit {
        background_visibility: Some(Meters(2000.123456789)),
        cloud_base: Some(Meters(200.123456789)),
    });
    world.run_system_once(select_pending_weather).unwrap();
    let chosen = snapshot(&world, world.resource::<Startup>());
    assert_eq!(chosen.requested, Some(WeatherPreset::Custom));
    assert!(world.resource::<WorldMapActions>().generation > generation);
    let mut pending = before.clone();
    world_runtime::apply_pending_weather(&world, &mut pending);
    assert_eq!(
        crate::conditions_runtime::PhysicalConditions::from_startup(&pending),
        forces
    );
    assert_eq!(
        world.resource::<Startup>().weather.selection,
        before.weather.selection
    );
    assert_eq!(
        world.resource::<Startup>().weather.requested,
        before.weather.requested
    );
    for invalid in [-1.0, f64::NAN, 30000.0] {
        world.resource_mut::<WorldMapActions>().weather = Some(WorldMapWeatherEdit {
            background_visibility: Some(Meters(3000.0)),
            cloud_base: Some(Meters(invalid)),
        });
        world.run_system_once(select_pending_weather).unwrap();
        let actual = snapshot(&world, world.resource::<Startup>());
        assert_eq!(actual.draft, chosen.draft);
        assert_eq!(actual.requested, chosen.requested);
        assert!(actual.revision > chosen.revision);
        assert!(
            !world
                .resource::<WorldMapState>()
                .weather_settings
                .error
                .is_empty()
        );
    }
    world.resource_mut::<WorldMapActions>().weather = Some(edit());
    world.run_system_once(select_pending_weather).unwrap();
    assert_eq!(
        snapshot(&world, world.resource::<Startup>()).draft,
        chosen.draft
    );
    assert_eq!(
        crate::conditions_runtime::PhysicalConditions::from_startup(world.resource::<Startup>()),
        forces
    );
}

#[test]
fn map_close_discards_custom_pending_and_reopening_keeps_committed_template_exact() {
    let mut world = world();
    let original = snapshot(&world, world.resource::<Startup>());
    world.resource_mut::<WorldMapActions>().weather = Some(WorldMapWeatherEdit {
        background_visibility: Some(Meters(2000.123456789)),
        ..edit()
    });
    world.run_system_once(select_pending_weather).unwrap();
    let changed = snapshot(&world, world.resource::<Startup>());
    assert_ne!(changed.draft, original.draft);
    world.resource_mut::<WorldMapState>().visible = false;
    world.run_system_once(select_pending_weather).unwrap();
    world.resource_mut::<WorldMapState>().visible = true;
    world.run_system_once(select_pending_weather).unwrap();
    assert_eq!(
        snapshot(&world, world.resource::<Startup>()).draft,
        original.draft
    );
    changed.apply(&mut world.resource_mut::<Startup>().weather);
    world.resource_mut::<WorldMapState>().visible = false;
    world.run_system_once(select_pending_weather).unwrap();
    world.resource_mut::<WorldMapState>().visible = true;
    world.run_system_once(select_pending_weather).unwrap();
    assert_eq!(
        snapshot(&world, world.resource::<Startup>()).draft,
        changed.draft
    );
    assert_eq!(
        world
            .resource::<WorldMapState>()
            .weather_settings
            .background_visibility,
        "2000.123"
    );
    assert!(
        world
            .resource::<WorldMapState>()
            .weather_note
            .contains("MODELED CUSTOM")
    );
}

#[test]
fn legacy_manual_lan_replay_and_child_modals_reject_injected_visual_edits() {
    for mode in 0..6 {
        let mut world = world();
        match mode {
            0 => {
                world.resource_mut::<PendingWeather>().requested = None;
            }
            1 => world.resource_mut::<Startup>().clouds_were_given = true,
            2 => {
                world.resource_mut::<Startup>().traffic.host =
                    Some("127.0.0.1:39999".parse().unwrap())
            }
            3 => world.resource_mut::<Startup>().replay = Some("recorded.fsreplay".into()),
            4 => world.resource_mut::<WorldMapState>().show_credits(),
            _ => world.resource_mut::<WorldMapState>().show_regions(),
        }
        let original = world.resource::<PendingWeather>().draft;
        let requested = world.resource::<PendingWeather>().requested;
        world.resource_mut::<WorldMapActions>().weather = Some(WorldMapWeatherEdit {
            background_visibility: Some(Meters(2000.0)),
            ..edit()
        });
        world.run_system_once(select_pending_weather).unwrap();
        assert_eq!(world.resource::<PendingWeather>().draft, original);
        assert_eq!(world.resource::<PendingWeather>().requested, requested);
        assert!(world.resource::<WorldMapActions>().weather.is_none());
        assert_eq!(
            world.resource::<WorldMapState>().weather_settings.enabled,
            mode >= 4
        );
    }
}

#[test]
fn custom_visual_edit_keeps_nontrivial_wind_turbulence_month_controls_and_flown_bits() {
    use flightsim_core::Seconds;
    let source = world();
    let startup = source.resource::<Startup>().clone();
    let forces = crate::conditions_runtime::PhysicalConditions::from_startup(&startup);
    let mut custom_startup = startup.clone();
    let draft = AuthoredDraft::from_preset(WeatherPreset::Rain, startup.weather.seed)
        .unwrap()
        .edited(WorldMapWeatherEdit {
            background_visibility: Some(Meters(2000.0)),
            cloud_base: Some(Meters(200.0)),
        })
        .unwrap();
    custom_startup.weather.requested = Some(WeatherPreset::Custom);
    custom_startup.weather.draft = Some(draft);
    let request = flightsim_ui::world_map::WorldMapStart {
        position: Geodetic::from_degrees(0.0, -140.0, 0.0),
        month: 11,
        aircraft_choice: 0,
        generation: 1,
    };
    let mut canonical = world_runtime::prepare_world_map_flight(startup, request, None).unwrap();
    let mut custom =
        world_runtime::prepare_world_map_flight(custom_startup, request, None).unwrap();
    assert_eq!(
        crate::conditions_runtime::PhysicalConditions::from_startup(&custom.startup),
        forces
    );
    assert_eq!(
        custom.startup.world.climate_date,
        canonical.startup.world.climate_date
    );
    assert_eq!(custom.startup.world.civil_date, (2026, 11, 15));
    let clock = flightsim_render::TimeOfDay::default();
    let conditions = crate::recording_conditions(&canonical.startup, &clock);
    let custom_conditions = crate::recording_conditions(&custom.startup, &clock);
    assert_eq!(conditions.environment, custom_conditions.environment);
    let mut record = flightsim_sim::CurrentRecorder::new(conditions);
    let mut custom_record = flightsim_sim::CurrentRecorder::new(custom_conditions);
    for step in 0..360 {
        let controls = flightsim_fdm::ControlInputs::neutral().with_throttle(if step < 120 {
            0.6
        } else {
            0.4
        });
        for (prepared, recorder) in [
            (&mut canonical, &mut record),
            (&mut custom, &mut custom_record),
        ] {
            prepared.simulation.legacy_mut().advance_with_controls(
                Seconds(1.0 / 120.0),
                |dt, state| {
                    recorder.record(dt, controls, Some(state));
                    controls
                },
            );
        }
        assert!(flightsim_sim::replay_v4::state_bits_equal(
            canonical.simulation.state(),
            custom.simulation.state()
        ));
    }
    assert_eq!(
        record.recording().frames(),
        custom_record.recording().frames()
    );
    assert_eq!(
        record.recording().keyframes(),
        custom_record.recording().keyframes()
    );
    assert_ne!(
        record.recording().conditions().weather,
        custom_record.recording().conditions().weather
    );
    assert_eq!(custom.simulation.elapsed(), canonical.simulation.elapsed());
}

#[test]
fn edited_fog_background_visibility_reaches_existing_additive_renderer_optics() {
    use flightsim_core::Seconds;
    let reference = Geodetic::from_degrees(0.0, 180.0, -430.0);
    let sample = Geodetic::from_degrees(0.0, 180.0, -280.0);
    let draft = AuthoredDraft::from_preset(WeatherPreset::Fog, 42).unwrap();
    let mut previous = 0.0;
    for visibility in [10.0, 2000.0, 200000.0] {
        let scenario = draft
            .edited(WorldMapWeatherEdit {
                background_visibility: Some(Meters(visibility)),
                ..edit()
            })
            .unwrap()
            .resolve(reference)
            .unwrap();
        let extinction =
            flightsim_render::modeled_weather::weather_extinction(scenario, Seconds::ZERO, sample);
        // Independent published convention: -ln(0.02) = 3.912023005428146.
        // Fog's retained 250 m contribution adds to the edited background.
        let expected = 3.912_023_005_428_146 * (1.0 / visibility + 1.0 / 250.0);
        let actual = f64::from(extinction.ambient + extinction.fog + extinction.cloud);
        assert!((actual - expected).abs() < expected * 2.0e-7);
        let transmission = (-actual * 100.0).exp();
        assert!(transmission >= previous);
        assert!(transmission < (-3.912_023_005_428_146 / visibility * 100.0).exp());
        previous = transmission;
        assert_eq!(
            scenario
                .parameters()
                .fog
                .unwrap()
                .visibility
                .get()
                .to_bits(),
            250.0_f64.to_bits()
        );
    }
}
