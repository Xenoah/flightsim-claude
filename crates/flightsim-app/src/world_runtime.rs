//! World terrain, regional climate and modal-map application integration.
//!
//! Map actions create an explicit new flight. Previewing a different month never
//! changes a live/replayed trajectory, and legacy replay files keep legacy ISA.

use super::*;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use flightsim_core::{Kelvin, Knots};
use flightsim_render::biome::{SurfaceAppearance, linear_surface_color, surface_color};
use flightsim_ui::world_map::{
    self, WorldMapActions, WorldMapLayer, WorldMapRaster, WorldMapState, WorldMapSystems,
};
use flightsim_world::climate::{ClimateDate, GlobalClimate};
use flightsim_world::global::GlobalTerrain;

const LEGACY_MAP_NOTICE: &str =
    "Preview only: global terrain is off\nRestart with --global-terrain on";

#[derive(Debug, Clone)]
pub(super) struct WorldOptions {
    pub global_terrain: bool,
    pub climate_enabled: bool,
    pub climate_date: ClimateDate,
    pub civil_date: (i32, u8, u8),
    pub fly_height: Option<Meters>,
    pub map_open: bool,
    pub map_credits: bool,
    pub map_layer: WorldMapLayer,
}

impl Default for WorldOptions {
    fn default() -> Self {
        Self {
            global_terrain: true,
            climate_enabled: true,
            climate_date: ClimateDate::from_gregorian(2026, 6, 21, Seconds(43_200.0))
                .expect("valid fixed default date"),
            civil_date: (2026, 6, 21),
            fly_height: None,
            map_open: false,
            map_credits: false,
            map_layer: WorldMapLayer::Terrain,
        }
    }
}

impl WorldOptions {
    pub fn climate_date(&self) -> Option<ClimateDate> {
        self.climate_enabled.then_some(self.climate_date)
    }
}

pub(super) fn parse_date(text: &str) -> Option<((i32, u8, u8), ClimateDate)> {
    if text.len() != 10
        || !text.is_ascii()
        || text.as_bytes()[4] != b'-'
        || text.as_bytes()[7] != b'-'
    {
        return None;
    }
    let mut pieces = text.split('-');
    let year = pieces.next()?.parse::<i32>().ok()?;
    let month = pieces.next()?.parse::<u8>().ok()?;
    let day = pieces.next()?.parse::<u8>().ok()?;
    if pieces.next().is_some() || !(1900..=2100).contains(&year) {
        return None;
    }
    let date = ClimateDate::from_gregorian(year, month, day, Seconds(43_200.0))?;
    Some(((year, month, day), date))
}

/// The global baseline is consulted only after every permitted real ancestor.
/// Legacy replay keeps the historical regional-level search range.
pub(super) fn terrain_levels(startup: &Startup) -> core::ops::RangeInclusive<u8> {
    if startup.aircraft.is_jet() {
        // Match JetSimulation's pinned physical sampler. Render-only LOD still
        // uses its own selector; a visual-detail option cannot alter jet ground.
        return 8..=12;
    }
    (if startup.world.global_terrain {
        0
    } else {
        startup.min_level
    })..=startup.max_level
}

pub(super) fn startup_clock(startup: &Startup) -> flightsim_render::TimeOfDay {
    if startup.start_hour.is_none()
        && !startup.start_was_explicit
        && startup.world.civil_date == (2026, 6, 21)
    {
        return flightsim_render::TimeOfDay::default();
    }
    let (year, month, day) = startup.world.civil_date;
    let (hour, minute) = startup.start_hour.unwrap_or((9, 30));
    flightsim_render::TimeOfDay::at_local_mean_solar_time(
        flightsim_render::UtcDateTime::new(year, month, day, hour, minute, 0.0),
        startup.start.longitude,
    )
}

#[derive(Resource, Debug)]
pub(super) struct WorldRuntime {
    pub global: GlobalTerrain,
    pub climate: GlobalClimate,
    last_navigation_error: Option<String>,
}

impl WorldRuntime {
    pub fn new(_startup: &Startup) -> Result<Self, String> {
        Ok(Self {
            global: GlobalTerrain::bundled().map_err(|error| error.to_string())?,
            climate: GlobalClimate::bundled().map_err(|error| error.to_string())?,
            last_navigation_error: None,
        })
    }

    fn appearance(
        &self,
        position: Geodetic,
        slope: Radians,
        date: ClimateDate,
    ) -> SurfaceAppearance {
        let terrain = self
            .global
            .sample(position)
            .expect("validated geographic mesh/map coordinate");
        let climate = self.climate.sample(position, date);
        SurfaceAppearance {
            land_fraction: terrain.land_fraction,
            elevation_msl: Meters(position.altitude.get() - terrain.geoid_undulation.get()),
            slope,
            temperature: climate.temperature,
            precipitation: climate.precipitation_rate,
            snow_fraction: climate.snow_fraction,
        }
    }

    pub fn surface_color(
        &self,
        position: Geodetic,
        slope: Radians,
        date: Option<ClimateDate>,
    ) -> [f32; 4] {
        if let Some(date) = date {
            return linear_surface_color(self.appearance(position, slope, date));
        }
        let Some(terrain) = self.global.sample(position) else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        #[allow(
            clippy::cast_possible_truncation,
            reason = "metre heights and radian slopes are finite and bounded"
        )]
        let mut color = flightsim_render::terrain_color(
            (position.altitude.get() - terrain.geoid_undulation.get()) as f32,
            slope.get() as f32,
        );
        for (channel, ocean) in color[..3].iter_mut().zip([0.035, 0.20, 0.32]) {
            #[allow(clippy::cast_possible_truncation, reason = "land fraction is in [0,1]")]
            {
                *channel = ocean + (*channel - ocean) * terrain.land_fraction as f32;
            }
            *channel = flightsim_render::srgb_to_linear(*channel);
        }
        color
    }

    fn map_color(&self, position: Geodetic, date: ClimateDate, layer: WorldMapLayer) -> [u8; 4] {
        let terrain = self
            .global
            .sample(position)
            .expect("map pixel coordinates are valid");
        let surface = Geodetic::new(
            position.latitude,
            position.longitude,
            terrain.surface_height,
        );
        let rgba = match layer {
            WorldMapLayer::Terrain => {
                let mut color = surface_color(self.appearance(surface, Radians::ZERO, date));
                // Coarse, north-west-lit relief cue. The map is still a data
                // visualization, not imagery. Queries stay bounded per pixel.
                let delta = Degrees(0.2).to_radians().get();
                let north = Geodetic::new(
                    Radians((position.latitude.get() + delta).min(core::f64::consts::FRAC_PI_2)),
                    position.longitude,
                    Meters::ZERO,
                );
                let east = Geodetic::new(
                    position.latitude,
                    Radians(position.longitude.get() + delta),
                    Meters::ZERO,
                );
                let h_n = self
                    .global
                    .sample(north)
                    .map_or(terrain.elevation_msl.get(), |v| v.elevation_msl.get());
                let h_e = self
                    .global
                    .sample(east)
                    .map_or(terrain.elevation_msl.get(), |v| v.elevation_msl.get());
                let shade = (1.0 + (h_n - h_e) / 2500.0).clamp(0.65, 1.18);
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "bounded visual color shading"
                )]
                for channel in &mut color[..3] {
                    *channel = (*channel * shade as f32).clamp(0.0, 1.0);
                }
                color
            }
            WorldMapLayer::Climate => temperature_color(
                self.climate.sample(surface, date).temperature,
                terrain.land_fraction,
            ),
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped RGBA byte conversion"
        )]
        rgba.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

fn temperature_color(temperature: Kelvin, land_fraction: f64) -> [f32; 4] {
    let cold = [0.10, 0.30, 0.66];
    let temperate = [0.95, 0.91, 0.72];
    let hot = [0.81, 0.20, 0.09];
    let celsius = temperature.to_celsius();
    let (a, b, t) = if celsius < 10.0 {
        (cold, temperate, ((celsius + 45.0) / 55.0).clamp(0.0, 1.0))
    } else {
        (temperate, hot, ((celsius - 10.0) / 35.0).clamp(0.0, 1.0))
    };
    let saturation = 0.48 + 0.52 * land_fraction.clamp(0.0, 1.0);
    let mut color = [0.0_f32; 4];
    for index in 0..3 {
        #[allow(clippy::cast_possible_truncation, reason = "bounded color channels")]
        {
            color[index] = ((a[index] + (b[index] - a[index]) * t) * saturation) as f32;
        }
    }
    color[3] = 1.0;
    color
}

/// Brief in-flight credit; full dataset versions, source URLs and licences are
/// in ATTRIBUTION.md and the map. No source is claimed when disabled.
pub(super) fn data_attribution(startup: &Startup) -> DataAttribution {
    let mut sources = Vec::new();
    if startup.world.global_terrain {
        sources.push("Terrain: NOAA/Natural Earth/Copernicus GLO-90");
    }
    if startup.world.climate_enabled {
        sources.push("Climate: NOAA PSL 1991-2020");
    }
    if startup.airport_enabled
        && matches!(startup.runway_source, RunwaySource::OpenStreetMap { .. })
    {
        sources.push(OSM_AIRPORT_ATTRIBUTION);
    }
    if matches!(
        startup.weather.selection,
        flightsim_sim::weather::WeatherSelection::Modeled(_)
    ) {
        sources.push("Weather: authored model, not live");
    }
    if startup.active_region.is_some() {
        sources.push("Region: recording/export unavailable; credits in M > Regions");
    }
    DataAttribution::new(sources.join(" | "))
}

pub(super) fn data_attribution_with_scenery(startup: &Startup, scenery: &str) -> DataAttribution {
    let mut sources = Vec::new();
    if startup.world.global_terrain {
        sources.push("Terrain: NOAA/NE/Copernicus");
    }
    if startup.world.climate_enabled {
        sources.push("Climate: NOAA normals");
    }
    sources.push(scenery);
    if startup.airport_enabled
        && matches!(startup.runway_source, RunwaySource::OpenStreetMap { .. })
        && !scenery.contains("OpenStreetMap")
    {
        sources.push(OSM_AIRPORT_ATTRIBUTION);
    }
    if matches!(
        startup.weather.selection,
        flightsim_sim::weather::WeatherSelection::Modeled(_)
    ) {
        sources.push("Weather: authored model, not live");
    }
    if startup.active_region.is_some() {
        sources.push("Region: recording/export unavailable; credits in M > Regions");
    }
    DataAttribution::new(sources.join(" | "))
}

/// Independent capture gate preserves an already-paused flight and freezes the
/// closing frame so Esc/C/arrows cannot leak through the modal.
#[derive(Resource, Debug, Default)]
pub(super) struct MapCapture {
    pub captured: bool,
    previously_visible: bool,
}

pub(super) fn flight_controls_active(capture: Res<MapCapture>) -> bool {
    !capture.captured
}

pub(super) fn capture_map_input(map: Res<WorldMapState>, mut capture: ResMut<MapCapture>) {
    capture.captured = map.visible || capture.previously_visible;
    capture.previously_visible = map.visible;
}

/// The application-owned flight view. Other/debug cameras are not suspended.
#[derive(Component, Debug)]
pub(super) struct FlightCamera;

#[derive(Component, Debug)]
struct WorldMapCamera;

#[derive(Resource, Debug)]
struct WorldMapView {
    flight_camera: Entity,
    map_camera: Entity,
    suspended_flight_active: Option<bool>,
}

/// Run after offscreen setup so the map and flight share the actual destination
/// (native window or screenshot image), without making every HUD a map child.
fn setup_world_map_camera(
    mut commands: Commands,
    flight_cameras: Query<(Entity, &Camera, &RenderTarget), With<FlightCamera>>,
    roots: Query<Entity, With<world_map::WorldMapRoot>>,
) {
    let Ok((flight_camera, flight, target)) = flight_cameras.single() else {
        return;
    };
    let map_camera = commands
        .spawn((
            Camera2d,
            Camera {
                is_active: false,
                order: flight.order.saturating_add(1),
                viewport: flight.viewport.clone(),
                ..default()
            },
            target.clone(),
            Msaa::Off,
            RenderLayers::none(),
            WorldMapCamera,
            Name::new("world map camera"),
        ))
        .id();
    for root in &roots {
        commands.entity(root).insert(UiTargetCamera(map_camera));
    }
    commands.insert_resource(WorldMapView {
        flight_camera,
        map_camera,
        suspended_flight_active: None,
    });
}

/// Opaque UI does not stop the underlying 3D render pass. Suspend that view
/// while the map is open, then restore its exact previous activity on close.
/// Model loading/fitting and world state continue to use their normal systems.
fn sync_world_map_camera(
    map: Res<WorldMapState>,
    view: Option<ResMut<WorldMapView>>,
    mut cameras: Query<&mut Camera>,
) {
    let Some(mut view) = view else {
        return;
    };
    let Ok([mut flight_camera, mut map_camera]) =
        cameras.get_many_mut([view.flight_camera, view.map_camera])
    else {
        return;
    };
    map_camera.is_active = map.visible;
    if map.visible {
        if view.suspended_flight_active.is_none() {
            view.suspended_flight_active = Some(flight_camera.is_active);
        }
        flight_camera.is_active = false;
    } else if let Some(was_active) = view.suspended_flight_active.take() {
        flight_camera.is_active = was_active;
    }
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<WorldMapState>()
        .init_resource::<WorldMapActions>()
        .init_resource::<WorldMapRaster>()
        .init_resource::<MapCapture>()
        .configure_sets(
            Update,
            (
                WorldMapSystems::Input.before(flightsim_input::InputSystems::Sample),
                WorldMapSystems::Display.after(publish_world_map),
            ),
        )
        .configure_sets(
            Update,
            flightsim_input::InputSystems::Sample.run_if(flight_controls_active),
        )
        .configure_sets(Update, RenderSet::Sun.run_if(flight_controls_active))
        .add_systems(
            Startup,
            (initialize_map, world_map::spawn_world_map)
                .chain()
                .after(super::setup),
        )
        .add_systems(
            PostStartup,
            setup_world_map_camera.after(super::screen_capture::setup_offscreen_target),
        )
        .add_systems(
            Update,
            world_map::handle_world_map_input.in_set(WorldMapSystems::Input),
        )
        .add_systems(
            Update,
            sync_world_map_camera
                .after(apply_world_map_start)
                .before(RenderSet::Weather),
        )
        .add_systems(
            Update,
            capture_map_input
                .after(WorldMapSystems::Input)
                .before(flightsim_input::InputSystems::Sample)
                .before(super::advance_simulation)
                .before(super::control_flight)
                .before(super::control_replay)
                .before(super::adjust_time_rate)
                .before(super::toggle_tutorial)
                .before(RenderSet::Sun),
        )
        .add_systems(
            Update,
            apply_world_map_start
                .after(capture_map_input)
                .before(super::advance_simulation)
                .before(super::control_flight)
                .before(super::control_replay),
        )
        .add_systems(
            Update,
            publish_world_map
                .after(super::advance_simulation)
                .after(apply_world_map_start),
        )
        .add_systems(
            Update,
            world_map::update_world_map.in_set(WorldMapSystems::Display),
        )
        .add_systems(
            Update,
            sync_climate_clouds
                .after(super::advance_simulation)
                .before(RenderSet::Weather),
        );
}

fn initialize_map(startup: Res<Startup>, mut map: ResMut<WorldMapState>) {
    map.visible = startup.world.map_open;
    map.layer = startup.world.map_layer;
    map.select(startup.start, "Departure");
    // Replay restores its exact climate phase independently of the solar clock.
    // Start the preview in that month rather than the default CLI civil date.
    map.month = startup
        .world
        .climate_date()
        .map_or(startup.world.civil_date.1, ClimateDate::month);
    map.navigation_enabled =
        startup.replay.is_none() && (startup.world.global_terrain || startup.aircraft.is_jet());
    if startup.replay.is_none() && !startup.world.global_terrain && !startup.aircraft.is_jet() {
        map.navigation_note = LEGACY_MAP_NOTICE.into();
    }
    refresh_map_credits(&startup, &mut map);
    if startup.world.map_credits {
        map.show_credits();
    }
}

fn refresh_map_credits(startup: &Startup, map: &mut WorldMapState) {
    // Preserve full UTF-8 legal notices in the distributed file. The default
    // Bevy font needs ASCII display spellings for copyright/trademark symbols.
    map.source_credits = include_str!("../../../docs/data/NOTICE-GLOBAL-TERRAIN.txt")
        .replace('©', "(c)")
        .replace('™', "(TM)");
    if startup.airport_enabled
        && matches!(startup.runway_source, RunwaySource::OpenStreetMap { .. })
    {
        map.source_credits.push_str("\nAIRPORT DATA\n(c) OpenStreetMap contributors, ODbL1.0\nhttps://www.openstreetmap.org/copyright\n");
    }
    if let Some(package) = &startup.active_region {
        map.source_credits.push_str("\nACTIVE REGIONAL TERRAIN\n");
        map.source_credits
            .push_str(&region_runtime::credits(package.manifest()));
    }
}

pub(super) fn initial_controls(startup: &Startup) -> PilotControls {
    if startup.aircraft.is_jet() {
        return startup
            .aircraft
            .pilot_controls(startup.approach.is_some() || startup.world.fly_height.is_some());
    }
    let mut controls = startup.aircraft.pilot_controls(startup.approach.is_some());
    if startup.world.fly_height.is_some() {
        controls = startup.aircraft.pilot_controls(false);
        controls.throttle.set_absolute(0.65);
        controls.flaps.set_absolute(0.0);
    }
    controls
}

/// Safe initial clearance and authored attitude/speed are initialization, not
/// trim or an autopilot. Legacy starts retain their density-adjusted speed;
/// jet starts use the exact profile hints and need continued pilot input.
pub(super) fn airborne_state(
    startup: &Startup,
    terrain: &mut Terrain<BoxedSource>,
    height: Meters,
) -> flightsim_fdm::RigidBodyState {
    let ground = GroundSampler::default().sample(terrain, startup.start);
    let position = Geodetic::new(
        startup.start.latitude,
        startup.start.longitude,
        Meters(ground.elevation.get() + height.get()),
    );
    let direction = Attitude::new(Radians::ZERO, Radians::ZERO, startup.heading).to_quaternion()
        * bevy::math::DVec3::X;
    if let Some(profile) = startup.aircraft.jet() {
        let controls = profile.controls();
        return flightsim_fdm::RigidBodyState::from_geodetic(
            position,
            Attitude::new(
                Radians::ZERO,
                Radians(controls.approach_pitch_rad.get()),
                startup.heading,
            ),
            Ned(direction * controls.approach_speed_mps.get() + startup.wind.to_ned().0),
        );
    }
    let atmosphere =
        flightsim_sim::climate_atmosphere_sample(position, startup.world.climate_date())
            .expect("world climate data validated before initialization");
    let density_ratio = atmosphere.density_ratio().max(0.15);
    let speed = Knots(90.0).to_meters_per_second().get() / density_ratio.sqrt();
    flightsim_fdm::RigidBodyState::from_geodetic(
        position,
        Attitude::new(Radians::ZERO, Degrees(2.0).to_radians(), startup.heading),
        Ned(direction * speed + startup.wind.to_ned().0),
    )
}

pub(super) const TOWER_CLEARANCE: Meters = Meters(25.0);

/// Offset through core's ECEF local frame so both poles and the dateline remain
/// valid. The tower is supported by terrain at its own location, not departure.
pub(super) fn tower_anchor(terrain: &mut Terrain<BoxedSource>, start: Geodetic) -> Geodetic {
    let point = LocalFrame::new(start)
        .ned_to_ecef_position(Ned::new(-200.0, 200.0, 0.0))
        .to_geodetic();
    let ground = GroundSampler::default().sample(terrain, point).elevation;
    Geodetic::new(
        point.latitude,
        point.longitude,
        Meters(ground.get() + TOWER_CLEARANCE.get()),
    )
}

/// Atomically replace one flight after all new state/data have been validated.
/// An exclusive system keeps the restart transaction independent of Bevy's
/// deferred Commands and prevents one stale frame of camera/recording state.
pub(super) fn apply_world_map_start(world: &mut World) {
    let jet = world.resource::<Startup>().aircraft.is_jet();
    let (request, package) = if jet {
        // A jet start must remain completely retryable until every candidate
        // component has passed validation. In particular, take_start may consume
        // a request or launch regional inspection, so never call it on this path.
        let Some(request) = world.resource::<WorldMapActions>().start_at else {
            return;
        };
        if let Some(error) = region_runtime::jet_start_error(world) {
            navigation_error(world, error.into());
            return;
        }
        if world.resource::<WorldMapState>().regions.visible {
            return;
        }
        (request, None)
    } else {
        let Some(start) = region_runtime::take_start(world) else {
            return;
        };
        start
    };
    if world.contains_resource::<ReplayPlayback>()
        || world.resource::<Startup>().replay.is_some()
        || world
            .get_resource::<FlightSimulation>()
            .is_some_and(|simulation| simulation.0.is_replay())
    {
        return;
    }
    if let Some(mut runtime) = world.get_resource_mut::<WorldRuntime>() {
        runtime.last_navigation_error = None;
    }
    if world_map::map_point_from_geodetic(request.position).is_none() {
        return;
    }
    let Some(date) = ClimateDate::from_month(request.month) else {
        return;
    };
    let mut startup = world.resource::<Startup>().clone();
    // Airport surfaces were draped once from this session's terrain source.
    // Never silently enable a different supporting surface beneath them. The
    // explicitly disabled legacy mode may preview the map, but not relocate.
    if !startup.world.global_terrain && !jet {
        let mut map = world.resource_mut::<WorldMapState>();
        map.navigation_enabled = false;
        map.navigation_note = LEGACY_MAP_NOTICE.into();
        return;
    }
    let source_changed = startup.active_region.as_ref().map(|p| p.identity())
        != package.as_ref().map(|p| p.identity());
    startup.active_region = package;
    if source_changed {
        startup.airport_enabled = false;
        startup.scenery = None;
    }
    startup.world.climate_enabled = true;
    startup.world.climate_date = date;
    startup.world.civil_date = (2026, request.month, 15);
    startup.world.fly_height = Some(Meters(1000.0));
    startup.world.map_open = false;
    startup.start = request.position;
    startup.start_was_explicit = true;
    startup.approach = None;
    startup.drop_height = None;
    let mut terrain = Terrain::new(
        make_source(&startup),
        64 * 1024 * 1024,
        terrain_levels(&startup),
    );
    if !startup.clouds_were_given
        && startup.traffic.host.is_none()
        && startup.traffic.join.is_none()
        && let Some(pending) = world.get_resource::<super::weather_runtime::PendingWeather>()
    {
        startup.weather.requested = pending.requested;
    }
    if let Err(error) = super::weather_runtime::resolve_with_terrain(&mut startup, &mut terrain) {
        navigation_error(world, error);
        return;
    }
    let state = airborne_state(&startup, &mut terrain, Meters(1000.0));
    let valid_state = if jet {
        crate::flight_session::validate_jet_state(&state)
    } else {
        replay_runtime::validate_replay_state(&state).map_err(str::to_owned)
    };
    if let Err(error) = valid_state {
        navigation_error(world, format!("Cannot start flight: {error}"));
        return;
    }
    let tower = tower_anchor(&mut terrain, startup.start);
    let mut clock = startup_clock(&startup);
    clock.rate = flightsim_render::TimeRate(startup.time_rate);
    let (simulation, recorder) = if jet {
        let session = match crate::flight_session::FlightSession::prepare_jet(
            &startup,
            &clock,
            StartCondition::InFlight(state),
        ) {
            Ok(session) => session,
            Err(error) => {
                navigation_error(world, format!("Cannot start jet flight: {error}"));
                return;
            }
        };
        (session, None)
    } else {
        let mut simulation = Simulation::from_state(
            startup.aircraft.configuration(),
            state,
            terrain,
            GroundSampler::default(),
        );
        if let Err(error) = simulation.set_climate(Some(date)) {
            navigation_error(world, format!("Cannot start climate: {error}"));
            return;
        }
        simulation.set_wind(startup.wind);
        simulation.set_turbulence(startup.turbulence);
        let recorder = flightsim_sim::CurrentRecorder::new(recording_conditions(&startup, &clock));
        (simulation.into(), Some(recorder))
    };
    let controls = initial_controls(&startup);
    let source = make_source(&startup);
    if jet {
        world.resource_mut::<WorldMapActions>().start_at.take();
    }
    if let Some(mut traffic) = world.get_resource_mut::<traffic_runtime::TrafficRuntime>() {
        traffic.restart_synthetic_at(state.geodetic());
    }
    world.insert_resource(FlightSimulation(simulation));
    if jet && let Some(mut reset) = world.get_resource_mut::<JetHudReset>() {
        reset.0 = true;
    }
    world.insert_resource(StartCondition::InFlight(state));
    if let Some(recorder) = recorder {
        world.insert_resource(FlightRecorder(recorder));
    } else {
        world.remove_resource::<FlightRecorder>();
    }
    world.insert_resource(controls);
    world.insert_resource(SampledPilotInput::default());
    world.insert_resource(clock);
    world.insert_resource(TowerViewAnchor(tower));
    world.insert_resource(flightsim_ui::Paused(false));
    world.insert_resource(flightsim_ui::TutorialState::default());
    world.insert_resource(flightsim_ui::TutorialVisibility(false));
    world.insert_resource(flightsim_ui::LandingReportState::default());
    world.remove_resource::<flightsim_ui::LandingReport>();
    world.resource_mut::<CameraRig>().reset();
    if let Some(sound) = world.get_resource::<flightsim_audio::SoundBridge>() {
        sound.0.request_reset();
    }
    {
        let mut query =
            world.query_filtered::<(&mut WorldPosition, &mut WorldOrientation), With<Aircraft>>();
        for (mut position, mut orientation) in query.iter_mut(world) {
            position.0 = state.position;
            orientation.0 = state.orientation;
        }
    }
    scenery_runtime::clear(world);
    if source_changed {
        region_runtime::clear_airport(world);
        if let Some(mut scenery) = world.get_resource_mut::<scenery_runtime::SceneryRuntime>() {
            scenery.suppress_after_source_change();
        }
    }
    let mut tiles = world.remove_resource::<TerrainTiles>().unwrap_or_default();
    // Cancel an in-flight stitching transaction too: it owns hidden bridges
    // and may retain the old visible mesh for a same-ID source replacement.
    for (entity, mesh) in tiles.drain_all() {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
        world.resource_mut::<Assets<Mesh>>().remove(&mesh);
    }
    world.insert_resource(tiles);
    {
        let mut streaming = world.resource_mut::<TerrainStreaming>();
        streaming.source = source;
        streaming.cache.clear();
        streaming.live = flightsim_render::TerrainSelectionState::default();
    }
    let mut map = world.resource_mut::<WorldMapState>();
    map.visible = false;
    map.regions.visible = false;
    refresh_map_credits(&startup, &mut map);
    map.aircraft = Some(state.geodetic());
    info!(
        "world flight: {:.5},{:.5}, 1000 m AGL, month {}; NOAA reanalysis climatology, not live weather",
        request.position.latitude_degrees(),
        request.position.longitude_degrees(),
        request.month
    );
    let scenery_credit = world
        .get_resource::<scenery_runtime::SceneryRuntime>()
        .and_then(scenery_runtime::SceneryRuntime::attribution);
    world.insert_resource(scenery_credit.map_or_else(
        || data_attribution(&startup),
        |credit| data_attribution_with_scenery(&startup, credit),
    ));
    world.insert_resource(startup);
}

fn navigation_error(world: &mut World, error: String) {
    if let Some(mut runtime) = world.get_resource_mut::<WorldRuntime>() {
        runtime.last_navigation_error = Some(error.clone());
    }
    world.resource_mut::<WorldMapState>().navigation_note = error;
}

fn publish_world_map(
    simulation: Res<FlightSimulation>,
    runtime: Res<WorldRuntime>,
    startup: Res<Startup>,
    playback: Option<Res<ReplayPlayback>>,
    mut map: ResMut<WorldMapState>,
    mut raster: ResMut<WorldMapRaster>,
    mut actions: ResMut<WorldMapActions>,
) {
    actions.month_changed.take();
    if !map.visible {
        return;
    }
    map.aircraft = Some(simulation.0.state().geodetic());
    let replay = playback.is_some() || simulation.0.is_replay() || startup.replay.is_some();
    map.navigation_enabled =
        !replay && (startup.world.global_terrain || startup.aircraft.is_jet()) && !map.regions.busy;
    map.navigation_note = if let Some(error) = &runtime.last_navigation_error {
        error.clone()
    } else if replay {
        "Replay preview only\nNew-flight relocation is disabled".into()
    } else if !startup.world.global_terrain && !startup.aircraft.is_jet() {
        LEGACY_MAP_NOTICE.into()
    } else if map.regions.busy {
        "Inspecting region; current flight unchanged".into()
    } else if map.regions.selected.is_some() || startup.active_region.is_some() {
        "New free flight: 1000 m AGL\nRegional replay recording/export unavailable".into()
    } else {
        "New flight: 1000 m AGL\nUnsaved recording will be reset".into()
    };
    let month = map.preview_month();
    let date = ClimateDate::from_month(month).expect("validated month");
    if !raster.ready || raster.month != month || raster.layer != map.layer {
        *raster = WorldMapRaster::from_sampler(map.layer, month, |position| {
            runtime.map_color(position, date, map.layer)
        });
    }
    if let Some(terrain) = runtime.global.sample(map.selected) {
        let position = Geodetic::new(
            map.selected.latitude,
            map.selected.longitude,
            terrain.surface_height,
        );
        let climate = runtime.climate.sample(position, date);
        map.selected_terrain = format!(
            "{} | ~{:.0} m MSL\nGlobal preview: ~20 km spacing{}",
            if terrain.is_land {
                "Land"
            } else if terrain.is_inland_water {
                "Inland water"
            } else {
                "Ocean"
            },
            terrain.elevation_msl.get(),
            if startup.tiles.is_some() {
                "\nLocal DEM has priority in flight"
            } else {
                ""
            }
        );
        map.selected_climate = format!(
            "{} | {:.1} C\nPrecip {:.1} mm/day | cloud {:.0}%\nSnow cue {:.0}% (derived)",
            climate.zone.label(),
            climate.temperature.to_celsius(),
            climate.precipitation_rate.get() * 86_400.0 * 1000.0,
            climate.cloud_cover * 100.0,
            climate.snow_fraction * 100.0
        );
    }
}

fn sync_climate_clouds(
    simulation: Res<FlightSimulation>,
    startup: Res<Startup>,
    runtime: Res<WorldRuntime>,
    time: Res<Time>,
    mut layer: ResMut<CloudLayer>,
    mut elapsed: Local<f64>,
) {
    let weather = simulation
        .0
        .jet_environment()
        .map_or(startup.weather.selection, |environment| environment.weather);
    if !matches!(weather, flightsim_sim::weather::WeatherSelection::Legacy)
        || startup.clouds_were_given
        || !startup.world.climate_enabled
    {
        return;
    }
    *elapsed += f64::from(time.delta_secs());
    if *elapsed < 2.0 && !startup.is_changed() {
        return;
    }
    *elapsed = 0.0;
    let Some(climate) = simulation.0.climate_sample() else {
        return;
    };
    let reference = runtime
        .climate
        .sample(startup.start, startup.world.climate_date);
    let next = modeled_climate_cloud_layer(climate, reference);
    // Equal assignments would unnecessarily regenerate the Light mask and
    // invalidate render settings every two seconds while the aircraft is still.
    if *layer != next {
        *layer = next;
    }
}

fn modeled_climate_cloud_layer(
    climate: flightsim_world::climate::ClimateSample,
    reference: flightsim_world::climate::ClimateSample,
) -> CloudLayer {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated climatological cloud fraction in [0,1]"
    )]
    let cover = climate.cloud_cover as f32;
    // This is one explicitly modeled layer, not a diagnosed cloud base. Use
    // the departure's fixed coarse model surface and geoid reference. Sampling
    // the aircraft's changing local DEM here made the entire deck follow hills.
    // Monthly temperature/precipitation do not supply humidity or stability.
    let base =
        (reference.model_surface_elevation.get() + reference.geoid_undulation.get() + 1500.0)
            .max(100.0);
    CloudLayer::try_new(cover, Meters(base), Meters(base + 1200.0), Meters(300.0), 1)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modeled_cloud_height_uses_fixed_departure_reference_not_current_hills() {
        let atlas = GlobalClimate::bundled().unwrap();
        let date = ClimateDate::from_month(6).unwrap();
        let reference = atlas.sample(Geodetic::from_degrees(47.0674, 9.5034, 0.0), date);
        let mut moved = reference;
        moved.model_surface_elevation = Meters(reference.model_surface_elevation.get() + 3000.0);
        moved.geoid_undulation = Meters(reference.geoid_undulation.get() + 20.0);
        moved.cloud_cover = 0.2;
        let before = modeled_climate_cloud_layer(reference, reference);
        let after = modeled_climate_cloud_layer(moved, reference);
        assert_eq!(before.base, after.base);
        assert_eq!(before.top, after.top);
        assert_eq!(after.cover.to_bits(), 0.2_f32.to_bits());
        assert!((after.top.get() - after.base.get() - 1200.0).abs() < 1e-9);
        assert!(before.base.get() > reference.model_surface_elevation.get());
    }

    fn map_camera_app(headless: bool, flight_active: bool) -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        app.insert_resource(Startup {
            headless_screenshot: headless,
            ..Startup::default()
        })
        .init_resource::<WorldMapState>()
        .init_resource::<Assets<Image>>()
        .add_systems(
            PostStartup,
            (
                super::super::screen_capture::setup_offscreen_target,
                setup_world_map_camera,
            )
                .chain(),
        )
        .add_systems(Update, sync_world_map_camera);
        let flight = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera {
                    is_active: flight_active,
                    viewport: Some(bevy::camera::Viewport {
                        physical_position: UVec2::new(12, 18),
                        physical_size: UVec2::new(960, 640),
                        ..default()
                    }),
                    ..default()
                },
                FlightCamera,
                IsDefaultUiCamera,
            ))
            .id();
        let root = app
            .world_mut()
            .spawn((Node::default(), world_map::WorldMapRoot))
            .id();
        let hud = app.world_mut().spawn(Node::default()).id();
        (app, flight, root, hud)
    }

    #[test]
    fn map_camera_copies_native_target_and_binds_only_the_map_root() {
        use bevy::window::WindowRef;
        for explicit_window in [false, true] {
            let (mut app, flight, root, hud) = map_camera_app(false, true);
            let window = app.world_mut().spawn_empty().id();
            let window_ref = if explicit_window {
                WindowRef::Entity(window)
            } else {
                WindowRef::Primary
            };
            app.world_mut()
                .entity_mut(flight)
                .insert(RenderTarget::Window(window_ref));
            app.update();
            let world = app.world();
            let view = world.resource::<WorldMapView>();
            let camera = world.get::<Camera>(view.map_camera).unwrap();
            assert!(!camera.is_active);
            assert!(world.get::<Camera>(flight).unwrap().is_active);
            assert_eq!(
                camera.viewport.as_ref().unwrap().physical_position,
                UVec2::new(12, 18)
            );
            assert_eq!(
                camera.viewport.as_ref().unwrap().physical_size,
                UVec2::new(960, 640)
            );
            assert!(
                matches!(world.get::<RenderTarget>(view.map_camera), Some(RenderTarget::Window(target)) if target.normalize(Some(window)) == window_ref.normalize(Some(window)))
            );
            assert_eq!(
                world.get::<UiTargetCamera>(root).unwrap().0,
                view.map_camera
            );
            assert!(world.get::<UiTargetCamera>(hud).is_none());
            assert!(world.get::<IsDefaultUiCamera>(flight).is_some());
            assert!(world.get::<IsDefaultUiCamera>(view.map_camera).is_none());
            assert!(
                world
                    .get::<RenderLayers>(view.map_camera)
                    .unwrap()
                    .iter()
                    .next()
                    .is_none()
            );
        }
    }

    #[test]
    fn map_camera_copies_the_actual_offscreen_image_after_capture_setup() {
        let (mut app, flight, root, _) = map_camera_app(true, true);
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_credits();
        app.update();
        let world = app.world();
        let view = world.resource::<WorldMapView>();
        let target = world
            .get::<RenderTarget>(flight)
            .unwrap()
            .as_image()
            .unwrap();
        let map_target = world
            .get::<RenderTarget>(view.map_camera)
            .unwrap()
            .as_image()
            .unwrap();
        assert_eq!(map_target.id(), target.id());
        assert!(world.resource::<Assets<Image>>().get(target).is_some());
        assert_eq!(
            world.get::<UiTargetCamera>(root).unwrap().0,
            view.map_camera
        );
        assert!(world.get::<Camera>(view.map_camera).unwrap().is_active);
        assert!(!world.get::<Camera>(flight).unwrap().is_active);
    }

    #[test]
    fn map_camera_repeated_close_restores_previous_activity_and_leaves_other_views_untouched() {
        for was_active in [false, true] {
            let (mut app, flight, _, _) = map_camera_app(false, was_active);
            app.update();
            let map_camera = app.world().resource::<WorldMapView>().map_camera;
            let other = app
                .world_mut()
                .spawn((
                    Camera3d::default(),
                    Camera {
                        is_active: true,
                        ..default()
                    },
                ))
                .id();
            let inactive_other = app
                .world_mut()
                .spawn((
                    Camera2d,
                    Camera {
                        is_active: false,
                        ..default()
                    },
                ))
                .id();
            for _ in 0..3 {
                app.world_mut().resource_mut::<WorldMapState>().visible = true;
                app.update();
                app.update(); // Do not replace the saved activity with our own false.
                assert!(!app.world().get::<Camera>(flight).unwrap().is_active);
                assert!(app.world().get::<Camera>(map_camera).unwrap().is_active);
                app.world_mut().resource_mut::<WorldMapState>().visible = false;
                app.update();
                app.update();
                assert_eq!(
                    app.world().get::<Camera>(flight).unwrap().is_active,
                    was_active
                );
                assert!(!app.world().get::<Camera>(map_camera).unwrap().is_active);
                assert!(app.world().get::<Camera>(other).unwrap().is_active);
                assert!(!app.world().get::<Camera>(inactive_other).unwrap().is_active);
            }
            assert_eq!(
                app.world_mut()
                    .query_filtered::<Entity, With<WorldMapCamera>>()
                    .iter(app.world())
                    .count(),
                1
            );
        }
    }

    #[test]
    fn successful_start_closes_map_and_restores_the_flight_view() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = map_jump_world();
        let flight = world
            .spawn((Camera3d::default(), FlightCamera, IsDefaultUiCamera))
            .id();
        world.spawn((Node::default(), world_map::WorldMapRoot));
        world.resource_mut::<WorldMapState>().visible = true;
        world.run_system_once(setup_world_map_camera).unwrap();
        world.run_system_once(sync_world_map_camera).unwrap();
        let map_camera = world.resource::<WorldMapView>().map_camera;
        assert!(!world.get::<Camera>(flight).unwrap().is_active);
        world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
            position: Geodetic::from_degrees(46.58, 8.0, 0.0),
            month: 1,
        });
        apply_world_map_start(&mut world);
        world.run_system_once(sync_world_map_camera).unwrap();
        assert!(!world.resource::<WorldMapState>().visible);
        assert!(world.get::<Camera>(flight).unwrap().is_active);
        assert!(!world.get::<Camera>(map_camera).unwrap().is_active);
    }

    #[test]
    fn calendar_boundary_rejects_ambiguous_or_impossible_dates() {
        for invalid in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-01",
            "1899-12-31",
            "2101-01-01",
            "2026-6-21",
            "2026-06-21-extra",
        ] {
            assert!(parse_date(invalid).is_none(), "{invalid}");
        }
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2026-06-21").is_some());
    }

    #[test]
    fn new_world_options_enable_offline_real_data_and_no_wall_clock() {
        let a = WorldOptions::default();
        let b = WorldOptions::default();
        assert!(a.global_terrain && a.climate_enabled);
        assert_eq!(a.climate_date, b.climate_date);
        assert_eq!(a.civil_date, (2026, 6, 21));
    }

    #[test]
    fn initial_preview_uses_restored_climate_month_instead_of_default_calendar() {
        let mut startup = Startup::default();
        startup.world.climate_date = ClimateDate::from_month(1).unwrap();
        let mut app = App::new();
        app.insert_resource(startup)
            .init_resource::<WorldMapState>()
            .add_systems(Startup, initialize_map);
        app.update();
        assert_eq!(app.world().resource::<WorldMapState>().month, 1);
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .source_credits
                .contains("Copernicus")
        );
    }

    #[test]
    fn modal_gate_keeps_the_close_frame_captured_without_changing_pause() {
        let mut app = App::new();
        app.init_resource::<WorldMapState>()
            .init_resource::<MapCapture>()
            .insert_resource(flightsim_ui::Paused(true))
            .add_systems(Update, capture_map_input);
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        app.update();
        assert!(app.world().resource::<MapCapture>().captured);
        app.world_mut().resource_mut::<WorldMapState>().visible = false;
        app.update();
        assert!(app.world().resource::<MapCapture>().captured);
        app.update();
        assert!(!app.world().resource::<MapCapture>().captured);
        assert!(app.world().resource::<flightsim_ui::Paused>().is_paused());
    }

    #[test]
    fn source_factory_only_disables_primary_discovery_without_a_configured_path() {
        let mut startup = Startup::default();
        for global_terrain in [true, false] {
            startup.world.global_terrain = global_terrain;
            startup.tiles = None;
            let source = make_source(&startup);
            assert!(!source.primary_reads_possible());
            assert_eq!(source.has_fallback(), global_terrain);
            startup.tiles = Some("possibly-future-regional-tiles".into());
            let source = make_source(&startup);
            assert!(source.primary_reads_possible());
            assert_eq!(source.has_fallback(), global_terrain);
        }
    }

    #[test]
    fn every_world_destination_spawns_clear_of_surface_with_finite_speed() {
        let mut startup = Startup::default();
        for destination in world_map::WORLD_MAP_DESTINATIONS {
            startup.start = destination.position();
            let mut terrain = Terrain::new(make_source(&startup), 1024 * 1024, 8..=13);
            let state = airborne_state(&startup, &mut terrain, Meters(1000.0));
            let ground = GroundSampler::default().sample(&mut terrain, startup.start);
            assert!(state.is_finite());
            assert!((state.altitude().get() - ground.elevation.get() - 1000.0).abs() < 1e-5);
            assert!(state.velocity_ned().0.length() > 40.0);
        }
    }

    #[test]
    fn world_cli_values_and_manual_cloud_override_are_explicit() {
        let (startup, notes) = parse_arguments_from(
            [
                "--start",
                "-33.95,151.18",
                "--date",
                "2026-01-15",
                "--fly",
                "1500",
                "--world-map",
                "--cloud-cover",
                "0.2",
                "--global-terrain",
                "off",
                "--climate",
                "off",
            ]
            .map(str::to_owned),
        );
        assert!(notes.0.is_empty(), "{:?}", notes.0);
        assert!(!startup.world.global_terrain && !startup.world.climate_enabled);
        assert!(startup.world.map_open && startup.clouds_were_given);
        assert_eq!(startup.world.civil_date, (2026, 1, 15));
        assert_eq!(startup.world.fly_height, Some(Meters(1500.0)));
    }

    #[test]
    fn bad_world_options_do_not_swallow_a_following_valid_option() {
        let (startup, notes) = parse_arguments_from(
            [
                "--date",
                "--world-map",
                "--fly",
                "NaN",
                "--climate",
                "perhaps",
                "--global-terrain",
                "on",
            ]
            .map(str::to_owned),
        );
        assert_eq!(notes.0.len(), 3);
        assert!(startup.world.map_open && startup.world.global_terrain);
        assert!(startup.world.fly_height.is_none());
    }

    #[test]
    fn map_credits_option_opens_map_without_changing_flight_settings() {
        let (startup, notes) = parse_arguments_from(["--map-credits".to_owned()]);
        assert!(notes.0.is_empty());
        assert!(startup.world.map_open && startup.world.map_credits);
        assert!(startup.world.fly_height.is_none());
        assert_eq!(startup.world.civil_date, WorldOptions::default().civil_date);
        let mut app = App::new();
        app.insert_resource(startup)
            .init_resource::<WorldMapState>()
            .add_systems(Startup, initialize_map);
        app.update();
        let map = app.world().resource::<WorldMapState>();
        assert!(map.visible);
        assert!(map.source_credits.contains("Copernicus"));
    }

    fn map_jump_world() -> World {
        let mut world = World::new();
        world.insert_resource(Startup::default());
        world.insert_resource(WorldMapActions::default());
        world.insert_resource(WorldMapState::default());
        world.insert_resource(CameraRig::default());
        world.insert_resource(TerrainTiles::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(TerrainStreaming {
            selector: LodSelector::new(
                16.0,
                720.0,
                Degrees(60.0).to_radians(),
                13,
                Meters(20_000.0),
            ),
            source: Box::new(MemoryTileSource::new()) as BoxedSource,
            cache: TileCache::new(1024 * 1024),
            live: flightsim_render::TerrainSelectionState::default(),
            material: Handle::default(),
        });
        world
    }

    pub(super) fn jet_profile() -> aircraft_profile::SelectedAircraftProfile {
        aircraft_profile::SelectedAircraftProfile::Jet(
            flightsim_sim::aircraft_profile::AircraftProfileV2::parse(include_str!(
                "../../../docs/examples/aircraft-profiles-v2/numerical-jet.json"
            ))
            .unwrap(),
        )
    }

    #[test]
    fn jet_airborne_initial_conditions_use_profile_hints_and_explicit_wind() {
        let mut startup = Startup {
            aircraft: jet_profile(),
            ..Default::default()
        };
        startup.world.global_terrain = false;
        startup.world.fly_height = Some(Meters(4000.0));
        startup.heading = Degrees(90.0).to_radians();
        startup.wind = flightsim_sim::Wind {
            from: Degrees(270.0).to_radians(),
            speed: flightsim_core::MetersPerSecond(7.0),
        };
        let hints = startup.aircraft.jet().unwrap().controls();
        let mut terrain = Terrain::new(make_source(&startup), 1024, 8..=12);
        let state = airborne_state(&startup, &mut terrain, Meters(4000.0));
        assert!((state.altitude().get() - 4000.0).abs() < 1e-8);
        let velocity = state.velocity_ned().0;
        assert!(velocity.x.abs() < 1e-9);
        assert!((velocity.y - hints.approach_speed_mps.get() - 7.0).abs() < 1e-9);
        assert!(velocity.z.abs() < 1e-9);
        assert!((state.attitude().pitch.get() - hints.approach_pitch_rad.get()).abs() < 1e-9);
        let controls = initial_controls(&startup).to_control_inputs();
        assert_eq!(
            controls.throttle().to_bits(),
            hints.approach_throttle.get().to_bits()
        );
        assert_eq!(
            controls.flaps().to_bits(),
            hints.approach_flaps.get().to_bits()
        );
        assert_eq!(
            controls.elevator().to_bits(),
            hints.approach_trim.get().to_bits()
        );
    }

    #[test]
    fn jet_departure_weather_and_altitude_match_authoritative_ground() {
        use flightsim_sim::weather::{WeatherPreset, WeatherSelection};
        // Neither user render-detail levels nor the legacy global ancestor range
        // changes the jet's supported physical sampler contract.
        let mut legacy = Startup {
            min_level: 6,
            max_level: 15,
            ..Default::default()
        };
        assert_eq!(terrain_levels(&legacy), 0..=15);
        legacy.world.global_terrain = false;
        assert_eq!(terrain_levels(&legacy), 6..=15);
        for global in [true, false] {
            for (latitude, longitude) in [
                (0.0, -140.0),
                (30.0, 90.0),
                (0.0, 180.0),
                (90.0, -180.0),
                (-90.0, 180.0),
            ] {
                let mut startup = Startup {
                    aircraft: jet_profile(),
                    start: Geodetic::from_degrees(latitude, longitude, 0.0),
                    min_level: 6,
                    max_level: 15,
                    ..Default::default()
                };
                startup.world.global_terrain = global;
                startup.world.fly_height = Some(Meters(1000.0));
                startup.weather.requested = Some(WeatherPreset::Rain);
                assert_eq!(terrain_levels(&startup), 8..=12);
                let mut terrain =
                    Terrain::new(make_source(&startup), 1024 * 1024, terrain_levels(&startup));
                super::super::weather_runtime::resolve_with_terrain(&mut startup, &mut terrain)
                    .unwrap();
                let state = airborne_state(&startup, &mut terrain, Meters(1000.0));
                let session = crate::flight_session::FlightSession::prepare_jet(
                    &startup,
                    &startup_clock(&startup),
                    StartCondition::InFlight(state),
                )
                .unwrap();
                let ground = session.ground().elevation;
                assert!((state.altitude().get() - ground.get() - 1000.0).abs() < 1e-5);
                let WeatherSelection::Modeled(weather) = startup.weather.selection else {
                    panic!("explicit rain preset was not resolved");
                };
                let reference = weather.parameters().departure_reference;
                assert!((reference.altitude.get() - ground.get()).abs() < 1e-6);
                assert_eq!(reference.latitude, startup.start.latitude);
                assert_eq!(reference.longitude, startup.start.longitude);
                assert_eq!(
                    session.jet_environment().unwrap().weather,
                    startup.weather.selection
                );
            }
        }
    }

    #[test]
    fn jet_map_start_commits_bundled_global_and_explicit_flat_zero_sessions() {
        use flightsim_sim::{model_simulation::JetTerrain, weather::WeatherPreset};
        for global in [true, false] {
            let mut world = map_jump_world();
            {
                let mut startup = world.resource_mut::<Startup>();
                startup.aircraft = jet_profile();
                startup.world.global_terrain = global;
            }
            let mut pending = super::super::weather_runtime::PendingWeather::default();
            pending.requested = Some(WeatherPreset::Rain);
            world.insert_resource(pending);
            let request = world_map::WorldMapStart {
                position: Geodetic::from_degrees(0.0, -140.0, 0.0),
                month: 7,
            };
            world.resource_mut::<WorldMapState>().visible = true;
            world.resource_mut::<WorldMapActions>().start_at = Some(request);
            apply_world_map_start(&mut world);
            let session = &world.resource::<FlightSimulation>().0;
            assert!(session.is_jet());
            assert!(!session.is_replay());
            assert!((session.agl().get() - 1000.0).abs() < 1e-8);
            assert_eq!(session.elapsed(), Seconds::ZERO);
            let environment = session.jet_environment().unwrap();
            assert_eq!(
                environment.terrain,
                if global {
                    JetTerrain::BundledGlobal
                } else {
                    JetTerrain::Flat {
                        elevation: Meters::ZERO,
                    }
                }
            );
            assert_eq!(
                environment.weather,
                world.resource::<Startup>().weather.selection
            );
            assert_eq!(session.climate(), ClimateDate::from_month(7));
            assert!(world.resource::<WorldMapActions>().start_at.is_none());
            assert!(!world.resource::<WorldMapState>().visible);
            assert!(!world.contains_resource::<FlightRecorder>());
            assert_eq!(world.resource::<Startup>().start, request.position);
        }
    }

    #[test]
    fn failed_jet_environment_and_domain_preserve_flight_weather_and_start_request() {
        for (newline, outside_domain) in
            [("\n", false), ("\n", true), ("\r\n", false), ("\r\n", true)]
        {
            let mut world = map_jump_world();
            let source =
                include_str!("../../../docs/examples/aircraft-profiles-v2/numerical-jet.json")
                    .replace("\r\n", "\n")
                    .replace('\n', newline);
            let before = format!("\"pressure_ratio\": [{newline}        0.01,");
            let after = format!("\"pressure_ratio\": [{newline}        0.7,");
            assert_eq!(source.matches(&before).count(), 1);
            let json = source.replacen(&before, &after, 1);
            assert!(!json.contains(&before));
            assert!(json.contains(&after));
            let profile = flightsim_sim::aircraft_profile::AircraftProfileV2::parse(&json).unwrap();
            assert_eq!(
                profile
                    .configuration()
                    .envelope()
                    .definition()
                    .pressure_ratio[0]
                    .to_bits(),
                0.7_f64.to_bits()
            );
            world.resource_mut::<Startup>().aircraft =
                aircraft_profile::SelectedAircraftProfile::Jet(profile);
            world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
                position: Geodetic::from_degrees(0.0, -140.0, 0.0),
                month: 7,
            });
            apply_world_map_start(&mut world);
            let before = *world.resource::<FlightSimulation>().0.state();
            let snapshot = world
                .resource::<FlightSimulation>()
                .0
                .jet()
                .unwrap()
                .snapshot();
            let clock = *world.resource::<flightsim_render::TimeOfDay>();
            let controls = world.resource::<PilotControls>().to_control_inputs();
            let start = world.resource::<Startup>().start;
            let weather = world.resource::<Startup>().weather.selection;
            let tower = world.resource::<TowerViewAnchor>().0;
            // A forged in-memory environment is rejected by full session preparation,
            // after candidate terrain/weather have resolved but before any commit.
            if !outside_domain {
                world.resource_mut::<Startup>().time_rate = f64::NAN;
            }
            let mut pending = super::super::weather_runtime::PendingWeather::default();
            pending.requested = Some(flightsim_sim::weather::WeatherPreset::Snow);
            world.insert_resource(pending);
            let request = world_map::WorldMapStart {
                position: if outside_domain {
                    Geodetic::from_degrees(30.0, 90.0, 0.0)
                } else {
                    Geodetic::from_degrees(35.0, 139.0, 0.0)
                },
                month: 1,
            };
            if outside_domain {
                // At this high-terrain departure, 1000 m AGL leaves the explicitly
                // narrowed pressure envelope even though all source/state inputs are valid.
                let mut candidate = world.resource::<Startup>().clone();
                candidate.start = request.position;
                let mut terrain = Terrain::new(
                    make_source(&candidate),
                    1024 * 1024,
                    terrain_levels(&candidate),
                );
                let state = airborne_state(&candidate, &mut terrain, Meters(1000.0));
                let atmosphere = flightsim_sim::climate_atmosphere_sample(
                    state.geodetic(),
                    ClimateDate::from_month(request.month),
                )
                .unwrap();
                let ambient = flightsim_fdm::subsonic::JetConditions::from_atmosphere(
                    atmosphere,
                    flightsim_core::MetersPerSecond(35.0),
                )
                .unwrap();
                assert!(ambient.pressure_ratio.0 < 0.7);
                assert!(world.resource::<Startup>().time_rate.is_finite());
            }
            world.resource_mut::<WorldMapState>().regions.status = "Existing region preview".into();
            world.resource_mut::<WorldMapState>().visible = true;
            world.resource_mut::<WorldMapActions>().start_at = Some(request);
            apply_world_map_start(&mut world);
            assert_eq!(*world.resource::<FlightSimulation>().0.state(), before);
            assert_eq!(
                world
                    .resource::<FlightSimulation>()
                    .0
                    .jet()
                    .unwrap()
                    .snapshot(),
                snapshot
            );
            assert_eq!(
                world.resource::<flightsim_render::TimeOfDay>().utc,
                clock.utc
            );
            assert_eq!(
                world.resource::<flightsim_render::TimeOfDay>().rate,
                clock.rate
            );
            assert_eq!(
                world.resource::<PilotControls>().to_control_inputs(),
                controls
            );
            assert_eq!(world.resource::<Startup>().start, start);
            assert_eq!(world.resource::<Startup>().weather.selection, weather);
            assert_eq!(
                world
                    .resource::<FlightSimulation>()
                    .0
                    .jet_environment()
                    .unwrap()
                    .weather,
                weather
            );
            assert_eq!(world.resource::<TowerViewAnchor>().0, tower);
            assert_eq!(world.resource::<WorldMapActions>().start_at, Some(request));
            let map = world.resource::<WorldMapState>();
            assert!(map.visible);
            assert!(map.navigation_note.starts_with("Cannot start jet flight:"));
            if outside_domain {
                assert!(map.navigation_note.contains("operating envelope"));
            }
            assert!(world.resource::<Startup>().active_region.is_none());
            assert!(map.regions.selected.is_none());
            assert_eq!(map.regions.status, "Existing region preview");
        }
    }

    #[test]
    fn legacy_terrain_rejects_forged_map_start_without_moving_airport_or_aircraft() {
        let mut world = map_jump_world();
        world.resource_mut::<Startup>().world.global_terrain = false;
        let startup = world.resource::<Startup>().clone();
        let source: BoxedSource = Box::new(MemoryTileSource::new());
        let simulation = Simulation::parked(
            startup.aircraft.configuration(),
            startup.start,
            startup.heading,
            Terrain::new(source, 1024, 8..=13),
            GroundSampler::default(),
        );
        let aircraft_before = *simulation.state();
        world.insert_resource(FlightSimulation(simulation.into()));
        world.insert_resource(ActiveRunway(startup.runway));
        let airport_before = startup.runway.threshold.to_ecef();
        let airport = world.spawn(WorldPosition(airport_before)).id();
        {
            let mut map = world.resource_mut::<WorldMapState>();
            map.visible = true;
            map.navigation_enabled = true; // A forged/stale UI flag cannot bypass the guard.
        }
        world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
            position: world_map::WORLD_MAP_DESTINATIONS[0].position(),
            month: 7,
        });
        apply_world_map_start(&mut world);
        assert_eq!(
            *world.resource::<FlightSimulation>().0.state(),
            aircraft_before
        );
        assert_eq!(world.resource::<ActiveRunway>().0, startup.runway);
        assert_eq!(
            world.get::<WorldPosition>(airport).unwrap().0,
            airport_before
        );
        assert_eq!(world.resource::<Startup>().start, startup.start);
        assert!(!world.resource::<Startup>().world.global_terrain);
        assert!(!world.contains_resource::<FlightRecorder>());
        let map = world.resource::<WorldMapState>();
        assert!(map.visible);
        assert!(!map.navigation_enabled);
        assert_eq!(map.navigation_note, LEGACY_MAP_NOTICE);
    }

    #[test]
    fn legacy_terrain_initial_map_is_explicitly_preview_only() {
        let mut startup = Startup::default();
        startup.world.global_terrain = false;
        let mut app = App::new();
        app.insert_resource(startup)
            .init_resource::<WorldMapState>()
            .add_systems(Startup, initialize_map);
        app.update();
        let map = app.world().resource::<WorldMapState>();
        assert!(!map.navigation_enabled);
        assert_eq!(map.navigation_note, LEGACY_MAP_NOTICE);
        assert!(map.navigation_note.lines().all(|line| line.len() <= 42));
    }

    fn seed_pending_terrain_replacement(world: &mut World) {
        use bevy::ecs::world::CommandQueue;
        use flightsim_core::RenderFrame;
        use flightsim_render::TerrainUpdate;
        use flightsim_render::terrain::{TerrainTile, prepare_tile};
        use flightsim_render::terrain_stitching::apply_stitched_update;
        use flightsim_world::{DemTile, HeightGrid, TileId};

        let a = TileId::new(3, 1, 4);
        let b = TileId::new(7, 32, 68);
        let frame = RenderFrame::new(Geodetic::from_degrees(-6.0, -133.5, 12_000.0));
        let mut tiles = world.remove_resource::<TerrainTiles>().unwrap();
        let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, world);
            for id in [a, b] {
                let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters::ZERO));
                tiles.insert_prepared(prepare_tile(
                    &mut commands,
                    &mut meshes,
                    Handle::<StandardMaterial>::default(),
                    &frame,
                    id,
                    &dem,
                    None,
                ));
            }
            apply_stitched_update(
                &mut commands,
                &mut meshes,
                &mut tiles,
                Handle::<StandardMaterial>::default(),
                &frame,
                TerrainUpdate {
                    prepared: vec![a, b],
                    spawned: vec![a, b],
                    ..Default::default()
                },
                3,
                None,
            );
        }
        queue.apply(world);
        assert!(!tiles.is_stitching());
        {
            let mut commands = Commands::new(&mut queue, world);
            let dem = DemTile::new(b.bounds(), HeightGrid::flat(33, 33, Meters(2_000.0)));
            tiles.insert_prepared(prepare_tile(
                &mut commands,
                &mut meshes,
                Handle::<StandardMaterial>::default(),
                &frame,
                b,
                &dem,
                None,
            ));
            apply_stitched_update(
                &mut commands,
                &mut meshes,
                &mut tiles,
                Handle::<StandardMaterial>::default(),
                &frame,
                TerrainUpdate {
                    prepared: vec![b],
                    spawned: vec![b],
                    replaced: vec![b],
                    ..Default::default()
                },
                1,
                None,
            );
        }
        queue.apply(world);
        assert!(tiles.is_stitching());
        assert_eq!(tiles.resource_usage().retired_surface_meshes, 1);
        assert_eq!(world.query::<&TerrainTile>().iter(world).count(), 3);
        world.insert_resource(tiles);
        world.insert_resource(meshes);
    }

    #[test]
    fn map_start_cancels_pending_bridges_and_retires_all_old_source_geometry() {
        use flightsim_render::terrain::TerrainTile;
        use flightsim_render::terrain_stitching::TerrainBridge;
        let mut world = map_jump_world();
        for (lat, lon) in [(0.0, 180.0), (-90.0, 0.0)] {
            seed_pending_terrain_replacement(&mut world);
            assert!(world.resource::<Assets<Mesh>>().len() >= 4);
            world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
                position: Geodetic::from_degrees(lat, lon, 0.0),
                month: 7,
            });
            apply_world_map_start(&mut world);
            let tiles = world.resource::<TerrainTiles>();
            assert!(tiles.is_empty());
            assert!(!tiles.is_stitching());
            assert_eq!(tiles.resource_usage(), Default::default());
            assert!(world.resource::<Assets<Mesh>>().is_empty());
            assert_eq!(world.query::<&TerrainTile>().iter(&world).count(), 0);
            assert_eq!(world.query::<&TerrainBridge>().iter(&world).count(), 0);
        }
    }

    #[test]
    fn repeated_map_starts_reset_state_and_record_matching_world_conditions() {
        let mut world = map_jump_world();
        for (lat, lon, month) in [
            (46.58, 8.0, 1),
            (-33.95, 151.18, 7),
            (0.0, 180.0, 12),
            (90.0, -180.0, 6),
        ] {
            world.resource_mut::<WorldMapState>().visible = true;
            world.insert_resource(flightsim_ui::Paused(true));
            world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
                position: Geodetic::from_degrees(lat, lon, 0.0),
                month,
            });
            apply_world_map_start(&mut world);
            let sim = &world.resource::<FlightSimulation>().0;
            assert!(sim.state().is_finite());
            assert!(
                (sim.state().altitude().get() - sim.ground().elevation.get() - 1000.0).abs() < 1e-5
            );
            assert_eq!(sim.climate(), ClimateDate::from_month(month));
            let recorder = &world.resource::<FlightRecorder>().0;
            assert!(recorder.recording().frames().is_empty());
            assert_eq!(
                recorder.recording().conditions().environment.climate_date,
                sim.climate()
            );
            assert!(recorder.recording().conditions().environment.world_terrain);
            assert!(!world.resource::<WorldMapState>().visible);
            assert!(!world.resource::<flightsim_ui::Paused>().is_paused());
        }
    }

    #[test]
    fn new_flight_weather_commits_only_with_start_and_uses_actual_surface() {
        use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
        let mut world = map_jump_world();
        world.insert_resource(super::super::weather_runtime::PendingWeather::default());
        for (preset, lat, lon) in [
            (WeatherPreset::Clear, 0.0, -140.0),
            (WeatherPreset::Cloud, 31.5, 35.5),
            (WeatherPreset::Fog, 90.0, 180.0),
            (WeatherPreset::Rain, -90.0, -180.0),
            (WeatherPreset::Snow, 0.0, 180.0),
            (WeatherPreset::Storm, 46.5, 8.0),
        ] {
            let before = world.resource::<Startup>().weather.selection;
            world
                .resource_mut::<super::super::weather_runtime::PendingWeather>()
                .requested = Some(preset);
            apply_world_map_start(&mut world); // Merely selecting cannot mutate a flight.
            assert_eq!(world.resource::<Startup>().weather.selection, before);
            world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
                position: Geodetic::from_degrees(lat, lon, 0.0),
                month: 7,
            });
            apply_world_map_start(&mut world);
            let startup = world.resource::<Startup>();
            // Resolve at the exact requested departure, before an ECEF state
            // round-trip slightly changes the sampler's latitude/longitude.
            let mut reference_terrain =
                Terrain::new(make_source(startup), 1024 * 1024, terrain_levels(startup));
            let reference = Geodetic::from_degrees(lat, lon, 0.0);
            let ground = GroundSampler::default()
                .sample(&mut reference_terrain, reference)
                .elevation;
            assert!(
                (ground.get()
                    - world
                        .resource::<FlightSimulation>()
                        .0
                        .ground()
                        .elevation
                        .get())
                .abs()
                    < 1e-8
            );
            let expected = WeatherSelection::Modeled(
                WeatherScenario::from_preset(
                    preset,
                    Geodetic::from_degrees(lat, lon, ground.get()),
                    startup.weather.seed,
                )
                .unwrap(),
            );
            assert_eq!(startup.weather.selection, expected);
            assert_eq!(
                world
                    .resource::<FlightRecorder>()
                    .0
                    .recording()
                    .conditions()
                    .weather,
                expected
            );
            assert_eq!(
                world.resource::<FlightSimulation>().0.elapsed(),
                Seconds::ZERO
            );
            assert!(!world.resource::<WorldMapState>().visible);
        }
    }

    #[test]
    fn a_preview_month_does_not_mutate_existing_physics_or_recorder() {
        let mut world = map_jump_world();
        world.resource_mut::<WorldMapActions>().start_at = Some(world_map::WorldMapStart {
            position: Geodetic::from_degrees(46.58, 8.0, 0.0),
            month: 1,
        });
        apply_world_map_start(&mut world);
        let old_date = world.resource::<FlightSimulation>().0.climate();
        let old_state = *world.resource::<FlightSimulation>().0.state();
        world.resource_mut::<WorldMapState>().month = 7;
        world.resource_mut::<WorldMapActions>().month_changed = Some(7);
        apply_world_map_start(&mut world);
        assert_eq!(*world.resource::<FlightSimulation>().0.state(), old_state);
        assert_eq!(world.resource::<FlightSimulation>().0.climate(), old_date);
        assert_eq!(
            world
                .resource::<FlightRecorder>()
                .0
                .recording()
                .conditions()
                .environment
                .climate_date,
            old_date
        );
    }

    #[test]
    fn airborne_start_preserves_target_airspeed_in_head_tail_and_crosswinds() {
        let mut startup = Startup {
            start: Geodetic::from_degrees(46.58, 8.0, 0.0),
            ..Startup::default()
        };
        for from in [0.0, 90.0, 180.0, 270.0] {
            startup.wind = flightsim_sim::Wind {
                from: Degrees(from).to_radians(),
                speed: Knots(100.0).to_meters_per_second(),
            };
            let mut terrain =
                Terrain::new(make_source(&startup), 1024 * 1024, terrain_levels(&startup));
            let state = airborne_state(&startup, &mut terrain, Meters(1000.0));
            let density = flightsim_sim::climate_atmosphere_sample(
                state.geodetic(),
                startup.world.climate_date(),
            )
            .unwrap()
            .density_ratio()
            .max(0.15);
            let expected = Knots(90.0).to_meters_per_second().get() / density.sqrt();
            let relative = state.velocity_ned().0 - startup.wind.to_ned().0;
            assert!(
                (relative.length() - expected).abs() < 1e-6,
                "wind from {from}: {relative:?}"
            );
            let direction = Attitude::new(Radians::ZERO, Radians::ZERO, startup.heading)
                .to_quaternion()
                * bevy::math::DVec3::X;
            assert!((relative.normalize() - direction).length() < 1e-8);
        }
    }

    #[test]
    fn tower_uses_its_own_ground_on_slopes_and_stays_valid_at_both_poles() {
        let id = flightsim_world::TileId::containing(12, Geodetic::from_degrees(46.58, 8.0, 0.0));
        let heights = (0..33)
            .flat_map(|_| (0..33).map(|x| f32::from(u16::try_from(x).unwrap()) * 100.0))
            .collect();
        let mut source = MemoryTileSource::new();
        source.insert(
            id,
            flightsim_world::DemTile::new(
                id.bounds(),
                flightsim_world::HeightGrid::new(33, 33, heights),
            ),
        );
        let boxed: BoxedSource = Box::new(source);
        let mut terrain = Terrain::new(boxed, 1024 * 1024, 0..=13);
        let tower = tower_anchor(&mut terrain, id.center());
        let ground = GroundSampler::default()
            .sample(&mut terrain, tower)
            .elevation;
        assert!((tower.altitude.get() - ground.get() - TOWER_CLEARANCE.get()).abs() < 1e-7);
        for latitude in [-90.0, 90.0] {
            let tower = tower_anchor(&mut terrain, Geodetic::from_degrees(latitude, 180.0, 0.0));
            assert!(world_map::map_point_from_geodetic(tower).is_some());
        }
    }
}

#[cfg(test)]
#[path = "scenery_map_tests.rs"]
mod scenery_map_tests;
