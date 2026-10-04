//! New-flight authored weather and the executed-physics-clock render bridge.
//! Pending map choices never mutate the current flight or its initial recording.

use bevy::prelude::*;
use flightsim_core::Geodetic;
use flightsim_render::RenderWeather;
use flightsim_sim::{
    GroundSampler,
    weather::{WeatherPreset, WeatherScenario, WeatherSelection},
};
use flightsim_ui::{WorldMapActions, WorldMapState, world_map::WorldMapSystems};

use crate::{FlightSimulation, ReplayPlayback, Startup, world_runtime};

#[derive(Debug, Clone, Default)]
pub(super) struct Options {
    pub requested: Option<WeatherPreset>,
    pub seed: u64,
    pub was_given: bool,
    pub seed_was_given: bool,
    pub error: Option<String>,
    pub selection: WeatherSelection,
}

impl Options {
    pub fn parse_flag(&mut self, flag: &str, value: Option<&str>) {
        let duplicate = if flag == "--weather" {
            std::mem::replace(&mut self.was_given, true)
        } else {
            std::mem::replace(&mut self.seed_was_given, true)
        };
        if duplicate {
            self.error = Some(format!("{flag} may only be specified once"));
            return;
        }
        if flag == "--weather" {
            self.requested = match value {
                Some("legacy") => None,
                Some("clear") => Some(WeatherPreset::Clear),
                Some("cloud") => Some(WeatherPreset::Cloud),
                Some("fog") => Some(WeatherPreset::Fog),
                Some("rain") => Some(WeatherPreset::Rain),
                Some("snow") => Some(WeatherPreset::Snow),
                Some("storm") => Some(WeatherPreset::Storm),
                _ => {
                    self.error =
                        Some("--weather expects legacy|clear|cloud|fog|rain|snow|storm".into());
                    None
                }
            };
        } else if let Some(seed) = value.and_then(|text| text.parse::<u64>().ok()) {
            self.seed = seed;
        } else {
            self.error = Some("--weather-seed expects an unsigned 64-bit integer".into());
        }
    }

    pub fn validate_flags(&mut self, manual_clouds: bool, replay: bool) {
        if self.seed_was_given && self.requested.is_none() {
            self.error =
                Some("--weather-seed requires an explicit modeled --weather preset".into());
        }
        if (self.was_given || self.seed_was_given) && manual_clouds {
            self.error =
                Some("--weather/--weather-seed cannot be combined with manual cloud flags".into());
        }
        if (self.was_given || self.seed_was_given) && replay {
            self.error = Some(
                "recorded replay weather is authoritative; remove --weather/--weather-seed".into(),
            );
        }
    }
}

pub(super) fn resolve_at(
    options: &Options,
    reference: Geodetic,
) -> Result<WeatherSelection, String> {
    options
        .requested
        .map_or(Ok(WeatherSelection::Legacy), |preset| {
            WeatherScenario::from_preset(preset, reference, options.seed)
                .map(WeatherSelection::Modeled)
                .map_err(|error| format!("cannot resolve authored weather: {error}"))
        })
}

/// Capture the same terrain sampler/datum used by flight initialization, once.
/// Legacy does no additional terrain work. Replay bypasses this constructor.
pub(super) fn resolve_departure(startup: &mut Startup) -> Result<(), String> {
    if startup.weather.requested.is_none() {
        startup.weather.selection = WeatherSelection::Legacy;
        return Ok(());
    }
    let mut terrain = flightsim_world::Terrain::new(
        crate::make_source(startup),
        8 * 1024 * 1024,
        world_runtime::terrain_levels(startup),
    );
    resolve_with_terrain(startup, &mut terrain)
}

pub(super) fn resolve_with_terrain(
    startup: &mut Startup,
    terrain: &mut flightsim_world::Terrain<crate::BoxedSource>,
) -> Result<(), String> {
    if startup.weather.requested.is_none() {
        startup.weather.selection = WeatherSelection::Legacy;
        return Ok(());
    }
    let ground = GroundSampler::default()
        .sample(terrain, startup.start)
        .elevation;
    let reference = Geodetic::new(startup.start.latitude, startup.start.longitude, ground);
    startup.weather.selection = resolve_at(&startup.weather, reference)?;
    Ok(())
}

pub(super) const fn preset_label(preset: WeatherPreset) -> &'static str {
    match preset {
        WeatherPreset::Custom => "MODELED CUSTOM",
        WeatherPreset::Clear => "MODELED CLEAR",
        WeatherPreset::Cloud => "MODELED CLOUD",
        WeatherPreset::Fog => "MODELED FOG",
        WeatherPreset::Rain => "MODELED RAIN",
        WeatherPreset::Snow => "MODELED SNOW",
        WeatherPreset::Storm => "MODELED STORM",
    }
}

#[derive(Resource, Debug, Default)]
pub(super) struct PendingWeather {
    pub requested: Option<WeatherPreset>,
    was_visible: bool,
    was_plain: bool,
}

fn next_preset(current: Option<WeatherPreset>) -> Option<WeatherPreset> {
    match current {
        None => Some(WeatherPreset::Clear),
        Some(WeatherPreset::Clear) => Some(WeatherPreset::Cloud),
        Some(WeatherPreset::Cloud) => Some(WeatherPreset::Fog),
        Some(WeatherPreset::Fog) => Some(WeatherPreset::Rain),
        Some(WeatherPreset::Rain) => Some(WeatherPreset::Snow),
        Some(WeatherPreset::Snow) => Some(WeatherPreset::Storm),
        Some(WeatherPreset::Storm | WeatherPreset::Custom) => None,
    }
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<PendingWeather>()
        .init_resource::<RenderWeather>()
        .add_systems(
            Update,
            select_pending_weather
                .after(WorldMapSystems::Input)
                .after(world_runtime::capture_map_input)
                .after(crate::region_runtime::update)
                .before(world_runtime::apply_world_map_start)
                .before(crate::traffic_runtime::update_traffic)
                .before(WorldMapSystems::Display),
        )
        .add_systems(
            Update,
            publish_weather
                .after(crate::advance_simulation)
                .after(crate::control_flight)
                .after(crate::control_replay)
                .after(world_runtime::apply_world_map_start)
                .before(flightsim_render::RenderSet::Weather)
                .before(crate::publish_hud),
        );
}

#[allow(clippy::too_many_arguments)]
fn select_pending_weather(
    startup: Res<Startup>,
    mut map: ResMut<WorldMapState>,
    capture: Res<world_runtime::MapCapture>,
    mut actions: ResMut<WorldMapActions>,
    playback: Option<Res<ReplayPlayback>>,
    simulation: Option<Res<FlightSimulation>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingWeather>,
) {
    if !map.visible || !pending.was_visible {
        pending.requested = startup.weather.requested;
    }
    pending.was_visible = map.visible;
    let was_plain = pending.was_plain;
    pending.was_plain = map.new_flight_controls_active();
    let lan = startup.traffic.host.is_some() || startup.traffic.join.is_some();
    let replay = playback.is_some()
        || startup.replay.is_some()
        || simulation
            .as_ref()
            .is_some_and(|simulation| simulation.0.is_replay());
    let selectable = !replay && !lan && !startup.clouds_were_given;
    if capture.captured
        && !lan
        && selectable
        && (map.new_flight_controls_active() || (!map.visible && was_plain))
    {
        if keys.just_pressed(KeyCode::F12) && actions.new_flight_shortcuts_available {
            pending.requested = next_preset(pending.requested);
            actions.invalidate_start();
        }
        // A map-owned F12 must never also leave a LAN session or leak on Close.
        keys.clear_just_pressed(KeyCode::F12);
    }
    map.weather_note = if replay {
        let label = simulation
            .as_ref()
            .and_then(|simulation| simulation.0.jet_environment())
            .map_or_else(
                || crate::cloud_runtime::source_label(&startup),
                |environment| {
                    crate::cloud_runtime::effective_source_label(
                        Some(environment.weather),
                        Some(&startup),
                    )
                },
            );
        format!("{label} | recorded weather\nNew-flight weather selection unavailable")
    } else if lan {
        "Weather selection unavailable in LAN\nCurrent flight weather is retained".into()
    } else if startup.clouds_were_given {
        "Manual clouds | current launch settings\nRestart without manual flags to select".into()
    } else {
        let label = pending.requested.map_or("MONTHLY / LEGACY", preset_label);
        format!("Weather: {label} [F12]\nAuthored / monthly model, not live; applies on Start")
    };
}

pub(super) fn publish_weather(
    simulation: Res<FlightSimulation>,
    startup: Option<Res<Startup>>,
    playback: Option<Res<ReplayPlayback>>,
    mut weather: ResMut<RenderWeather>,
) {
    let selection = simulation.0.jet_environment().map_or_else(
        || {
            playback.as_ref().map_or_else(
                || {
                    startup
                        .as_ref()
                        .map_or(WeatherSelection::Legacy, |startup| {
                            startup.weather.selection
                        })
                },
                |playback| playback.player.recording().weather(),
            )
        },
        |environment| environment.weather,
    );
    let next = RenderWeather {
        selection,
        elapsed: simulation.0.elapsed(),
    };
    if *weather != next {
        *weather = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FlightRecorder,
        controls_runtime_tests::{control_app, tick},
        parse_arguments_from,
    };
    use bevy::ecs::system::RunSystemOnce;
    use flightsim_core::{Meters, Seconds};
    use flightsim_input::PilotKeys;
    use flightsim_world::{DemTile, HeightGrid, MemoryTileSource, Terrain, TileId};
    use std::time::Duration;

    #[test]
    fn jet_replay_publishes_recorded_weather_and_blocks_pending_selection() {
        use crate::flight_session::FlightSession;
        use flightsim_core::Radians;
        use flightsim_sim::{
            aircraft_profile::AircraftProfileV2,
            model_simulation::{JetEnvironment, JetSimulation},
            replay_v4::{JetRecorder, JetReplayPlayer},
        };
        let profile = AircraftProfileV2::parse(include_str!(
            "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
        ))
        .unwrap();
        let recorded = WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                WeatherPreset::Snow,
                Geodetic::from_degrees(0.0, 0.0, 0.0),
                919,
            )
            .unwrap(),
        );
        let environment = JetEnvironment {
            weather: recorded,
            ..Default::default()
        };
        let simulation = JetSimulation::parked(
            profile.configuration().clone(),
            Geodetic::from_degrees(0.0, 0.0, 0.0),
            Radians::ZERO,
            environment,
        )
        .unwrap();
        let recording = JetRecorder::new(&simulation).unwrap().finish();
        let player = JetReplayPlayer::new(profile.configuration().clone(), recording).unwrap();
        let mut world = World::new();
        world.insert_resource(FlightSimulation(FlightSession::replay(player)));
        let startup = Startup {
            aircraft: crate::aircraft_profile::SelectedAircraftProfile::Jet(profile),
            ..Default::default()
        };
        world.insert_resource(startup);
        world.init_resource::<RenderWeather>();
        world.init_resource::<WorldMapState>();
        world.resource_mut::<WorldMapState>().visible = true;
        world.init_resource::<world_runtime::MapCapture>();
        world.resource_mut::<world_runtime::MapCapture>().captured = true;
        world.init_resource::<WorldMapActions>();
        world
            .resource_mut::<WorldMapActions>()
            .new_flight_shortcuts_available = true;
        world.init_resource::<ButtonInput<KeyCode>>();
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F12);
        world.init_resource::<PendingWeather>();
        world.run_system_once(select_pending_weather).unwrap();
        world.run_system_once(publish_weather).unwrap();
        assert!(!world.contains_resource::<ReplayPlayback>());
        assert_eq!(world.resource::<RenderWeather>().selection, recorded);
        assert_eq!(world.resource::<RenderWeather>().elapsed, Seconds::ZERO);
        assert_eq!(world.resource::<PendingWeather>().requested, None);
        assert!(
            world
                .resource::<WorldMapState>()
                .weather_note
                .starts_with("MODELED SNOW")
        );
        assert!(
            world
                .resource::<WorldMapState>()
                .weather_note
                .contains("recorded weather")
        );
    }

    const PRESETS: [WeatherPreset; 6] = [
        WeatherPreset::Clear,
        WeatherPreset::Cloud,
        WeatherPreset::Fog,
        WeatherPreset::Rain,
        WeatherPreset::Snow,
        WeatherPreset::Storm,
    ];

    #[test]
    fn bounded_cli_rejects_ambiguous_weather_and_retains_legacy_default() {
        let (startup, _) = parse_arguments_from(std::iter::empty());
        assert_eq!(startup.weather.selection, WeatherSelection::Legacy);
        assert!(startup.weather.requested.is_none());
        for (name, preset) in ["clear", "cloud", "fog", "rain", "snow", "storm"]
            .into_iter()
            .zip(PRESETS)
        {
            for seed in ["0", "18446744073709551615"] {
                let (startup, _) = parse_arguments_from(
                    ["--weather", name, "--weather-seed", seed].map(str::to_owned),
                );
                assert!(startup.weather.error.is_none());
                assert_eq!(startup.weather.requested, Some(preset));
                assert_eq!(startup.weather.seed, seed.parse::<u64>().unwrap());
            }
        }
        for args in [
            vec!["--weather"],
            vec!["--weather", "Rain"],
            vec!["--weather", "custom"],
            vec!["--weather", "clear", "--weather", "rain"],
            vec!["--weather-seed", "0"],
            vec!["--weather", "legacy", "--weather-seed", "0"],
            vec!["--weather", "snow", "--weather-seed", "-1"],
            vec![
                "--weather",
                "snow",
                "--weather-seed",
                "18446744073709551616",
            ],
            vec![
                "--weather",
                "snow",
                "--weather-seed",
                "1",
                "--weather-seed",
                "2",
            ],
            vec!["--weather", "clear", "--replay", "missing.fsreplay"],
            vec!["--weather", "legacy", "--cloud-cover", "0"],
        ] {
            let (startup, _) = parse_arguments_from(args.iter().map(|s| (*s).to_owned()));
            assert!(startup.weather.error.is_some(), "{args:?}");
        }
        for flag in [
            "--cloud-cover",
            "--cloud-base",
            "--cloud-top",
            "--cloud-visibility",
        ] {
            let (startup, _) =
                parse_arguments_from(["--weather", "clear", flag, "1"].map(str::to_owned));
            assert!(startup.weather.error.unwrap().contains("manual cloud"));
        }
    }

    #[test]
    fn all_presets_bind_once_to_ground_not_aircraft_altitude_at_global_boundaries() {
        for (lat, lon, ground) in [
            (31.5, 35.5, -430.0),
            (0.0, 180.0, -20.0),
            (0.0, -180.0, 0.0),
            (90.0, 180.0, 10_000.0),
            (-90.0, -180.0, -1000.0),
        ] {
            for preset in PRESETS {
                let mut startup = Startup {
                    start: Geodetic::from_degrees(lat, lon, 7000.0),
                    ..default()
                };
                startup.weather.requested = Some(preset);
                startup.weather.seed = u64::MAX;
                let id = TileId::containing(0, startup.start);
                let mut source = MemoryTileSource::new();
                source.insert(
                    id,
                    DemTile::new(id.bounds(), HeightGrid::flat(3, 3, Meters(ground))),
                );
                let mut terrain = Terrain::new(Box::new(source) as crate::BoxedSource, 1024, 0..=0);
                resolve_with_terrain(&mut startup, &mut terrain).unwrap();
                let reference = Geodetic::from_degrees(lat, lon, ground);
                let expected = WeatherSelection::Modeled(
                    WeatherScenario::from_preset(preset, reference, u64::MAX).unwrap(),
                );
                assert_eq!(startup.weather.selection, expected);
                let conditions =
                    crate::recording_conditions(&startup, &world_runtime::startup_clock(&startup));
                startup.start = Geodetic::from_degrees(45.0, 10.0, 12_000.0);
                assert_eq!(startup.weather.selection, expected);
                assert_eq!(conditions.weather, expected);
            }
        }
    }

    #[test]
    fn ocean_uses_surface_datum_and_legacy_does_no_resolution() {
        let mut startup = Startup {
            start: Geodetic::from_degrees(0.0, -140.0, 9000.0),
            ..default()
        };
        startup.weather.requested = Some(WeatherPreset::Fog);
        resolve_departure(&mut startup).unwrap();
        let WeatherSelection::Modeled(scenario) = startup.weather.selection else {
            panic!("modeled")
        };
        let expected = flightsim_world::global::GlobalTerrain::bundled()
            .unwrap()
            .sample(startup.start)
            .unwrap();
        assert!(!expected.is_land);
        assert!(
            (scenario.parameters().departure_reference.altitude.get()
                - expected.surface_height.get())
            .abs()
                < 1.0
        );
        assert!(scenario.parameters().cloud.is_none());
        assert_eq!(
            scenario.parameters().fog.unwrap().bottom,
            scenario.parameters().departure_reference.altitude
        );
        startup.weather.requested = None;
        // Legacy must not inspect even an invalid source selection.
        startup.tiles = Some(std::path::PathBuf::from("does-not-exist"));
        resolve_departure(&mut startup).unwrap();
        assert_eq!(startup.weather.selection, WeatherSelection::Legacy);
    }

    fn pending_app() -> App {
        let mut app = App::new();
        app.insert_resource(Startup::default())
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_message::<bevy::input::keyboard::KeyboardFocusLost>()
            .init_resource::<WorldMapState>()
            .init_resource::<WorldMapActions>()
            .init_resource::<world_runtime::MapCapture>()
            .init_resource::<PendingWeather>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(
                Update,
                flightsim_ui::world_map::handle_world_map_input.in_set(WorldMapSystems::Input),
            )
            .add_systems(
                Update,
                world_runtime::capture_map_input.after(WorldMapSystems::Input),
            )
            .add_systems(
                Update,
                select_pending_weather
                    .after(WorldMapSystems::Input)
                    .after(world_runtime::capture_map_input),
            );
        app
    }

    fn f12(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F12);
        app.update();
    }

    #[test]
    fn map_cycles_only_pending_and_cancel_restores_actual_weather() {
        let mut app = pending_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        for preset in PRESETS {
            f12(&mut app);
            assert_eq!(
                app.world().resource::<PendingWeather>().requested,
                Some(preset)
            );
            assert_eq!(
                app.world().resource::<Startup>().weather.selection,
                WeatherSelection::Legacy
            );
            assert!(
                app.world()
                    .resource::<WorldMapState>()
                    .weather_note
                    .contains(preset_label(preset))
            );
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::F12)
            );
        }
        app.world_mut().resource_mut::<WorldMapState>().visible = false;
        f12(&mut app); // Closing frame is captured, and cannot change the flight.
        assert!(app.world().resource::<PendingWeather>().requested.is_none());
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::F12)
        );
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(app.world().resource::<PendingWeather>().requested.is_none());
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_credits();
        f12(&mut app);
        assert!(app.world().resource::<PendingWeather>().requested.is_none());
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_regions();
        f12(&mut app);
        assert!(app.world().resource::<PendingWeather>().requested.is_none());
    }

    #[test]
    fn canceling_a_modeled_preview_retains_the_actual_scenario_and_seed() {
        let mut app = pending_app();
        let selection = WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                WeatherPreset::Rain,
                Geodetic::from_degrees(31.5, 35.5, -430.0),
                42,
            )
            .unwrap(),
        );
        {
            let mut startup = app.world_mut().resource_mut::<Startup>();
            startup.weather.requested = Some(WeatherPreset::Rain);
            startup.weather.selection = selection;
            startup.weather.seed = 42;
        }
        let actual = RenderWeather {
            selection,
            elapsed: Seconds(120.0),
        };
        app.insert_resource(actual);
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        f12(&mut app);
        assert_eq!(
            app.world().resource::<PendingWeather>().requested,
            Some(WeatherPreset::Snow)
        );
        assert_eq!(*app.world().resource::<RenderWeather>(), actual);
        app.world_mut().resource_mut::<WorldMapState>().visible = false;
        f12(&mut app);
        assert_eq!(
            app.world().resource::<PendingWeather>().requested,
            Some(WeatherPreset::Rain)
        );
        assert_eq!(*app.world().resource::<RenderWeather>(), actual);
        assert_eq!(
            app.world().resource::<Startup>().weather.selection,
            selection
        );
        assert_eq!(app.world().resource::<Startup>().weather.seed, 42);
    }

    #[test]
    fn lan_map_keeps_leave_key_and_never_advertises_weather_cycle() {
        for host in [false, true] {
            let mut app = pending_app();
            let address = Some("127.0.0.1:5678".parse().unwrap());
            if host {
                app.world_mut().resource_mut::<Startup>().traffic.host = address;
            } else {
                app.world_mut().resource_mut::<Startup>().traffic.join = address;
            }
            app.world_mut().resource_mut::<WorldMapState>().visible = true;
            f12(&mut app);
            assert!(app.world().resource::<PendingWeather>().requested.is_none());
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::F12)
            );
            assert!(
                !app.world()
                    .resource::<WorldMapState>()
                    .weather_note
                    .contains("F12")
            );
            app.world_mut().resource_mut::<WorldMapState>().visible = false;
            f12(&mut app);
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::F12)
            );
        }
    }

    #[test]
    fn child_modal_close_frame_does_not_cycle_weather_or_capture_lan_leave() {
        use flightsim_ui::world_map::{RegionsButton, WorldMapButton};
        for lan in [false, true] {
            for regions in [false, true] {
                for click in [false, true] {
                    let mut app = pending_app();
                    if lan {
                        app.world_mut().resource_mut::<Startup>().traffic.join =
                            Some("127.0.0.1:5678".parse().unwrap());
                    }
                    {
                        let mut map = app.world_mut().resource_mut::<WorldMapState>();
                        if regions {
                            map.show_regions();
                        } else {
                            map.show_credits();
                        }
                    }
                    app.update();
                    if click {
                        let button = if regions {
                            WorldMapButton::Regions(RegionsButton::Close)
                        } else {
                            WorldMapButton::CloseCredits
                        };
                        app.world_mut().spawn((button, Interaction::Pressed));
                    } else {
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::Escape);
                    }
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::F12);
                    app.update();
                    assert!(
                        app.world()
                            .resource::<WorldMapState>()
                            .new_flight_controls_active()
                    );
                    assert!(
                        !app.world()
                            .resource::<WorldMapActions>()
                            .new_flight_shortcuts_available
                    );
                    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
                    assert!(app.world().resource::<PendingWeather>().requested.is_none());
                    assert_eq!(
                        app.world()
                            .resource::<ButtonInput<KeyCode>>()
                            .just_pressed(KeyCode::F12),
                        lan
                    );
                    // Ownership can return on a later fresh plain-map frame.
                    f12(&mut app);
                    assert_eq!(
                        app.world().resource::<PendingWeather>().requested,
                        (!lan).then_some(WeatherPreset::Clear)
                    );
                }
            }
        }
    }

    #[test]
    fn coordinate_commit_frame_does_not_cycle_even_when_editor_opens_in_same_batch() {
        use bevy::input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        };
        use flightsim_ui::world_map::WorldMapButton;
        for lan in [false, true] {
            for same_frame_open in [false, true] {
                for click_start in [false, true] {
                    let mut app = pending_app();
                    if lan {
                        app.world_mut().resource_mut::<Startup>().traffic.join =
                            Some("127.0.0.1:5678".parse().unwrap());
                    }
                    app.world_mut().resource_mut::<WorldMapState>().visible = true;
                    let field = app
                        .world_mut()
                        .spawn((WorldMapButton::Latitude, Interaction::Pressed))
                        .id();
                    if !same_frame_open {
                        app.update();
                        assert!(
                            !app.world()
                                .resource::<WorldMapState>()
                                .new_flight_controls_active()
                        );
                        *app.world_mut().get_mut::<Interaction>(field).unwrap() = Interaction::None;
                    }
                    if click_start {
                        app.world_mut()
                            .spawn((WorldMapButton::Start, Interaction::Pressed));
                    } else {
                        app.world_mut()
                            .write_message(KeyboardInput {
                                key_code: KeyCode::Enter,
                                logical_key: Key::Enter,
                                text: None,
                                state: ButtonState::Pressed,
                                repeat: false,
                                window: Entity::PLACEHOLDER,
                            })
                            .unwrap();
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::Enter);
                    }
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::F12);
                    app.update();
                    assert!(
                        app.world()
                            .resource::<WorldMapState>()
                            .new_flight_controls_active()
                    );
                    assert!(
                        !app.world()
                            .resource::<WorldMapActions>()
                            .new_flight_shortcuts_available
                    );
                    assert_eq!(
                        app.world().resource::<WorldMapActions>().start_at.is_some(),
                        click_start
                    );
                    assert!(app.world().resource::<PendingWeather>().requested.is_none());
                    assert_eq!(
                        app.world()
                            .resource::<ButtonInput<KeyCode>>()
                            .just_pressed(KeyCode::F12),
                        lan
                    );
                }
            }
        }
    }

    #[test]
    fn executed_clock_and_recorded_selection_reach_weather_before_render() {
        #[derive(Resource, Default)]
        struct Observed(RenderWeather);
        fn observe(weather: Res<RenderWeather>, mut observed: ResMut<Observed>) {
            observed.0 = *weather;
        }
        let mut app = control_app("light-single");
        let mut startup = Startup::default();
        startup.weather.selection = WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                WeatherPreset::Rain,
                Geodetic::from_degrees(0.0, -180.0, -400.0),
                123,
            )
            .unwrap(),
        );
        let selection = startup.weather.selection;
        app.insert_resource(startup)
            .init_resource::<RenderWeather>()
            .init_resource::<Observed>()
            .add_systems(
                Update,
                publish_weather
                    .after(crate::advance_simulation)
                    .before(flightsim_render::RenderSet::Weather),
            )
            .add_systems(Update, observe.in_set(flightsim_render::RenderSet::Weather));
        for dt in [
            Duration::from_millis(1),
            Duration::from_millis(23),
            Duration::from_millis(41),
        ] {
            tick(&mut app, dt, PilotKeys::default());
            assert_eq!(
                app.world().resource::<Observed>().0,
                RenderWeather {
                    selection,
                    elapsed: app.world().resource::<FlightSimulation>().0.elapsed()
                }
            );
        }
        let before = app.world().resource::<Observed>().0;
        app.world_mut().resource_mut::<flightsim_ui::Paused>().0 = true;
        tick(&mut app, Duration::from_secs(20), PilotKeys::default());
        assert_eq!(app.world().resource::<Observed>().0, before);
        let mut conditions = app
            .world()
            .resource::<FlightRecorder>()
            .0
            .recording()
            .conditions()
            .clone();
        conditions.weather = WeatherSelection::Modeled(
            WeatherScenario::from_preset(
                WeatherPreset::Snow,
                Geodetic::from_degrees(0.0, 0.0, 0.0),
                9,
            )
            .unwrap(),
        );
        let recorded = conditions.weather;
        app.insert_resource(ReplayPlayback::new(flightsim_sim::ReplayFile::V3(
            flightsim_sim::CurrentRecorder::new(conditions).finish(),
        )));
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_eq!(app.world().resource::<Observed>().0.selection, recorded);
        // Replay cursor elapsed is presentation budget, not the executed FDM clock.
        app.world_mut().resource_mut::<ReplayPlayback>().elapsed = Seconds(999.0);
        tick(&mut app, Duration::ZERO, PilotKeys::default());
        assert_eq!(app.world().resource::<Observed>().0.elapsed, before.elapsed);
    }
}
