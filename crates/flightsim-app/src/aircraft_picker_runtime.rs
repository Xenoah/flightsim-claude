//! Bounded, app-owned new-flight aircraft transactions. Catalog IDs identify
//! choices; the validated profile, never its display name, determines physics.
use super::*;
use flightsim_sim::weather::WeatherPreset;
use flightsim_ui::world_map::{
    WorldMapActions, WorldMapAircraftChoice, WorldMapStart, WorldMapState,
};

#[derive(Clone)]
struct AircraftChoice {
    profile: aircraft_profile::SelectedAircraftProfile,
    model: Option<String>,
    fit: ModelFit,
    sound: flightsim_audio::EngineKind,
    source: Option<String>,
}
impl AircraftChoice {
    fn current(startup: &Startup) -> Self {
        Self {
            profile: startup.aircraft.clone(),
            model: startup.model.clone(),
            fit: startup.model_fit,
            sound: startup.engine_sound,
            source: startup.aircraft_choice.clone(),
        }
    }
    fn apply(&self, startup: &mut Startup) {
        startup.aircraft = self.profile.clone();
        startup.aircraft_choice = self.source.clone();
        startup.model = self.model.clone();
        startup.model_fit = self.fit;
        startup.engine_sound = self.sound;
    }
}
struct Entry {
    view: WorldMapAircraftChoice,
    choice: Option<AircraftChoice>,
}

#[derive(Resource)]
pub(super) struct AircraftPicker {
    entries: Vec<Entry>,
    active: usize,
    was_visible: bool,
    observed_generation: u64,
    pending: Option<PendingFlight>,
}
struct PendingFlight {
    request: WorldMapStart,
    /// Exact validated profile/config, launch options and selected weather are
    /// cloned once. No later re-read can silently change the target aircraft.
    startup: Startup,
    selected_region: Option<String>,
    weather: Option<WeatherPreset>,
    conditions: conditions_runtime::PhysicalConditions,
    phase: Phase,
}
enum Phase {
    Region,
    Model {
        prepared: Box<world_runtime::PreparedWorldFlight>,
        scene: aircraft_scene::AircraftScene,
    },
    Failed,
}
impl PendingFlight {
    fn cancel(self, world: &mut World) {
        if world.resource::<WorldMapActions>().start_at == Some(self.request) {
            world.resource_mut::<WorldMapActions>().start_at = None;
        }
        if let Phase::Model { scene, .. } = self.phase {
            scene.cancel(world);
        }
    }
    fn matches(&self, world: &World) -> bool {
        let map = world.resource::<WorldMapState>();
        let actions = world.resource::<WorldMapActions>();
        let current = world.resource::<Startup>();
        let weather = if !current.clouds_were_given
            && current.traffic.host.is_none()
            && current.traffic.join.is_none()
        {
            world
                .get_resource::<weather_runtime::PendingWeather>()
                .map_or(current.weather.requested, |pending| pending.requested)
        } else {
            current.weather.requested
        };
        map.new_flight_modal_ready()
            && actions.generation == self.request.generation
            && map.aircraft_choice == self.request.aircraft_choice
            && map.selected == self.request.position
            && map.preview_month() == self.request.month
            && map.regions.selected == self.selected_region
            && weather == self.weather
            && conditions_runtime::snapshot(world, current) == self.conditions
    }
}

pub(super) fn configure(app: &mut App) {
    app.add_systems(Startup, initialize.after(super::setup));
}
pub(super) fn initialize(world: &mut World) {
    let picker = AircraftPicker::new(world.resource::<Startup>());
    let mut map = world.resource_mut::<WorldMapState>();
    map.aircraft_choices = picker
        .entries
        .iter()
        .map(|entry| entry.view.clone())
        .collect();
    map.aircraft_choice = picker.active;
    world.insert_resource(picker);
}
impl AircraftPicker {
    fn new(startup: &Startup) -> Self {
        let mut entries = Vec::new();
        // Keep the launch profile as a distinct choice, including custom
        // dynamics and explicit model/no-model/sound overrides. Do not dedupe by
        // profile ID or label: neither establishes complete physical identity.
        if !cfg!(feature = "commercial-staging") {
            entries.push(Entry {
                view: WorldMapAircraftChoice {
                    label: format!("Launch: {}", startup.aircraft.name()),
                    note: "Launch profile with its model and sound choices".into(),
                    available: true,
                },
                choice: Some(AircraftChoice::current(startup)),
            });
        }
        for (file, label, note) in [
            (
                "swift_sport.json",
                "Swift Sport",
                "Original low wing; legacy propeller dynamics",
            ),
            (
                "meadow_trainer.json",
                "Meadow Trainer",
                "Original high wing; Light Single dynamics",
            ),
            (
                "kestrel_jet_trainer.json",
                "Kestrel Jet Trainer",
                "Experimental dry jet; authored Mach 0-0.35 range",
            ),
        ] {
            if cfg!(feature = "commercial-staging") && file != "swift_sport.json" {
                continue;
            }
            let choice = (|| {
                let assets = startup
                    .assets
                    .as_ref()
                    .ok_or("Assets directory is unavailable")?;
                let path = assets.join("aircraft").join(file);
                let profile = aircraft_profile::SelectedAircraftProfile::load(
                    path.to_str().ok_or("Aircraft profile path is not UTF-8")?,
                )?;
                let expected_id = file.trim_end_matches(".json").replace('_', "-");
                let expected_model = format!("aircraft/{}.glb", file.trim_end_matches(".json"));
                if profile.id() != expected_id
                    || profile.model_path() != expected_model
                    || profile.is_jet() != (file == "kestrel_jet_trainer.json")
                {
                    return Err("Installed profile does not match this aircraft preset; use an explicit CLI profile".into());
                }
                distribution::validate_model(Some(profile.model_path()), Some(assets), true)?;
                Ok::<_, String>(AircraftChoice {
                    model: Some(profile.model_path().to_owned()),
                    fit: profile.model_fit(),
                    sound: profile.engine_kind(),
                    profile,
                    source: Some(path.display().to_string()),
                })
            })();
            let (choice, note) = match choice {
                Ok(choice) => (Some(choice), note.to_owned()),
                Err(error) => (None, format!("Unavailable: {error}")),
            };
            entries.push(Entry {
                view: WorldMapAircraftChoice {
                    label: label.into(),
                    note,
                    available: choice.is_some(),
                },
                choice,
            });
        }
        Self {
            entries,
            active: 0,
            was_visible: false,
            observed_generation: 0,
            pending: None,
        }
    }
    pub fn target_is_jet(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .and_then(|e| e.choice.as_ref())
            .is_some_and(|choice| choice.profile.is_jet())
    }
    pub fn available(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.choice.is_some())
    }
    pub fn preparing(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| !matches!(pending.phase, Phase::Failed))
    }
}

/// Called by the existing exclusive map Start owner, after all modal input and
/// weather/region choices, and before controls, simulation and presentation.
pub(super) fn apply(world: &mut World) {
    let mut picker = world
        .remove_resource::<AircraftPicker>()
        .expect("picker configured");
    let replay = world.contains_resource::<ReplayPlayback>()
        || world.resource::<Startup>().replay.is_some()
        || world
            .get_resource::<FlightSimulation>()
            .is_some_and(|s| s.0.is_replay());
    let active_label = world.resource::<Startup>().aircraft.name().to_owned();
    {
        let mut map = world.resource_mut::<WorldMapState>();
        map.aircraft_selection_enabled = !replay;
        // UI owns the Current / Locked labels; keep this value as the name.
        map.active_aircraft = active_label;
        if picker.was_visible && !map.visible {
            map.aircraft_choice = picker.active;
        }
        picker.was_visible = map.visible;
    }
    if picker
        .pending
        .as_ref()
        .is_some_and(|pending| replay || !pending.matches(world))
    {
        picker
            .pending
            .take()
            .expect("pending checked")
            .cancel(world);
    }
    let generation = world.resource::<WorldMapActions>().generation;
    if generation != picker.observed_generation {
        picker.observed_generation = generation;
        world_runtime::clear_navigation_error(world);
    }
    let map = world.resource::<WorldMapState>();
    if replay || !map.visible || map.regions.visible {
        world.insert_resource(picker);
        return;
    }
    let request = world.resource::<WorldMapActions>().start_at;
    if let Some(request) = request
        && picker
            .pending
            .as_ref()
            .is_none_or(|pending| pending.request != request)
    {
        if let Some(pending) = picker.pending.take() {
            pending.cancel(world);
        }
        let entry = picker.entries.get(request.aircraft_choice);
        let choice = entry.and_then(|entry| entry.choice.clone());
        if let Some(choice) = choice {
            let mut startup = world.resource::<Startup>().clone();
            choice.apply(&mut startup);
            world_runtime::apply_pending_weather(world, &mut startup);
            picker.pending = Some(PendingFlight {
                request,
                selected_region: world.resource::<WorldMapState>().regions.selected.clone(),
                weather: startup.weather.requested,
                conditions: conditions_runtime::PhysicalConditions::from_startup(&startup),
                startup,
                phase: Phase::Region,
            });
        } else {
            let reason = entry.map_or("Unknown aircraft selection", |entry| {
                entry.view.note.as_str()
            });
            world_runtime::navigation_error(world, reason.to_owned());
        }
    }
    if let Some(mut pending) = picker.pending.take() {
        if !pending.matches(world) {
            pending.cancel(world);
            world.insert_resource(picker);
            return;
        }
        let error = if pending.startup.aircraft.is_jet() {
            region_runtime::jet_start_error(world).map(str::to_owned)
        } else if !pending.startup.world.global_terrain {
            Some("Preview only: global terrain is off\nRestart with --global-terrain on".into())
        } else {
            None
        };
        if !matches!(pending.phase, Phase::Failed)
            && let Some(error) = error
        {
            fail(world, &mut pending, error);
        }
        if matches!(pending.phase, Phase::Region) {
            let resolved = if pending.startup.aircraft.is_jet() {
                Some((pending.request, None))
            } else {
                region_runtime::take_start_for_target(world, false)
            };
            if let Some((request, package)) = resolved {
                if request != pending.request {
                    fail(
                        world,
                        &mut pending,
                        "Region result belongs to an earlier new-flight request; press Start again"
                            .into(),
                    );
                } else {
                    match world_runtime::prepare_world_map_flight(
                        pending.startup.clone(),
                        request,
                        package,
                    )
                    .and_then(|prepared| {
                        aircraft_scene::stage(world, &prepared).map(|scene| (prepared, scene))
                    }) {
                        Ok((prepared, scene)) => {
                            // Admission is one-shot; keeping choices editable is
                            // not permission to silently restage after an edit.
                            world.resource_mut::<WorldMapActions>().start_at = None;
                            pending.phase = Phase::Model {
                                prepared: Box::new(prepared),
                                scene,
                            };
                        }
                        Err(error) => fail(world, &mut pending, error),
                    }
                }
            } else if !region_runtime::start_is_pending(world) {
                let error = world.resource::<WorldMapState>().regions.error.clone();
                fail(
                    world,
                    &mut pending,
                    if error.is_empty() {
                        "Regional source changed; press Start again".into()
                    } else {
                        error
                    },
                );
            }
        }
        let outcome = if let Phase::Model { scene, .. } = &mut pending.phase {
            scene.poll(world)
        } else {
            Ok(false)
        };
        match outcome {
            Ok(true) => {
                if let Phase::Model { prepared, scene } = pending.phase {
                    // No fallible operation follows: publish only the complete
                    // prepared session, exact scene, controls and synth family.
                    scene.commit(world, &prepared.startup, &prepared.simulation);
                    flightsim_audio::replace_sound_source(world, prepared.startup.engine_sound);
                    world_runtime::commit_world_map_flight(world, *prepared);
                    picker.active = pending.request.aircraft_choice;
                    world.resource_mut::<WorldMapState>().aircraft_choice = picker.active;
                }
            }
            Ok(false) => picker.pending = Some(pending),
            Err(error) => {
                fail(world, &mut pending, error);
                picker.pending = Some(pending);
            }
        }
    }
    world.insert_resource(picker);
}

fn fail(world: &mut World, pending: &mut PendingFlight, error: String) {
    let previous = std::mem::replace(&mut pending.phase, Phase::Failed);
    if let Phase::Model { scene, .. } = previous {
        scene.cancel(world);
    }
    // A failed attempt stays selected and retryable, including destination,
    // weather and regional choice. Only an explicit new Start tries again.
    world.resource_mut::<WorldMapActions>().start_at = Some(pending.request);
    world_runtime::navigation_error(world, error);
}

#[cfg(test)]
#[path = "aircraft_picker_tests.rs"]
mod tests;
