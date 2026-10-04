//! Modal, offline world navigation map.
//!
//! The app owns terrain/climate sampling, pause state and flight restarts. This
//! module only presents one bounded equirectangular raster and queues explicit
//! user actions. A preview month never changes the current simulation by itself.
//! Register input before simulation/pilot input, publish app data after input,
//! and run display last. Gate flight and camera input while `visible` is true.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::LineHeight;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use flightsim_core::{Degrees, Geodetic, Meters};

#[path = "regions.rs"]
mod regions;
pub use regions::{
    RegionAction, RegionSummary, RegionsActions, RegionsButton, RegionsState, RegionsText,
    WorldMapRegionsRoot,
};

/// Bounded global raster: one sample per half degree, north at the top.
pub const WORLD_MAP_WIDTH: u32 = 720;
pub const WORLD_MAP_HEIGHT: u32 = 360;
const RASTER_BYTES: usize = 720 * 360 * 4;
const CREDIT_COLUMNS: usize = 96;
const CREDIT_LINES_PER_PAGE: usize = 20;
const MAX_CREDIT_CHARACTERS: usize = 16_384;
// Layout budget for the supported 1280x720 viewport. The centered maximum
// width also prevents wide windows from making the 2:1 map too tall.
const MAP_ROOT_PADDING: f32 = 16.0;
const MAP_ROOT_GAP: f32 = 10.0;
const MAP_HEADER_HEIGHT: f32 = 36.0;
const MAP_FOOTER_HEIGHT: f32 = 16.0;
const MAP_BODY_GAP: f32 = 16.0;
const MAP_COLUMN_MAX_WIDTH: f32 = 900.0;
const MAP_COLUMN_GAP: f32 = 6.0;
const SIDEBAR_WIDTH: f32 = 308.0;
const SIDEBAR_PADDING: f32 = 14.0;
const SIDEBAR_GAP: f32 = 6.0;
const MAP_BUTTON_HEIGHT: f32 = 29.0;
const DETAILS_FONT_SIZE: f32 = 12.0;
const DETAILS_LINE_HEIGHT: f32 = 15.0;
const DETAILS_COLUMNS: usize = 36;
const TERRAIN_DETAIL_LINES: usize = 3;
const CLIMATE_DETAIL_LINES: usize = 4;
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const TEXT: Color = Color::srgb(0.91, 0.95, 1.0);
const MUTED: Color = Color::srgb(0.60, 0.72, 0.83);
const ACCENT: Color = Color::srgb(0.25, 0.83, 0.88);
const BUTTON: Color = Color::srgb(0.10, 0.16, 0.22);

/// App ordering points. Input must precede the app's input/pause gating.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldMapSystems {
    Input,
    Display,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorldMapLayer {
    #[default]
    Terrain,
    Climate,
}

impl WorldMapLayer {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Terrain => "TERRAIN",
            Self::Climate => "CLIMATE",
        }
    }
}

/// A normalized, dimensionless UI position. `(0, 0)` is north-west.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPoint {
    pub x: f64,
    pub y: f64,
}

/// Linear equirectangular UI projection, not an ECEF/geodetic conversion.
/// Invalid latitude/altitude and non-finite coordinates are rejected. Wrapped
/// longitudes use core's wrapping convention; the two dateline edges remain
/// individually selectable for exact -180 / +180 degree inputs.
#[must_use]
pub fn map_point_from_geodetic(position: Geodetic) -> Option<MapPoint> {
    let latitude = position.latitude.to_degrees().get();
    let longitude = position.longitude.to_degrees().get();
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !position.altitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
    {
        return None;
    }
    let longitude = if (-180.0..=180.0).contains(&longitude) {
        longitude
    } else {
        position.longitude.wrap_signed().to_degrees().get()
    };
    Some(MapPoint {
        x: ((longitude + 180.0) / 360.0).clamp(0.0, 1.0),
        y: ((90.0 - latitude) / 180.0).clamp(0.0, 1.0),
    })
}

/// A click on the inclusive map bounds selects a surface coordinate. The app
/// chooses safe AGL and samples actual terrain when starting a new flight.
#[must_use]
pub fn geodetic_from_map_point(point: MapPoint) -> Option<Geodetic> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !(0.0..=1.0).contains(&point.x)
        || !(0.0..=1.0).contains(&point.y)
    {
        return None;
    }
    Some(Geodetic::new(
        Degrees(90.0 - point.y * 180.0).to_radians(),
        Degrees(point.x * 360.0 - 180.0).to_radians(),
        Meters::ZERO,
    ))
}

/// Recognizable geographic examples, not claims of airport/runway availability.
#[derive(Debug, Clone, Copy)]
pub struct WorldMapDestination {
    pub name: &'static str,
    pub latitude: Degrees,
    pub longitude: Degrees,
}

impl WorldMapDestination {
    #[must_use]
    pub fn position(self) -> Geodetic {
        Geodetic::new(
            self.latitude.to_radians(),
            self.longitude.to_radians(),
            Meters::ZERO,
        )
    }
}

pub const WORLD_MAP_DESTINATIONS: [WorldMapDestination; 8] = [
    WorldMapDestination {
        name: "Tokyo",
        latitude: Degrees(35.55),
        longitude: Degrees(139.78),
    },
    WorldMapDestination {
        name: "Alps",
        latitude: Degrees(46.58),
        longitude: Degrees(8.00),
    },
    WorldMapDestination {
        name: "Sahara",
        latitude: Degrees(23.00),
        longitude: Degrees(13.00),
    },
    WorldMapDestination {
        name: "Amazon",
        latitude: Degrees(-3.12),
        longitude: Degrees(-60.02),
    },
    WorldMapDestination {
        name: "Andes",
        latitude: Degrees(-13.52),
        longitude: Degrees(-71.97),
    },
    WorldMapDestination {
        name: "Sydney",
        latitude: Degrees(-33.95),
        longitude: Degrees(151.18),
    },
    WorldMapDestination {
        name: "Arctic",
        latitude: Degrees(78.22),
        longitude: Degrees(15.65),
    },
    WorldMapDestination {
        name: "Date line",
        latitude: Degrees(-16.50),
        longitude: Degrees(179.90),
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinateField {
    Latitude,
    Longitude,
}

/// Public app-fed state. Descriptions are sanitized and bounded at display time.
#[derive(Resource, Debug, Clone)]
pub struct WorldMapState {
    pub visible: bool,
    pub regions: RegionsState,
    pub aircraft: Option<Geodetic>,
    pub selected: Geodetic,
    pub selected_name: String,
    /// Preview month, 1 through 12. It is applied only on an explicit start.
    pub month: u8,
    pub layer: WorldMapLayer,
    pub selected_terrain: String,
    pub selected_climate: String,
    pub navigation_enabled: bool,
    /// App can explain why a start is unavailable, for example during replay.
    pub navigation_note: String,
    /// Full app-verified ASCII derivative/source credits, shown in a separate
    /// paginated modal rather than abbreviated in the navigation sidebar.
    pub source_credits: String,
    credits_visible: bool,
    credits_page: usize,
    coordinate_field: Option<CoordinateField>,
    coordinate_draft: String,
    coordinate_error: String,
}

impl Default for WorldMapState {
    fn default() -> Self {
        Self {
            visible: false,
            regions: RegionsState::default(),
            aircraft: None,
            selected: WORLD_MAP_DESTINATIONS[0].position(),
            selected_name: WORLD_MAP_DESTINATIONS[0].name.to_owned(),
            month: 7,
            layer: WorldMapLayer::Terrain,
            selected_terrain: "Terrain sample pending".to_owned(),
            selected_climate: "Climate sample pending".to_owned(),
            navigation_enabled: true,
            navigation_note: "New flight: safe airborne start above terrain".to_owned(),
            source_credits: "Data credits are supplied by the application.".to_owned(),
            credits_visible: false,
            credits_page: 0,
            coordinate_field: None,
            coordinate_draft: String::new(),
            coordinate_error: String::new(),
        }
    }
}

impl WorldMapState {
    /// Open regional package selection without applying any flight changes.
    pub fn show_regions(&mut self) {
        self.visible = true;
        self.regions.show();
        self.credits_visible = false;
        self.coordinate_field = None;
        self.coordinate_error.clear();
    }

    /// Open the first credits page, including from application startup.
    /// Dismiss any unfinished coordinate edit without applying it.
    pub fn show_credits(&mut self) {
        self.visible = true;
        self.regions.visible = false;
        self.credits_visible = true;
        self.credits_page = 0;
        self.coordinate_field = None;
        self.coordinate_error.clear();
    }

    /// Select without moving the aircraft or applying climate to the flight.
    pub fn select(&mut self, position: Geodetic, name: &str) -> bool {
        if map_point_from_geodetic(position).is_none() {
            return false;
        }
        self.selected = position;
        self.selected_name = bounded_ascii(name, 30, 1);
        self.coordinate_field = None;
        self.coordinate_error.clear();
        true
    }

    /// Invalid externally supplied months fall back to January for presentation.
    #[must_use]
    pub fn preview_month(&self) -> u8 {
        if (1..=12).contains(&self.month) {
            self.month
        } else {
            1
        }
    }

    fn shift_month(&mut self, direction: i8, actions: &mut WorldMapActions) {
        let month = i16::from(self.preview_month()) - 1;
        self.month = u8::try_from((month + i16::from(direction)).rem_euclid(12) + 1)
            .expect("wrapped month is in 1..=12");
        actions.month_changed = Some(self.month);
    }

    fn begin_coordinate(&mut self, field: CoordinateField) {
        self.coordinate_field = Some(field);
        self.coordinate_draft = format!(
            "{:.5}",
            match field {
                CoordinateField::Latitude => self.selected.latitude.to_degrees().get(),
                CoordinateField::Longitude => self.selected.longitude.to_degrees().get(),
            }
        );
        self.coordinate_error.clear();
    }

    fn commit_coordinate(&mut self) -> bool {
        let Some(field) = self.coordinate_field else {
            return true;
        };
        let parsed = self
            .coordinate_draft
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite());
        let limit = match field {
            CoordinateField::Latitude => 90.0,
            CoordinateField::Longitude => 180.0,
        };
        let Some(value) = parsed.filter(|n| (-limit..=limit).contains(n)) else {
            self.coordinate_error = format!("Enter a number from -{limit:.0} to +{limit:.0}");
            return false;
        };
        match field {
            CoordinateField::Latitude => self.selected.latitude = Degrees(value).to_radians(),
            CoordinateField::Longitude => self.selected.longitude = Degrees(value).to_radians(),
        }
        self.selected_name = "Custom point".to_owned();
        self.coordinate_field = None;
        self.coordinate_error.clear();
        true
    }

    fn request_start(&self, actions: &mut WorldMapActions) {
        if self.navigation_enabled && map_point_from_geodetic(self.selected).is_some() {
            actions.start_at = Some(WorldMapStart {
                position: self.selected,
                month: self.preview_month(),
            });
        }
    }
}

/// App consumes with `take()`. No unbounded event queue or implicit flight move.
#[derive(Resource, Debug, Clone, Default)]
pub struct WorldMapActions {
    pub regions: RegionsActions,
    pub start_at: Option<WorldMapStart>,
    /// A preview notification only. Do not apply it to the current flight.
    pub month_changed: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMapStart {
    pub position: Geodetic,
    pub month: u8,
}

/// One fixed-size image, rebuilt by the app only for new layer/month data.
/// No tile IO, downloads, raster resolution growth or world dependency lives here.
#[derive(Resource, Debug, Clone)]
pub struct WorldMapRaster {
    pixels: Vec<u8>,
    pub layer: WorldMapLayer,
    pub month: u8,
    pub ready: bool,
}

impl Default for WorldMapRaster {
    fn default() -> Self {
        let mut raster = Self {
            pixels: vec![0; RASTER_BYTES],
            layer: WorldMapLayer::Terrain,
            month: 7,
            ready: false,
        };
        for pixel in raster.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[18, 35, 52, 255]);
        }
        raster
    }
}

impl WorldMapRaster {
    /// Samples pixel centers exactly once in row-major order. Coordinates span
    /// every longitude and latitude band without duplicating either seam edge.
    /// `sample` returns sRGB RGBA bytes; the app owns all source/color semantics.
    #[must_use]
    pub fn from_sampler(
        layer: WorldMapLayer,
        month: u8,
        mut sample: impl FnMut(Geodetic) -> [u8; 4],
    ) -> Self {
        let mut pixels = Vec::with_capacity(RASTER_BYTES);
        for y in 0..WORLD_MAP_HEIGHT {
            for x in 0..WORLD_MAP_WIDTH {
                let position = Geodetic::new(
                    Degrees(90.0 - (f64::from(y) + 0.5) * 180.0 / f64::from(WORLD_MAP_HEIGHT))
                        .to_radians(),
                    Degrees((f64::from(x) + 0.5) * 360.0 / f64::from(WORLD_MAP_WIDTH) - 180.0)
                        .to_radians(),
                    Meters::ZERO,
                );
                pixels.extend_from_slice(&sample(position));
            }
        }
        Self {
            pixels,
            layer,
            month,
            ready: true,
        }
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    fn image(&self) -> Image {
        let mut image = Image::new(
            Extent3d {
                width: WORLD_MAP_WIDTH,
                height: WORLD_MAP_HEIGHT,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.pixels.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::nearest();
        image
    }
}

#[derive(Component, Debug)]
pub struct WorldMapRoot;
#[derive(Component, Debug)]
struct WorldMapBody;
#[derive(Component, Debug)]
struct WorldMapColumn;
#[derive(Component, Debug)]
struct WorldMapSidebar;
#[derive(Component, Debug)]
pub struct WorldMapCanvas;
#[derive(Component, Debug)]
pub struct WorldMapCreditsRoot;
#[derive(Component, Debug)]
pub struct WorldMapImage;
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapText {
    Regions(RegionsText),
    Month,
    Selection,
    Latitude,
    Longitude,
    Details,
    Status,
    Legend,
    CoordinateHint,
    Navigation,
    Credits,
    CreditsPage,
}
#[derive(Component, Debug, Clone, Copy)]
pub enum WorldMapMarker {
    Aircraft,
    Selected,
}
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapButton {
    OpenRegions,
    Regions(RegionsButton),
    Close,
    OpenCredits,
    CloseCredits,
    PreviousCreditsPage,
    NextCreditsPage,
    Start,
    Aircraft,
    PreviousMonth,
    NextMonth,
    Layer(WorldMapLayer),
    Destination(usize),
    Latitude,
    Longitude,
}

/// Spawn an opaque modal over existing HUDs. The map keeps a 2:1 aspect ratio;
/// a 308-pixel sidebar and bounded descriptions target a 1280x720 viewport.
pub fn spawn_world_map(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    raster: Res<WorldMapRaster>,
) {
    let image = images.add(raster.image());
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            padding: UiRect::all(px(MAP_ROOT_PADDING)),
            flex_direction: FlexDirection::Column,
            row_gap: px(MAP_ROOT_GAP),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.025, 0.045, 0.068)),
        GlobalZIndex(100),
        Visibility::Hidden,
        FocusPolicy::Block,
        WorldMapRoot,
        Name::new("World map"),
    )).with_children(|root| {
        root.spawn(Node {
            width: percent(100.0), height: px(MAP_HEADER_HEIGHT), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween,
            ..default()
        }).with_children(|header| {
            header.spawn((Text::new("WORLD EXPLORER"), TextFont { font_size: 25.0, ..default() }, TextColor(TEXT)));
            header.spawn((Text::new("M / Esc: return to flight"), TextFont { font_size: 14.0, ..default() }, TextColor(MUTED)));
            spawn_button(header, "Regions [G]", WorldMapButton::OpenRegions, px(100.0));
            spawn_button(header, "Data credits", WorldMapButton::OpenCredits, px(118.0));
            spawn_button(header, "Close", WorldMapButton::Close, px(90.0));
        });
        root.spawn((Node {
            width: percent(100.0), flex_grow: 1.0, min_height: px(0.0),
            column_gap: px(MAP_BODY_GAP), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center, ..default()
        }, WorldMapBody)).with_children(|body| {
            body.spawn((Node {
                flex_grow: 1.0, flex_basis: px(0.0), min_width: px(0.0),
                max_width: px(MAP_COLUMN_MAX_WIDTH),
                flex_direction: FlexDirection::Column, row_gap: px(MAP_COLUMN_GAP), ..default()
            }, WorldMapColumn)).with_children(|map_column| {
                map_column.spawn((Text::new("90 N     CLICK TO SELECT A DEPARTURE POINT"), TextFont { font_size: 13.0, ..default() }, TextColor(MUTED)));
                map_column.spawn((
                    Node { width: percent(100.0), aspect_ratio: Some(2.0), overflow: Overflow::clip(), ..default() },
                    ImageNode::new(image), RelativeCursorPosition::default(), Interaction::None,
                    WorldMapCanvas, WorldMapImage, FocusPolicy::Block,
                )).with_children(|map| {
                    // Dimensionless graticule; geography is exclusively app-fed.
                    for longitude in 1_u16..12 {
                        map.spawn((Node {
                            position_type: PositionType::Absolute, left: percent(f32::from(longitude) * 100.0 / 12.0),
                            top: px(0.0), height: percent(100.0), width: px(1.0), ..default()
                        }, BackgroundColor(Color::srgba(0.8, 0.9, 1.0, 0.10)), FocusPolicy::Pass));
                    }
                    for latitude in 1_u16..6 {
                        map.spawn((Node {
                            position_type: PositionType::Absolute, top: percent(f32::from(latitude) * 100.0 / 6.0),
                            left: px(0.0), width: percent(100.0), height: px(1.0), ..default()
                        }, BackgroundColor(Color::srgba(0.8, 0.9, 1.0, if latitude == 3 { 0.32 } else { 0.10 })), FocusPolicy::Pass));
                    }
                    for (index, destination) in WORLD_MAP_DESTINATIONS.iter().enumerate() {
                        let point = map_point_from_geodetic(destination.position()).expect("valid destination");
                        map.spawn((
                            Text::new((index + 1).to_string()), TextFont { font_size: 13.0, ..default() },
                            TextColor(Color::srgb(0.94, 0.95, 0.88)),
                            BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.8)),
                            marker_node(point, 14.0), FocusPolicy::Pass,
                        ));
                    }
                    map.spawn((Text::new("+"), TextFont { font_size: 28.0, ..default() }, TextColor(Color::srgb(1.0, 0.75, 0.22)), marker_node(MapPoint { x: 0.5, y: 0.5 }, 28.0), WorldMapMarker::Selected, FocusPolicy::Pass));
                    map.spawn((Text::new("A"), TextFont { font_size: 18.0, ..default() }, TextColor(ACCENT), BackgroundColor(Color::srgb(0.02, 0.08, 0.10)), marker_node(MapPoint { x: 0.5, y: 0.5 }, 20.0), WorldMapMarker::Aircraft, FocusPolicy::Pass));
                });
                map_column.spawn(Node { width: percent(100.0), justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|axis| {
                    for label in ["180 W", "90 W", "0", "90 E", "180 E"] {
                        axis.spawn((Text::new(label), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED)));
                    }
                });
                map_column.spawn((Text::new("90 S      A  aircraft       +  selected       1-8  destinations"), TextFont { font_size: 13.0, ..default() }, TextColor(MUTED)));
                spawn_map_text(map_column, WorldMapText::Legend, 13.0, TEXT);
                spawn_map_text(map_column, WorldMapText::Status, 12.0, MUTED);
            });
            body.spawn((Node {
                // Resolve height against the bounded body. An auto-height column
                // containing percentage-width text gets an inflated intrinsic
                // cross size in Taffy, despite its final children fitting.
                width: px(SIDEBAR_WIDTH), height: percent(100.0), min_height: px(0.0),
                flex_shrink: 0.0, flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(SIDEBAR_PADDING)), row_gap: px(SIDEBAR_GAP), ..default()
            }, BackgroundColor(Color::srgb(0.045, 0.073, 0.103)), WorldMapSidebar)).with_children(|side| {
                side.spawn((Text::new("EXPLORE A REGION"), TextFont { font_size: 14.0, ..default() }, TextColor(ACCENT)));
                side.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6.0), row_gap: px(6.0), ..default() }).with_children(|places| {
                    for (index, destination) in WORLD_MAP_DESTINATIONS.iter().enumerate() {
                        spawn_button(places, &format!("{}  {}", index + 1, destination.name), WorldMapButton::Destination(index), px(137.0));
                    }
                });
                spawn_map_text(side, WorldMapText::Selection, 17.0, TEXT);
                spawn_coordinate_button(side, WorldMapButton::Latitude, WorldMapText::Latitude);
                spawn_coordinate_button(side, WorldMapButton::Longitude, WorldMapText::Longitude);
                spawn_map_text(side, WorldMapText::CoordinateHint, 11.0, MUTED);
                side.spawn(Node { column_gap: px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|month| {
                    spawn_button(month, "<", WorldMapButton::PreviousMonth, px(34.0));
                    month.spawn((Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() },)).with_children(|label| {
                        spawn_map_text(label, WorldMapText::Month, 15.0, TEXT);
                    });
                    spawn_button(month, ">", WorldMapButton::NextMonth, px(34.0));
                });
                side.spawn(Node { column_gap: px(6.0), ..default() }).with_children(|layers| {
                    spawn_button(layers, "Terrain", WorldMapButton::Layer(WorldMapLayer::Terrain), px(137.0));
                    spawn_button(layers, "Climate", WorldMapButton::Layer(WorldMapLayer::Climate), px(137.0));
                });
                spawn_map_text(side, WorldMapText::Details, DETAILS_FONT_SIZE, TEXT);
                spawn_button(side, "Select aircraft position", WorldMapButton::Aircraft, percent(100.0));
                spawn_button(side, "START NEW FLIGHT  [Enter]", WorldMapButton::Start, percent(100.0));
                spawn_map_text(side, WorldMapText::Navigation, 11.0, MUTED);
            });
        });
        root.spawn((Text::new("Preview only until Start. A new flight resets the current flight recording.    M / Esc closes this map."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { height: px(MAP_FOOTER_HEIGHT), flex_shrink: 0.0, ..default() }));
        regions::spawn_regions(root);
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0), top: px(0.0), width: percent(100.0), height: percent(100.0),
                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.96)),
            GlobalZIndex(110), FocusPolicy::Block, Visibility::Hidden,
            WorldMapCreditsRoot,
        )).with_children(|overlay| {
            overlay.spawn((Node {
                width: percent(80.0), height: percent(82.0), padding: UiRect::all(px(22.0)),
                flex_direction: FlexDirection::Column, row_gap: px(14.0),
                overflow: Overflow::clip(), ..default()
            }, BackgroundColor(Color::srgb(0.045, 0.073, 0.103)))).with_children(|panel| {
                panel.spawn(Node {
                    width: percent(100.0), min_height: px(32.0), flex_shrink: 0.0,
                    align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween,
                    ..default()
                }).with_children(|header| {
                    header.spawn((Text::new("WORLD DATA CREDITS"), TextFont { font_size: 22.0, ..default() }, TextColor(TEXT)));
                    spawn_button(header, "Close credits", WorldMapButton::CloseCredits, px(120.0));
                });
                panel.spawn(Node { width: percent(100.0), flex_grow: 1.0, min_height: px(0.0), ..default() }).with_children(|body| {
                    spawn_map_text(body, WorldMapText::Credits, 14.0, TEXT);
                });
                panel.spawn(Node {
                    width: percent(100.0), min_height: px(32.0), flex_shrink: 0.0,
                    justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center,
                    ..default()
                }).with_children(|footer| {
                    spawn_button(footer, "Previous page", WorldMapButton::PreviousCreditsPage, px(120.0));
                    footer.spawn(Node { width: px(140.0), ..default() }).with_children(|page| {
                        spawn_map_text(page, WorldMapText::CreditsPage, 14.0, MUTED);
                    });
                    spawn_button(footer, "Next page", WorldMapButton::NextCreditsPage, px(120.0));
                });
                panel.spawn((Text::new("Esc: return to map. Source versions and complete licence details are also in ATTRIBUTION.md."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
            });
        });
    });
}

fn spawn_map_text(parent: &mut ChildSpawnerCommands, kind: WorldMapText, size: f32, color: Color) {
    let line_height = if kind == WorldMapText::Details {
        DETAILS_LINE_HEIGHT
    } else {
        size * 1.2
    };
    parent.spawn((
        Text::new(""),
        TextFont {
            font_size: size,
            ..default()
        },
        LineHeight::Px(line_height),
        TextColor(color),
        kind,
        Node {
            width: percent(100.0),
            ..default()
        },
        FocusPolicy::Pass,
    ));
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    button: WorldMapButton,
    width: Val,
) {
    parent
        .spawn((
            Button,
            Node {
                width,
                min_height: px(MAP_BUTTON_HEIGHT),
                padding: UiRect::axes(px(5.0), px(5.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BUTTON),
            button,
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(TEXT),
                FocusPolicy::Pass,
            ));
        });
}

fn spawn_coordinate_button(
    parent: &mut ChildSpawnerCommands,
    button: WorldMapButton,
    kind: WorldMapText,
) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                min_height: px(MAP_BUTTON_HEIGHT),
                padding: UiRect::all(px(5.0)),
                ..default()
            },
            BackgroundColor(BUTTON),
            button,
        ))
        .with_children(|field| {
            spawn_map_text(field, kind, 13.0, TEXT);
        });
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "normalized UI coordinates are bounded to 0..=1 before f32 rendering"
)]
fn marker_node(point: MapPoint, size: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent((point.x * 100.0) as f32),
        top: percent((point.y * 100.0) as f32),
        margin: UiRect {
            left: px(-size / 2.0),
            top: px(-size / 2.0),
            ..default()
        },
        width: px(size),
        height: px(size),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// Input is local to this modal. App must gate pilot/camera input before it can
/// see these same key presses. Escape never toggles the app's pause flag here.
#[expect(
    clippy::too_many_arguments,
    reason = "ordered keyboard messages and modifier history supplement the modal's typed input queries"
)]
pub fn handle_world_map_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut logical_keys: Option<ResMut<ButtonInput<Key>>>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
    mut control_modifiers: Local<[bool; 2]>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<WorldMapState>,
    mut actions: ResMut<WorldMapActions>,
    buttons: Query<(&Interaction, &WorldMapButton), Changed<Interaction>>,
    canvases: Query<&RelativeCursorPosition, With<WorldMapCanvas>>,
) {
    // ButtonInput loses event order and duplicate presses. Keep modifier history
    // from the ordered message stream, including while the editor/map is hidden.
    // A whole-frame focus-loss discard also avoids stale Ctrl after Alt-Tab.
    let lost_focus = focus_lost.read().count() > 0;
    if lost_focus {
        *control_modifiers = [false; 2];
    }
    let editing_allowed = !lost_focus
        && state.visible
        && !state.credits_visible
        && !state.regions.visible
        && !keys.just_pressed(KeyCode::KeyM)
        && !keys.just_pressed(KeyCode::Escape)
        && !buttons.iter().any(|(interaction, button)| {
            *interaction == Interaction::Pressed
                && matches!(
                    button,
                    WorldMapButton::Close
                        | WorldMapButton::OpenCredits
                        | WorldMapButton::OpenRegions
                )
        });
    // Bevy resolves pointer Interaction in PreUpdate. Establish that frame's
    // clicked field before consuming its keyboard messages, so a quick
    // click -> Ctrl+A -> typing batch neither loses the clear nor edits the
    // previously focused field. Modal close/open actions retain priority.
    if editing_allowed {
        for (interaction, button) in &buttons {
            if *interaction == Interaction::Pressed {
                match button {
                    WorldMapButton::Latitude => state.begin_coordinate(CoordinateField::Latitude),
                    WorldMapButton::Longitude => state.begin_coordinate(CoordinateField::Longitude),
                    _ => {}
                }
            }
        }
    }
    let mut coordinate_enter = false;
    for event in keyboard.read() {
        if lost_focus {
            continue;
        }
        let pressed = event.state == ButtonState::Pressed;
        match event.key_code {
            KeyCode::ControlLeft => control_modifiers[0] = pressed,
            KeyCode::ControlRight => control_modifiers[1] = pressed,
            _ => {}
        }
        if editing_allowed && state.coordinate_field.is_some() {
            coordinate_enter |= edit_coordinate(
                event,
                control_modifiers.iter().any(|held| *held),
                &mut state,
            );
        }
    }
    if lost_focus {
        return;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        state.visible = !state.visible;
        regions::consume_keyboard_shortcuts(&mut keys, logical_keys.as_deref_mut());
        state.regions.dismiss(&mut actions.regions);
        state.credits_visible = false;
        state.coordinate_field = None;
        state.coordinate_error.clear();
        actions.start_at = None;
        consume_start_keys(&mut keys);
        keys.clear_just_pressed(KeyCode::KeyM);
        keys.clear_just_pressed(KeyCode::Escape);
        return;
    }
    if !state.visible {
        return;
    }
    if state.regions.visible {
        let keyboard_button = regions::keyboard_button(&keys, logical_keys.as_deref());
        regions::consume_keyboard_shortcuts(&mut keys, logical_keys.as_deref_mut());
        // The complete frame belongs to the panel, including its close frame.
        // Consume Enter so returning to the map cannot submit a stale start.
        actions.start_at = None;
        consume_start_keys(&mut keys);
        let close = keys.just_pressed(KeyCode::Escape)
            || buttons.iter().any(|(interaction, button)| {
                *interaction == Interaction::Pressed
                    && *button == WorldMapButton::Regions(RegionsButton::Close)
            });
        if close {
            state.regions.dismiss(&mut actions.regions);
            keys.clear_just_pressed(KeyCode::Escape);
            return;
        }
        // Cancel wins even when a selection/refresh is pressed in this frame.
        if keyboard_button == Some(RegionsButton::Cancel)
            || buttons.iter().any(|(interaction, button)| {
                *interaction == Interaction::Pressed
                    && *button == WorldMapButton::Regions(RegionsButton::Cancel)
            })
        {
            state
                .regions
                .act(RegionsButton::Cancel, &mut actions.regions);
            return;
        }
        if let Some(button) = keyboard_button {
            state.regions.act(button, &mut actions.regions);
            return;
        }
        for (interaction, button) in &buttons {
            if *interaction == Interaction::Pressed
                && let WorldMapButton::Regions(button) = button
            {
                state.regions.act(*button, &mut actions.regions);
            }
        }
        return;
    }
    if state.credits_visible {
        regions::consume_keyboard_shortcuts(&mut keys, logical_keys.as_deref_mut());
        actions.start_at = None;
        consume_start_keys(&mut keys);
        let close = keys.just_pressed(KeyCode::Escape)
            || buttons.iter().any(|(interaction, button)| {
                *interaction == Interaction::Pressed && *button == WorldMapButton::CloseCredits
            });
        if close {
            state.credits_visible = false;
            keys.clear_just_pressed(KeyCode::Escape);
            return;
        }
        let page_count = credit_pages(&state.source_credits).len();
        state.credits_page = state.credits_page.min(page_count - 1);
        for (interaction, button) in &buttons {
            if *interaction != Interaction::Pressed {
                continue;
            }
            match button {
                WorldMapButton::PreviousCreditsPage => {
                    state.credits_page = state.credits_page.saturating_sub(1)
                }
                WorldMapButton::NextCreditsPage => {
                    state.credits_page = state.credits_page.saturating_add(1).min(page_count - 1)
                }
                _ => {}
            }
        }
        // Credits capture all other keys and map/sidebar interactions.
        return;
    }
    let close_button = buttons.iter().any(|(interaction, button)| {
        *interaction == Interaction::Pressed && *button == WorldMapButton::Close
    });
    if keys.just_pressed(KeyCode::Escape) || close_button {
        regions::consume_keyboard_shortcuts(&mut keys, logical_keys.as_deref_mut());
        state.visible = false;
        state.regions.dismiss(&mut actions.regions);
        state.coordinate_field = None;
        state.coordinate_error.clear();
        actions.start_at = None;
        consume_start_keys(&mut keys);
        // App pause handlers must not also act on the map's close key.
        keys.clear_just_pressed(KeyCode::Escape);
        return;
    }
    if (state.coordinate_field.is_none()
        && !coordinate_enter
        && regions::shortcuts_allowed(&keys, logical_keys.as_deref())
        && keys.just_pressed(KeyCode::KeyG))
        || buttons.iter().any(|(interaction, button)| {
            *interaction == Interaction::Pressed && *button == WorldMapButton::OpenRegions
        })
    {
        regions::consume_keyboard_shortcuts(&mut keys, logical_keys.as_deref_mut());
        state.show_regions();
        actions.start_at = None;
        consume_start_keys(&mut keys);
        return;
    }
    if buttons.iter().any(|(interaction, button)| {
        *interaction == Interaction::Pressed && *button == WorldMapButton::OpenCredits
    }) {
        state.show_credits();
        actions.start_at = None;
        consume_start_keys(&mut keys);
        return;
    }
    // Even a repeated Enter in the same message batch must never submit a
    // coordinate and then also start a new flight.
    if coordinate_enter {
        return;
    }
    if state.coordinate_field.is_none()
        && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter))
    {
        state.request_start(&mut actions);
    }
    if mouse.just_pressed(MouseButton::Left) {
        for cursor in &canvases {
            if cursor.cursor_over
                && let Some(relative) = cursor.normalized
            {
                // Bevy 0.18 uses centered [-0.5, 0.5], not [0, 1].
                let point = MapPoint {
                    x: f64::from(relative.x) + 0.5,
                    y: f64::from(relative.y) + 0.5,
                };
                if let Some(position) = geodetic_from_map_point(point) {
                    state.select(position, "Custom point");
                }
            }
        }
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *button {
            WorldMapButton::Close
            | WorldMapButton::OpenRegions
            | WorldMapButton::Regions(_)
            | WorldMapButton::OpenCredits
            | WorldMapButton::CloseCredits
            | WorldMapButton::PreviousCreditsPage
            | WorldMapButton::NextCreditsPage => {} // handled first
            WorldMapButton::Start => {
                if state.commit_coordinate() {
                    state.request_start(&mut actions);
                }
            }
            WorldMapButton::Aircraft => {
                if let Some(position) = state.aircraft {
                    state.select(position, "Aircraft position");
                }
            }
            WorldMapButton::PreviousMonth => state.shift_month(-1, &mut actions),
            WorldMapButton::NextMonth => state.shift_month(1, &mut actions),
            WorldMapButton::Layer(layer) => state.layer = layer,
            WorldMapButton::Destination(index) => {
                if let Some(destination) = WORLD_MAP_DESTINATIONS.get(index) {
                    state.select(destination.position(), destination.name);
                }
            }
            // Focus was already established before this frame's keyboard batch.
            WorldMapButton::Latitude | WorldMapButton::Longitude => {}
        }
    }
}

fn consume_start_keys(keys: &mut ButtonInput<KeyCode>) {
    keys.clear_just_pressed(KeyCode::Enter);
    keys.clear_just_pressed(KeyCode::NumpadEnter);
}

/// Apply one ordered key event. The bool means an editor Enter was handled,
/// even if validation failed; it must not also become the map's Start shortcut.
fn edit_coordinate(
    event: &KeyboardInput,
    control_pressed: bool,
    state: &mut WorldMapState,
) -> bool {
    if event.state != ButtonState::Pressed {
        return false;
    }
    if matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
        state.commit_coordinate();
        return true;
    }
    if control_pressed {
        if event.key_code == KeyCode::KeyA
            || matches!(&event.logical_key, Key::Character(character) if character.eq_ignore_ascii_case("a"))
        {
            state.coordinate_draft.clear();
            state.coordinate_error.clear();
        }
        return false;
    }
    match event.key_code {
        KeyCode::Backspace => {
            state.coordinate_draft.pop();
            state.coordinate_error.clear();
            return false;
        }
        KeyCode::Delete => {
            state.coordinate_draft.clear();
            state.coordinate_error.clear();
            return false;
        }
        _ => {}
    }
    // Text honors the keyboard layout and can contain multiple characters.
    // Logical Character is a fallback for platforms that omit the text field.
    // Do not fall back to physical digit keys: NumLock-off navigation and
    // shifted/localized keys must not silently insert unrelated digits.
    let text = event.text.as_deref().or(match &event.logical_key {
        Key::Character(character) => Some(character.as_str()),
        _ => None,
    });
    if let Some(text) = text {
        state.coordinate_error.clear();
        for character in text.chars() {
            if state.coordinate_draft.len() >= 16 {
                // Truncating an otherwise valid decimal could change its value.
                // Keep the bounded draft visibly invalid until explicitly edited.
                state.coordinate_draft.pop();
                state.coordinate_draft.push('?');
                state.coordinate_error = "Input too long; clear or edit the value".to_owned();
                break;
            }
            let normalized = match character {
                '0'..='9' | '+' | '-' | '.' => character,
                '\u{2212}' | '\u{ff0d}' => '-',
                ',' => '.',
                // Never drop a sign, separator, letter or control character
                // from a multi-character payload and then commit another number.
                // '?' stays ASCII and makes numeric parsing fail safely.
                _ => '?',
            };
            state.coordinate_draft.push(normalized);
        }
        if state.coordinate_draft.contains('?') && state.coordinate_error.is_empty() {
            state.coordinate_error = "Use digits, +/- and . or ,".to_owned();
        }
    }
    false
}

// Keep the credits visibility write provably disjoint from the map root and
// marker visibility writes in the same system. The alias names that ECS filter
// instead of suppressing complexity across the rest of the presentation code.
type CreditsVisibilityFilter = (
    With<WorldMapCreditsRoot>,
    Without<WorldMapRoot>,
    Without<WorldMapMarker>,
    Without<WorldMapRegionsRoot>,
);

type RegionsVisibilityFilter = (
    With<WorldMapRegionsRoot>,
    Without<WorldMapRoot>,
    Without<WorldMapMarker>,
);

/// Upload changed raster once, then update text/markers without resampling world
/// data. App should publish selected descriptions before this system.
#[expect(
    clippy::too_many_arguments,
    reason = "modal presentation coordinates separate typed UI queries and one image asset"
)]
pub fn update_world_map(
    state: Res<WorldMapState>,
    raster: Res<WorldMapRaster>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<&mut Visibility, (With<WorldMapRoot>, Without<WorldMapMarker>)>,
    mut credits: Query<&mut Visibility, CreditsVisibilityFilter>,
    mut region_panels: Query<&mut Visibility, RegionsVisibilityFilter>,
    map_images: Query<&ImageNode, With<WorldMapImage>>,
    mut texts: Query<(&WorldMapText, &mut Text)>,
    mut markers: Query<(&WorldMapMarker, &mut Node, &mut Visibility), Without<WorldMapRoot>>,
    mut buttons: Query<(&WorldMapButton, &Interaction, &mut BackgroundColor)>,
) {
    for mut visibility in &mut roots {
        *visibility = if state.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut visibility in &mut credits {
        *visibility = if state.credits_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut visibility in &mut region_panels {
        *visibility = if state.regions.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if raster.is_changed() {
        for node in &map_images {
            if let Some(image) = images.get_mut(&node.image) {
                image.data = Some(raster.pixels.clone());
            }
        }
    }
    if !state.visible {
        return;
    }
    for (kind, mut text) in &mut texts {
        let formatted = format_world_map_text(*kind, &state, &raster);
        if text.as_str() != formatted {
            **text = formatted;
        }
    }
    for (marker, mut node, mut visibility) in &mut markers {
        let (position, size) = match marker {
            WorldMapMarker::Selected => (Some(state.selected), 28.0),
            WorldMapMarker::Aircraft => (state.aircraft, 20.0),
        };
        if let Some(point) = position.and_then(map_point_from_geodetic) {
            *node = marker_node(point, size);
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    let last_credit_page = credit_pages(&state.source_credits).len() - 1;
    for (button, interaction, mut color) in &mut buttons {
        let selected = matches!(button, WorldMapButton::Layer(layer) if *layer == state.layer)
            || matches!(button, WorldMapButton::Regions(button) if state.regions.button_selected(*button))
            || matches!(
                (button, state.coordinate_field),
                (WorldMapButton::Latitude, Some(CoordinateField::Latitude))
                    | (WorldMapButton::Longitude, Some(CoordinateField::Longitude))
            );
        let disabled = (*button == WorldMapButton::Start && !state.navigation_enabled)
            || matches!(button, WorldMapButton::Regions(button) if state.regions.button_disabled(*button))
            || (*button == WorldMapButton::PreviousCreditsPage && state.credits_page == 0)
            || (*button == WorldMapButton::NextCreditsPage
                && state.credits_page >= last_credit_page);
        color.0 = if disabled {
            Color::srgb(0.09, 0.10, 0.12)
        } else if *interaction == Interaction::Pressed {
            Color::srgb(0.11, 0.42, 0.47)
        } else if selected {
            Color::srgb(0.09, 0.31, 0.36)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.17, 0.27, 0.34)
        } else if *button == WorldMapButton::Start {
            Color::srgb(0.08, 0.33, 0.31)
        } else {
            BUTTON
        };
    }
}

#[must_use]
pub fn format_world_map_text(
    kind: WorldMapText,
    state: &WorldMapState,
    raster: &WorldMapRaster,
) -> String {
    match kind {
        WorldMapText::Regions(kind) => regions::format_regions_text(kind, &state.regions),
        WorldMapText::Month => format!(
            "{} (preview)",
            MONTHS[usize::from(state.preview_month() - 1)]
        ),
        WorldMapText::Selection => bounded_ascii(&state.selected_name, 26, 1),
        WorldMapText::Latitude => coordinate_text(state, CoordinateField::Latitude),
        WorldMapText::Longitude => coordinate_text(state, CoordinateField::Longitude),
        WorldMapText::Details => format!(
            "{}\n{}",
            bounded_ascii(
                &state.selected_terrain,
                DETAILS_COLUMNS,
                TERRAIN_DETAIL_LINES
            ),
            bounded_ascii(
                &state.selected_climate,
                DETAILS_COLUMNS,
                CLIMATE_DETAIL_LINES
            )
        ),
        WorldMapText::Status => {
            let raster_status = if raster.ready
                && raster.layer == state.layer
                && raster.month == state.preview_month()
            {
                "Coarse global terrain / local DEM when available"
            } else {
                "Map preview loading; geographic data pending"
            };
            format!(
                "{raster_status}\n1991-2020 NOAA reanalysis climatology; not live weather\nClimate-derived cues; not satellite imagery or measured landcover/snow"
            )
        }
        WorldMapText::Legend => match state.layer {
            WorldMapLayer::Terrain => {
                "TERRAIN  Blue: water   Sand: dry/warm   Green: wet   White: snow cue".to_owned()
            }
            WorldMapLayer::Climate => {
                "CLIMATE  Blue: cold   Cream: temperate   Red: hot   (monthly mean)".to_owned()
            }
        },
        WorldMapText::CoordinateHint => {
            if !state.coordinate_error.is_empty() {
                return bounded_ascii(&state.coordinate_error, 42, 1);
            }
            if state.coordinate_field.is_some() {
                "Type degrees; Ctrl+A clears; Enter applies".to_owned()
            } else {
                "Click LAT / LON to enter exact coordinates".to_owned()
            }
        }
        WorldMapText::Navigation => bounded_ascii(&state.navigation_note, 42, 2),
        WorldMapText::Credits => {
            let pages = credit_pages(&state.source_credits);
            pages[state.credits_page.min(pages.len() - 1)].clone()
        }
        WorldMapText::CreditsPage => {
            let count = credit_pages(&state.source_credits).len();
            format!("Page {} / {count}", state.credits_page.min(count - 1) + 1)
        }
    }
}

/// Credits retain every word (up to the defensive app-input limit) and paginate;
/// long legal statements must never disappear behind the sidebar truncation.
fn credit_pages(credits: &str) -> Vec<String> {
    let mut characters = credits.chars();
    let mut sanitized: String = characters
        .by_ref()
        .take(MAX_CREDIT_CHARACTERS)
        .map(|character| {
            if character == '\n' || (character.is_ascii() && !character.is_control()) {
                character
            } else {
                ' '
            }
        })
        .collect();
    if characters.next().is_some() {
        sanitized.push_str("\n\nAdditional credits exceed the display limit. See ATTRIBUTION.md for the complete text.");
    }
    if sanitized.trim().is_empty() {
        sanitized = "Data credits are supplied by the application.".to_owned();
    }
    let mut lines = Vec::new();
    for paragraph in sanitized.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_ascii_whitespace() {
            if !line.is_empty() && line.len() + word.len() + 1 > CREDIT_COLUMNS {
                lines.push(std::mem::take(&mut line));
            }
            // Long URLs/identifiers have no word boundaries. Preserve every
            // byte and split them explicitly instead of widening the panel.
            let mut remainder = word;
            while remainder.len() > CREDIT_COLUMNS {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                lines.push(remainder[..CREDIT_COLUMNS].to_owned());
                remainder = &remainder[CREDIT_COLUMNS..];
            }
            if !remainder.is_empty() {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(remainder);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
        .chunks(CREDIT_LINES_PER_PAGE)
        .map(|page| page.join("\n"))
        .collect()
}

fn coordinate_text(state: &WorldMapState, field: CoordinateField) -> String {
    let label = match field {
        CoordinateField::Latitude => "LAT",
        CoordinateField::Longitude => "LON",
    };
    if state.coordinate_field == Some(field) {
        return format!("{label}  {}_", state.coordinate_draft);
    }
    let value = match field {
        CoordinateField::Latitude => state.selected.latitude.to_degrees().get(),
        CoordinateField::Longitude => state.selected.longitude.to_degrees().get(),
    };
    if !value.is_finite() {
        return format!("{label}  unavailable");
    }
    format!("{label}  {value:+.5} deg")
}

fn bounded_ascii(text: &str, columns: usize, lines: usize) -> String {
    text.lines()
        .take(lines)
        .map(|line| {
            let mut result: String = line
                .chars()
                .take(columns)
                .map(|character| {
                    if character.is_ascii() && !character.is_control() {
                        character
                    } else {
                        '?'
                    }
                })
                .collect();
            if line.chars().count() > columns && result.len() >= 3 {
                result.truncate(columns - 3);
                result.push_str("...");
            }
            result
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }

    #[test]
    fn map_corners_and_equator_match_known_geographic_bounds() {
        for (x, y, latitude, longitude) in [
            (0.0, 0.0, 90.0, -180.0),
            (1.0, 0.0, 90.0, 180.0),
            (0.0, 1.0, -90.0, -180.0),
            (1.0, 1.0, -90.0, 180.0),
            (0.5, 0.5, 0.0, 0.0),
            (0.75, 0.5, 0.0, 90.0),
        ] {
            let location = geodetic_from_map_point(MapPoint { x, y }).unwrap();
            near(location.latitude.to_degrees().get(), latitude);
            near(location.longitude.to_degrees().get(), longitude);
            let point = map_point_from_geodetic(location).unwrap();
            near(point.x, x);
            near(point.y, y);
        }
    }

    #[test]
    fn real_destinations_round_trip_without_meridian_or_hemisphere_flip() {
        for destination in WORLD_MAP_DESTINATIONS {
            let location = destination.position();
            let point = map_point_from_geodetic(location).unwrap();
            let recovered = geodetic_from_map_point(point).unwrap();
            near(recovered.latitude.get(), location.latitude.get());
            near(recovered.longitude.get(), location.longitude.get());
        }
        let tokyo = map_point_from_geodetic(WORLD_MAP_DESTINATIONS[0].position()).unwrap();
        assert!(tokyo.x > 0.8 && tokyo.y < 0.4);
        let sydney = map_point_from_geodetic(WORLD_MAP_DESTINATIONS[5].position()).unwrap();
        assert!(sydney.x > 0.9 && sydney.y > 0.6);
    }

    #[test]
    fn map_projection_rejects_invalid_values_and_outside_clicks() {
        for point in [
            MapPoint {
                x: f64::NAN,
                y: 0.0,
            },
            MapPoint {
                x: 0.0,
                y: f64::INFINITY,
            },
            MapPoint { x: -0.01, y: 0.5 },
            MapPoint { x: 1.01, y: 0.5 },
            MapPoint { x: 0.5, y: -0.01 },
            MapPoint { x: 0.5, y: 1.01 },
        ] {
            assert!(geodetic_from_map_point(point).is_none());
        }
        for position in [
            Geodetic::from_degrees(91.0, 0.0, 0.0),
            Geodetic::from_degrees(-91.0, 0.0, 0.0),
            Geodetic::from_degrees(f64::NAN, 0.0, 0.0),
            Geodetic::from_degrees(0.0, f64::INFINITY, 0.0),
            Geodetic::from_degrees(0.0, 0.0, f64::NAN),
        ] {
            assert!(map_point_from_geodetic(position).is_none());
        }
    }

    #[test]
    fn core_longitude_wrapping_is_used_outside_the_dateline() {
        let west = map_point_from_geodetic(Geodetic::from_degrees(0.0, 270.0, 0.0)).unwrap();
        near(west.x, 0.25);
        let east = map_point_from_geodetic(Geodetic::from_degrees(0.0, -270.0, 0.0)).unwrap();
        near(east.x, 0.75);
    }

    #[test]
    fn raster_is_bounded_center_sampled_north_first_and_deterministic() {
        let mut positions = Vec::new();
        let raster = WorldMapRaster::from_sampler(WorldMapLayer::Terrain, 1, |position| {
            if positions.len() < 721 {
                positions.push(position);
            }
            [1, 2, 3, 255]
        });
        assert_eq!(raster.pixels().len(), RASTER_BYTES);
        near(positions[0].latitude.to_degrees().get(), 89.75);
        near(positions[0].longitude.to_degrees().get(), -179.75);
        near(positions[719].longitude.to_degrees().get(), 179.75);
        near(positions[720].latitude.to_degrees().get(), 89.25);
        let identical = WorldMapRaster::from_sampler(WorldMapLayer::Terrain, 1, |_| [1, 2, 3, 255]);
        assert_eq!(raster.pixels(), identical.pixels());
    }

    #[test]
    fn selecting_a_destination_never_requests_a_flight_or_changes_month() {
        let mut state = WorldMapState::default();
        let actions = WorldMapActions::default();
        assert!(state.select(WORLD_MAP_DESTINATIONS[5].position(), "Sydney"));
        assert_eq!(state.month, 7);
        assert!(actions.start_at.is_none());
        assert_eq!(state.selected_name, "Sydney");
        assert!(!state.select(Geodetic::from_degrees(100.0, 0.0, 0.0), "invalid"));
        assert_eq!(state.selected_name, "Sydney");
    }

    #[test]
    fn month_wraps_in_both_directions_and_is_preview_only() {
        let mut state = WorldMapState {
            month: 1,
            ..default()
        };
        let mut actions = WorldMapActions::default();
        state.shift_month(-1, &mut actions);
        assert_eq!(state.month, 12);
        assert_eq!(actions.month_changed, Some(12));
        assert!(actions.start_at.is_none());
        state.shift_month(1, &mut actions);
        assert_eq!(state.month, 1);
        assert_eq!(actions.month_changed, Some(1));
    }

    #[test]
    fn start_carries_the_selected_position_and_preview_month_once() {
        let state = WorldMapState {
            month: 2,
            ..default()
        };
        let mut actions = WorldMapActions::default();
        state.request_start(&mut actions);
        let request = actions.start_at.take().unwrap();
        assert_eq!(request.position, state.selected);
        assert_eq!(request.month, 2);
        assert!(actions.start_at.take().is_none());
        let blocked = WorldMapState {
            navigation_enabled: false,
            ..state
        };
        blocked.request_start(&mut actions);
        assert!(actions.start_at.is_none());
    }

    #[test]
    fn exact_coordinate_edit_validates_bounds_and_keeps_old_selection_on_error() {
        let mut state = WorldMapState::default();
        let old = state.selected;
        state.begin_coordinate(CoordinateField::Latitude);
        for invalid in ["NaN", "inf", "91", "-90.1", "", "--5"] {
            state.coordinate_draft = invalid.into();
            assert!(!state.commit_coordinate());
            assert_eq!(state.selected, old);
        }
        state.coordinate_draft = "-90".into();
        assert!(state.commit_coordinate());
        near(state.selected.latitude.to_degrees().get(), -90.0);
        state.begin_coordinate(CoordinateField::Longitude);
        state.coordinate_draft = "180".into();
        assert!(state.commit_coordinate());
        near(state.selected.longitude.to_degrees().get(), 180.0);
    }

    #[test]
    fn externally_supplied_text_is_ascii_and_bounded() {
        let state = WorldMapState {
            selected_name: "Tokyo 日本".into(),
            selected_terrain: "x".repeat(2000),
            selected_climate: "é\n".repeat(100),
            ..default()
        };
        for kind in [
            WorldMapText::Month,
            WorldMapText::Selection,
            WorldMapText::Latitude,
            WorldMapText::Longitude,
            WorldMapText::Details,
            WorldMapText::Status,
            WorldMapText::Legend,
            WorldMapText::CoordinateHint,
            WorldMapText::Navigation,
        ] {
            let text = format_world_map_text(kind, &state, &WorldMapRaster::default());
            assert!(text.is_ascii());
            assert!(text.len() < 400);
        }
        assert!(
            format_world_map_text(WorldMapText::Status, &state, &WorldMapRaster::default())
                .contains("not live weather")
        );
    }

    fn key_event(
        key_code: KeyCode,
        logical_key: Key,
        text: Option<&str>,
        state: ButtonState,
    ) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            text: text.map(Into::into),
            state,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn typed(key_code: KeyCode, text: &str) -> KeyboardInput {
        key_event(
            key_code,
            Key::Character(text.into()),
            Some(text),
            ButtonState::Pressed,
        )
    }

    fn send_keys(app: &mut App, keys: impl IntoIterator<Item = KeyboardInput>) {
        for key in keys {
            app.world_mut()
                .write_message(key)
                .expect("registered keyboard messages");
        }
    }

    fn coordinate_input_app() -> App {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.begin_coordinate(CoordinateField::Latitude);
            state.coordinate_draft.clear();
        }
        app
    }

    fn input_app() -> App {
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .add_message::<KeyboardFocusLost>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<WorldMapState>()
            .init_resource::<WorldMapActions>()
            .add_systems(Update, handle_world_map_input);
        app
    }

    #[test]
    fn modal_toggle_and_escape_do_not_leave_a_start_or_pause_key_behind() {
        let mut app = input_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        app.update();
        assert!(app.world().resource::<WorldMapState>().visible);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<WorldMapState>().visible);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Escape)
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn hidden_map_ignores_enter_buttons_and_mouse() {
        let mut app = input_app();
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Start));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn centered_bevy_cursor_selects_greenwich_equator_not_north_west() {
        let mut app = input_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        app.world_mut().spawn((
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            WorldMapCanvas,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let selected = app.world().resource::<WorldMapState>().selected;
        near(selected.latitude.get(), 0.0);
        near(selected.longitude.get(), 0.0);
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn coordinate_click_and_ctrl_a_typing_in_one_frame_keep_the_new_draft() {
        let mut app = input_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Latitude));
        let original = app.world().resource::<WorldMapState>().selected;
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
                typed(KeyCode::KeyA, "a"),
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Released,
                ),
                typed(KeyCode::Digit2, "2"),
                typed(KeyCode::Digit1, "1"),
                typed(KeyCode::Period, "."),
                typed(KeyCode::Digit1, "1"),
                typed(KeyCode::Digit1, "1"),
            ],
        );
        app.update();
        let state = app.world().resource::<WorldMapState>();
        assert_eq!(state.coordinate_field, Some(CoordinateField::Latitude));
        assert_eq!(state.coordinate_draft, "21.11");
        assert_eq!(state.selected, original);
        // An unchanged Pressed interaction on the following frame must not
        // restart the editor or replace the draft with the previous selection.
        send_keys(
            &mut app,
            [key_event(
                KeyCode::Enter,
                Key::Enter,
                None,
                ButtonState::Pressed,
            )],
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let state = app.world().resource::<WorldMapState>();
        near(state.selected.latitude.to_degrees().get(), 21.11);
        assert!(state.visible && state.coordinate_field.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn clicking_another_coordinate_before_same_frame_text_and_enter_targets_that_field() {
        let mut app = coordinate_input_app();
        let original_latitude = app.world().resource::<WorldMapState>().selected.latitude;
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .coordinate_draft = "45".into();
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Longitude));
        send_keys(
            &mut app,
            [
                key_event(KeyCode::Delete, Key::Delete, None, ButtonState::Pressed),
                typed(KeyCode::Minus, "-71.97"),
                key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
            ],
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let state = app.world().resource::<WorldMapState>();
        assert_eq!(state.selected.latitude, original_latitude);
        near(state.selected.longitude.to_degrees().get(), -71.97);
        assert!(state.visible && state.coordinate_field.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn same_frame_coordinate_focus_does_not_override_modal_priority_or_focus_loss() {
        for case in 0..6 {
            let mut app = input_app();
            app.world_mut().resource_mut::<WorldMapState>().visible = true;
            app.world_mut()
                .spawn((Interaction::Pressed, WorldMapButton::Latitude));
            let original = app.world().resource::<WorldMapState>().selected;
            match case {
                0 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::KeyM),
                1 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape),
                2 => {
                    app.world_mut()
                        .spawn((Interaction::Pressed, WorldMapButton::Close));
                }
                3 => {
                    app.world_mut()
                        .spawn((Interaction::Pressed, WorldMapButton::OpenCredits));
                }
                4 => {
                    app.world_mut().write_message(KeyboardFocusLost).unwrap();
                }
                _ => app
                    .world_mut()
                    .resource_mut::<WorldMapState>()
                    .show_credits(),
            }
            send_keys(
                &mut app,
                [
                    key_event(KeyCode::Delete, Key::Delete, None, ButtonState::Pressed),
                    typed(KeyCode::Digit2, "21"),
                    key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                ],
            );
            app.update();
            let state = app.world().resource::<WorldMapState>();
            assert_eq!(state.selected, original, "priority case {case}");
            assert!(state.coordinate_field.is_none(), "priority case {case}");
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        }
    }

    #[test]
    fn enter_while_editing_applies_coordinates_without_starting() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.begin_coordinate(CoordinateField::Latitude);
            state.coordinate_draft = "-33.95".into();
        }
        app.world_mut()
            .write_message(key_event(
                KeyCode::Enter,
                Key::Enter,
                None,
                ButtonState::Pressed,
            ))
            .expect("registered keyboard messages");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        near(
            app.world()
                .resource::<WorldMapState>()
                .selected
                .latitude
                .to_degrees()
                .get(),
            -33.95,
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn close_wins_over_a_simultaneous_enter_and_cancels_unconsumed_start() {
        let mut app = input_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Close));
        app.world_mut().resource_mut::<WorldMapActions>().start_at = Some(WorldMapStart {
            position: WORLD_MAP_DESTINATIONS[0].position(),
            month: 7,
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(!app.world().resource::<WorldMapState>().visible);
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn display_updates_visibility_text_marker_and_one_image_without_resampling() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<WorldMapState>()
            .init_resource::<WorldMapRaster>()
            .add_systems(Startup, spawn_world_map)
            .add_systems(Update, update_world_map);
        app.update();
        let root_visibility = |app: &mut App| {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Visibility, With<WorldMapRoot>>();
            *query.single(world).unwrap()
        };
        assert_eq!(root_visibility(&mut app), Visibility::Hidden);
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.selected_terrain = "Coarse terrain 500 m".into();
            state.selected_climate = "Dry / warm".into();
            state.aircraft = Some(Geodetic::from_degrees(0.0, 0.0, 1000.0));
        }
        app.update();
        assert_eq!(root_visibility(&mut app), Visibility::Inherited);
        let world = app.world_mut();
        let mut query = world.query::<(&WorldMapText, &Text)>();
        let (_, details) = query
            .iter(world)
            .find(|(kind, _)| **kind == WorldMapText::Details)
            .unwrap();
        assert!(details.contains("500 m"));
        assert!(details.contains("Dry / warm"));
        let mut query = world.query::<(&WorldMapMarker, &Node, &Visibility)>();
        let (_, node, visible) = query
            .iter(world)
            .find(|(kind, _, _)| matches!(kind, WorldMapMarker::Aircraft))
            .unwrap();
        assert_eq!(node.left, percent(50.0));
        assert_eq!(node.top, percent(50.0));
        assert_eq!(*visible, Visibility::Inherited);
        assert_eq!(world.resource::<Assets<Image>>().len(), 1);
    }

    #[test]
    fn navigation_disclosure_keeps_both_safe_altitude_and_unsaved_recording_warning() {
        let state = WorldMapState {
            navigation_note: "New flight: 1000 m AGL\nUnsaved recording will be reset".into(),
            ..default()
        };
        let text =
            format_world_map_text(WorldMapText::Navigation, &state, &WorldMapRaster::default());
        assert!(text.contains("1000 m AGL"));
        assert!(text.contains("Unsaved recording will be reset"));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn credits_paginate_without_losing_the_last_notice_or_long_identifiers() {
        let text = (0..45)
            .map(|n| format!("Required source notice {n:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = credit_pages(&text);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages.join("\n"), text);
        assert!(pages[2].contains("Required source notice 44"));
        let identifier = "a".repeat(CREDIT_COLUMNS * 3 + 7);
        let pages = credit_pages(&identifier);
        assert_eq!(pages.join("").replace('\n', ""), identifier);
        assert!(
            pages
                .iter()
                .flat_map(|page| page.lines())
                .all(|line| line.len() <= CREDIT_COLUMNS)
        );
    }

    #[test]
    fn credits_have_ascii_and_explicit_overflow_fallbacks() {
        assert_eq!(credit_pages("").len(), 1);
        assert!(
            credit_pages("Copyright © 世界\nNOAA PSL")
                .iter()
                .all(|page| page.is_ascii())
        );
        let oversized = "x".repeat(MAX_CREDIT_CHARACTERS + 10);
        let pages = credit_pages(&oversized);
        assert!(pages.last().unwrap().contains("complete text"));
        assert!(
            pages
                .iter()
                .all(|page| page.lines().count() <= CREDIT_LINES_PER_PAGE)
        );
    }

    #[test]
    fn credits_open_close_repeatedly_and_escape_dismisses_only_the_top_modal() {
        let mut app = input_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        let button = app
            .world_mut()
            .spawn((Interaction::None, WorldMapButton::OpenCredits))
            .id();
        for _ in 0..3 {
            *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
            app.update();
            assert!(app.world().resource::<WorldMapState>().credits_visible);
            *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.update();
            assert!(!app.world().resource::<WorldMapState>().credits_visible);
            assert!(app.world().resource::<WorldMapState>().visible);
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::Escape)
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::Escape);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<WorldMapState>().visible);
    }

    #[test]
    fn credits_capture_enter_month_and_map_clicks_and_have_a_close_button() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.credits_visible = true;
        }
        let original = app.world().resource::<WorldMapState>().selected;
        app.world_mut().spawn((
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            WorldMapCanvas,
        ));
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::NextMonth));
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Start));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        assert_eq!(app.world().resource::<WorldMapState>().month, 7);
        assert_eq!(app.world().resource::<WorldMapState>().selected, original);
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::CloseCredits));
        app.update();
        assert!(!app.world().resource::<WorldMapState>().credits_visible);
        assert!(app.world().resource::<WorldMapState>().visible);
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn opening_credits_wins_over_enter_and_page_buttons_stay_in_bounds() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.source_credits = "line\n".repeat(45);
        }
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::OpenCredits));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        let next = app
            .world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::NextCreditsPage))
            .id();
        app.update();
        assert_eq!(app.world().resource::<WorldMapState>().credits_page, 1);
        for _ in 0..4 {
            *app.world_mut().get_mut::<Interaction>(next).unwrap() = Interaction::None;
            app.update();
            *app.world_mut().get_mut::<Interaction>(next).unwrap() = Interaction::Pressed;
            app.update();
        }
        assert_eq!(app.world().resource::<WorldMapState>().credits_page, 2);
        let state = app.world().resource::<WorldMapState>();
        assert_eq!(
            format_world_map_text(WorldMapText::CreditsPage, state, &WorldMapRaster::default()),
            "Page 3 / 3"
        );
    }

    // Run the real Bevy/Taffy layout and Cosmic Text measurement systems. A
    // manually sized camera avoids GPU/window requirements without substituting
    // estimated character widths or hand-calculated flex sizes.
    fn real_layout_app(state: WorldMapState) -> App {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
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
        .init_resource::<WorldMapRaster>()
        .insert_resource(state)
        .add_systems(Startup, spawn_world_map)
        .add_systems(Update, update_world_map);
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(1280, 720),
                    ..default()
                }),
                ..default()
            },
        ));
        app.finish();
        app.cleanup();
        for _ in 0..4 {
            app.update();
        }
        app
    }

    fn computed_rect(world: &World, entity: Entity) -> Rect {
        let node = world.get::<ComputedNode>(entity).expect("computed UI node");
        let transform = world
            .get::<bevy::ui::UiGlobalTransform>(entity)
            .expect("computed UI transform");
        Rect::from_center_size(transform.translation, node.size())
    }

    #[test]
    fn real_layout_keeps_sidebar_controls_inside_body_at_1280x720() {
        let mut app = real_layout_app(WorldMapState {
            visible: true,
            selected_name: "N".repeat(80),
            selected_terrain: vec!["T".repeat(DETAILS_COLUMNS); TERRAIN_DETAIL_LINES].join("\n"),
            selected_climate: vec!["C".repeat(DETAILS_COLUMNS); CLIMATE_DETAIL_LINES].join("\n"),
            navigation_note: "New flight: 1000 m AGL\nUnsaved recording will be reset".into(),
            ..default()
        });
        let world = app.world_mut();
        let body = world
            .query_filtered::<Entity, With<WorldMapBody>>()
            .single(world)
            .unwrap();
        let side = world
            .query_filtered::<Entity, With<WorldMapSidebar>>()
            .single(world)
            .unwrap();
        let body_rect = computed_rect(world, body);
        let side_rect = computed_rect(world, side);
        assert!(body_rect.min.y >= MAP_ROOT_PADDING + MAP_HEADER_HEIGHT);
        assert!(body_rect.max.y <= 720.0 - MAP_ROOT_PADDING - MAP_FOOTER_HEIGHT);
        assert!(
            body_rect.contains(side_rect.min) && body_rect.contains(side_rect.max),
            "sidebar outside body: {side_rect:?}, {body_rect:?}"
        );
        for (entity, kind) in world.query::<(Entity, &WorldMapText)>().iter(world) {
            if matches!(
                kind,
                WorldMapText::Credits | WorldMapText::CreditsPage | WorldMapText::Regions(_)
            ) {
                continue;
            }
            let rect = computed_rect(world, entity);
            assert!(
                body_rect.contains(rect.min) && body_rect.contains(rect.max),
                "text outside body: {kind:?}, {rect:?}"
            );
            assert_text_fits(world, entity);
        }
        for (entity, button) in world.query::<(Entity, &WorldMapButton)>().iter(world) {
            if matches!(
                button,
                WorldMapButton::Close
                    | WorldMapButton::OpenRegions
                    | WorldMapButton::Regions(_)
                    | WorldMapButton::OpenCredits
                    | WorldMapButton::CloseCredits
                    | WorldMapButton::PreviousCreditsPage
                    | WorldMapButton::NextCreditsPage
            ) {
                continue;
            }
            let rect = computed_rect(world, entity);
            assert!(
                rect.width() >= 29.0 && rect.height() >= MAP_BUTTON_HEIGHT - 1.0,
                "undersized {button:?}: {rect:?}"
            );
            assert!(
                body_rect.contains(rect.min) && body_rect.contains(rect.max),
                "unreachable {button:?}: {rect:?}"
            );
        }
    }

    fn assert_text_fits(world: &World, entity: Entity) {
        let node = world.get::<ComputedNode>(entity).unwrap();
        let text = world.get::<Text>(entity).unwrap();
        let shaped = world.get::<bevy::text::TextLayoutInfo>(entity).unwrap();
        assert!(!shaped.glyphs.is_empty(), "text was not measured: {text:?}");
        assert!(
            shaped.size.x <= node.size().x + 1.0 && shaped.size.y <= node.size().y + 1.0,
            "shaped text exceeds node: {text:?}, {:?}, {:?}",
            shaped.size,
            node.size()
        );
    }

    #[test]
    fn real_layout_keeps_full_credits_page_and_controls_inside_panel() {
        let mut state = WorldMapState {
            source_credits: vec!["W".repeat(CREDIT_COLUMNS); CREDIT_LINES_PER_PAGE * 2].join("\n"),
            ..default()
        };
        state.show_credits();
        let mut app = real_layout_app(state);
        for page in 0..2 {
            app.world_mut().resource_mut::<WorldMapState>().credits_page = page;
            app.update();
            let world = app.world_mut();
            let overlay = world
                .query_filtered::<Entity, With<WorldMapCreditsRoot>>()
                .single(world)
                .unwrap();
            let panel = world.get::<Children>(overlay).unwrap()[0];
            let panel_rect = computed_rect(world, panel);
            assert!(
                panel_rect.min.x >= 0.0
                    && panel_rect.min.y >= 0.0
                    && panel_rect.max.x <= 1280.0
                    && panel_rect.max.y <= 720.0
            );
            for (entity, kind) in world.query::<(Entity, &WorldMapText)>().iter(world) {
                if matches!(kind, WorldMapText::Credits | WorldMapText::CreditsPage) {
                    let rect = computed_rect(world, entity);
                    assert!(
                        panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                        "credits text outside panel: {rect:?}"
                    );
                    assert_text_fits(world, entity);
                }
            }
            for (entity, button) in world.query::<(Entity, &WorldMapButton)>().iter(world) {
                if matches!(
                    button,
                    WorldMapButton::CloseCredits
                        | WorldMapButton::PreviousCreditsPage
                        | WorldMapButton::NextCreditsPage
                ) {
                    let rect = computed_rect(world, entity);
                    assert!(
                        panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                        "unreachable credits control: {button:?}, {rect:?}"
                    );
                    assert!(rect.height() >= MAP_BUTTON_HEIGHT - 1.0);
                }
            }
        }
    }

    #[test]
    fn regions_reopen_and_dismiss_cancel_pending_work_and_consume_start_keys() {
        let mut app = input_app();
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        let open = app
            .world_mut()
            .spawn((Interaction::None, WorldMapButton::OpenRegions))
            .id();
        for close in [KeyCode::Escape, KeyCode::KeyM, KeyCode::Escape] {
            app.world_mut().resource_mut::<WorldMapState>().visible = true;
            *app.world_mut().get_mut::<Interaction>(open).unwrap() = Interaction::Pressed;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.update();
            assert!(app.world().resource::<WorldMapState>().regions.visible);
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
            *app.world_mut().get_mut::<Interaction>(open).unwrap() = Interaction::None;
            app.world_mut().resource_mut::<WorldMapState>().regions.busy = true;
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending = Some(RegionAction::Refresh);
            app.world_mut().resource_mut::<WorldMapActions>().start_at = Some(WorldMapStart {
                position: WORLD_MAP_DESTINATIONS[0].position(),
                month: 7,
            });
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(close);
            app.update();
            assert!(!app.world().resource::<WorldMapState>().regions.visible);
            assert_eq!(
                app.world().resource::<WorldMapState>().visible,
                close == KeyCode::Escape
            );
            let actions = app.world_mut().resource_mut::<WorldMapActions>();
            assert!(actions.start_at.is_none());
            assert_eq!(actions.regions.pending, Some(RegionAction::Cancel));
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            assert!(!keys.just_pressed(close));
            assert!(!keys.just_pressed(KeyCode::Enter));
            keys.release(close);
            keys.release(KeyCode::Enter);
        }
    }

    #[test]
    fn regions_capture_underlying_controls_and_selection_is_a_single_app_action() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.show_regions();
            state.regions.set_installed([RegionSummary {
                key: "alps@1.2.3".into(),
                name: "Alps".into(),
            }]);
        }
        let original = app.world().resource::<WorldMapState>().selected;
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Start));
        app.world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::NextMonth));
        app.world_mut().spawn((
            Interaction::Pressed,
            WorldMapButton::Regions(RegionsButton::Row(0)),
        ));
        app.world_mut().spawn((
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
            WorldMapCanvas,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::NumpadEnter);
        let mut repeated = key_event(KeyCode::NumpadEnter, Key::Enter, None, ButtonState::Pressed);
        repeated.repeat = true;
        send_keys(&mut app, [repeated.clone(), repeated]);
        app.update();
        let state = app.world().resource::<WorldMapState>();
        assert_eq!(state.selected, original);
        assert_eq!(state.month, 7);
        assert!(state.regions.selected.is_none() && state.regions.active.is_none());
        let mut actions = app.world_mut().resource_mut::<WorldMapActions>();
        assert!(actions.start_at.is_none());
        assert_eq!(
            actions.regions.pending.take(),
            Some(RegionAction::Select(Some("alps@1.2.3".into())))
        );
        app.update();
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .regions
                .pending
                .is_none()
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::NumpadEnter)
        );
    }

    #[test]
    fn regions_close_button_cancels_before_other_actions() {
        let mut app = input_app();
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_regions();
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .regions
            .pending = Some(RegionAction::Select(None));
        for button in [
            RegionsButton::Refresh,
            RegionsButton::Close,
            RegionsButton::Base,
        ] {
            app.world_mut()
                .spawn((Interaction::Pressed, WorldMapButton::Regions(button)));
        }
        app.update();
        assert!(app.world().resource::<WorldMapState>().visible);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        assert_eq!(
            app.world().resource::<WorldMapActions>().regions.pending,
            Some(RegionAction::Cancel)
        );
    }

    #[test]
    fn regions_simultaneous_m_escape_and_enter_are_consumed_on_dismissal() {
        let mut app = input_app();
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_regions();
        app.world_mut().resource_mut::<WorldMapState>().regions.busy = true;
        for key in [
            KeyCode::KeyM,
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::NumpadEnter,
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
        }
        app.update();
        let state = app.world().resource::<WorldMapState>();
        assert!(!state.visible && !state.regions.visible);
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert_eq!(keys.get_just_pressed().count(), 0);
        let actions = app.world().resource::<WorldMapActions>();
        assert!(actions.start_at.is_none());
        assert_eq!(actions.regions.pending, Some(RegionAction::Cancel));
    }

    fn region_keys(app: &mut App, pressed: &[KeyCode]) {
        let mut keys = ButtonInput::<KeyCode>::default();
        for key in pressed {
            keys.press(*key);
        }
        app.insert_resource(keys);
        app.update();
    }

    #[test]
    fn regions_keyboard_opens_selects_visible_rows_and_leaves_activation_to_app() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state
                .regions
                .set_installed((0..7).map(|index| RegionSummary {
                    key: format!("region-{index}@1.0.0"),
                    name: format!("Region {index}"),
                }));
            state.regions.credits = vec!["Credit"; 80].join("\n");
        }
        let original = app.world().resource::<WorldMapState>().selected;
        region_keys(&mut app, &[KeyCode::KeyG, KeyCode::Enter, KeyCode::Digit1]);
        assert!(app.world().resource::<WorldMapState>().regions.visible);
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .regions
                .pending
                .is_none()
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        assert_eq!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_just_pressed()
                .count(),
            0
        );
        region_keys(&mut app, &[KeyCode::PageDown, KeyCode::Digit1]);
        assert!(
            regions::format_regions_text(
                RegionsText::Page,
                &app.world().resource::<WorldMapState>().regions
            )
            .starts_with("2 / 2")
        );
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .regions
                .pending
                .is_none()
        );
        region_keys(&mut app, &[KeyCode::Digit1]);
        assert_eq!(
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending
                .take(),
            Some(RegionAction::Select(Some("region-5@1.0.0".into())))
        );
        assert_eq!(app.world().resource::<WorldMapState>().selected, original);
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .regions
                .selected
                .is_none()
        );
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .regions
                .active
                .is_none()
        );
        region_keys(&mut app, &[KeyCode::Digit5]);
        assert!(
            app.world()
                .resource::<WorldMapActions>()
                .regions
                .pending
                .is_none()
        );
        region_keys(&mut app, &[KeyCode::PageUp]);
        region_keys(&mut app, &[KeyCode::Digit5]);
        assert_eq!(
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending
                .take(),
            Some(RegionAction::Select(Some("region-4@1.0.0".into())))
        );
        region_keys(&mut app, &[KeyCode::Digit0]);
        assert_eq!(
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending
                .take(),
            Some(RegionAction::Select(None))
        );
        region_keys(&mut app, &[KeyCode::ArrowRight]);
        assert!(
            regions::format_regions_text(
                RegionsText::CreditsPage,
                &app.world().resource::<WorldMapState>().regions
            )
            .starts_with("Page 2 /")
        );
        region_keys(&mut app, &[KeyCode::ArrowLeft]);
        assert!(
            regions::format_regions_text(
                RegionsText::CreditsPage,
                &app.world().resource::<WorldMapState>().regions
            )
            .starts_with("Page 1 /")
        );
        region_keys(&mut app, &[KeyCode::KeyR]);
        assert_eq!(
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending
                .take(),
            Some(RegionAction::Refresh)
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        assert_eq!(app.world().resource::<WorldMapState>().month, 7);
    }

    #[test]
    fn regions_keyboard_preserves_busy_pending_modifier_and_close_guards() {
        let mut app = input_app();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.show_regions();
            state.regions.set_installed([RegionSummary {
                key: "alps@1.0.0".into(),
                name: "Alps".into(),
            }]);
        }
        for busy in [true, false] {
            {
                let mut state = app.world_mut().resource_mut::<WorldMapState>();
                state.regions.busy = busy;
                state.regions.operations_enabled = busy;
            }
            for key in [KeyCode::KeyR, KeyCode::Digit0, KeyCode::Digit1] {
                region_keys(&mut app, &[key]);
                assert!(
                    app.world()
                        .resource::<WorldMapActions>()
                        .regions
                        .pending
                        .is_none()
                );
                assert!(
                    !app.world()
                        .resource::<ButtonInput<KeyCode>>()
                        .just_pressed(key)
                );
            }
        }
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .regions
            .operations_enabled = true;
        for modifier in [KeyCode::ControlLeft, KeyCode::AltRight, KeyCode::SuperLeft] {
            region_keys(&mut app, &[modifier, KeyCode::KeyR, KeyCode::Digit0]);
            assert!(
                app.world()
                    .resource::<WorldMapActions>()
                    .regions
                    .pending
                    .is_none()
            );
        }
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .regions
            .pending = Some(RegionAction::Refresh);
        region_keys(&mut app, &[KeyCode::Digit1]);
        assert_eq!(
            app.world().resource::<WorldMapActions>().regions.pending,
            Some(RegionAction::Refresh)
        );
        app.world_mut().spawn((
            Interaction::Pressed,
            WorldMapButton::Regions(RegionsButton::Row(0)),
        ));
        region_keys(&mut app, &[KeyCode::KeyX, KeyCode::KeyR]);
        assert_eq!(
            app.world().resource::<WorldMapActions>().regions.pending,
            Some(RegionAction::Cancel)
        );
        for close in [KeyCode::Escape, KeyCode::KeyM] {
            app.world_mut()
                .resource_mut::<WorldMapState>()
                .show_regions();
            app.world_mut()
                .resource_mut::<WorldMapActions>()
                .regions
                .pending = Some(RegionAction::Refresh);
            region_keys(
                &mut app,
                &[close, KeyCode::KeyR, KeyCode::Digit1, KeyCode::Enter],
            );
            assert!(!app.world().resource::<WorldMapState>().regions.visible);
            assert_eq!(
                app.world().resource::<WorldMapActions>().regions.pending,
                Some(RegionAction::Cancel)
            );
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
            assert_eq!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .get_just_pressed()
                    .count(),
                0
            );
        }
    }

    #[test]
    fn regions_shortcuts_do_not_steal_hidden_map_or_coordinate_editor_frames() {
        let mut app = input_app();
        region_keys(&mut app, &[KeyCode::KeyG, KeyCode::KeyR, KeyCode::PageUp]);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        assert_eq!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_just_pressed()
                .count(),
            3
        );
        app.world_mut().resource_mut::<WorldMapState>().visible = true;
        region_keys(&mut app, &[KeyCode::ControlLeft, KeyCode::KeyG]);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        // Same-frame field focus owns typed G, even though the physical key is
        // also the map shortcut. It remains an invalid coordinate edit.
        let field = app
            .world_mut()
            .spawn((Interaction::Pressed, WorldMapButton::Latitude))
            .id();
        send_keys(&mut app, [typed(KeyCode::KeyG, "g")]);
        region_keys(&mut app, &[KeyCode::KeyG]);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_field
                .is_some()
        );
        *app.world_mut().get_mut::<Interaction>(field).unwrap() = Interaction::None;
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .coordinate_draft = "12".into();
        send_keys(
            &mut app,
            [key_event(
                KeyCode::Enter,
                Key::Enter,
                None,
                ButtonState::Pressed,
            )],
        );
        region_keys(&mut app, &[KeyCode::KeyG, KeyCode::Enter]);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        near(
            app.world()
                .resource::<WorldMapState>()
                .selected
                .latitude_degrees(),
            12.0,
        );
        // G must not dismiss or replace a higher-priority credits modal.
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .show_credits();
        region_keys(&mut app, &[KeyCode::KeyG, KeyCode::Digit1]);
        assert!(app.world().resource::<WorldMapState>().credits_visible);
        assert!(!app.world().resource::<WorldMapState>().regions.visible);
        assert_eq!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_just_pressed()
                .count(),
            0
        );
    }

    #[test]
    fn real_layout_keeps_region_rows_credits_and_controls_inside_panel() {
        let mut state = WorldMapState::default();
        state.regions.set_installed((0..256).map(|n| RegionSummary {
            key: format!("{}@1.2.{n}", "a".repeat(80)),
            name: "W".repeat(80),
        }));
        state.regions.selected = Some("S".repeat(112));
        state.regions.active = Some("A".repeat(112));
        state.regions.status = "W".repeat(200);
        state.regions.error = "W".repeat(200);
        state.regions.store = "W".repeat(400);
        state.regions.credits = vec!["W".repeat(60); 34].join("\n");
        state.show_regions();
        let mut app = real_layout_app(state);
        let world = app.world_mut();
        let overlay = world
            .query_filtered::<Entity, With<WorldMapRegionsRoot>>()
            .single(world)
            .unwrap();
        let panel = world.get::<Children>(overlay).unwrap()[0];
        let panel_rect = computed_rect(world, panel);
        assert!(
            panel_rect.min.x >= 0.0
                && panel_rect.min.y >= 0.0
                && panel_rect.max.x <= 1280.0
                && panel_rect.max.y <= 720.0
        );
        for (entity, kind) in world.query::<(Entity, &WorldMapText)>().iter(world) {
            if let WorldMapText::Regions(_) = kind {
                let rect = computed_rect(world, entity);
                assert!(
                    panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                    "region text outside panel: {kind:?}, {rect:?}"
                );
                assert_text_fits(world, entity);
            }
        }
        for (entity, _) in world.query::<(Entity, &Text)>().iter(world) {
            let mut ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
            while let Some(parent) = ancestor {
                if parent == panel {
                    let rect = computed_rect(world, entity);
                    assert!(
                        panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                        "region static text outside panel: {rect:?}"
                    );
                    assert_text_fits(world, entity);
                    break;
                }
                ancestor = world.get::<ChildOf>(parent).map(ChildOf::parent);
            }
        }
        for (entity, button) in world.query::<(Entity, &WorldMapButton)>().iter(world) {
            if let WorldMapButton::Regions(_) = button {
                let rect = computed_rect(world, entity);
                assert!(
                    panel_rect.contains(rect.min) && panel_rect.contains(rect.max),
                    "region control outside panel: {button:?}, {rect:?}"
                );
                assert!(rect.height() >= MAP_BUTTON_HEIGHT - 1.0);
            }
        }
    }

    #[test]
    fn show_credits_resets_page_and_coordinate_focus_without_applying_draft() {
        let mut state = WorldMapState::default();
        let selected = state.selected;
        state.begin_coordinate(CoordinateField::Latitude);
        state.coordinate_draft = "-80".into();
        state.coordinate_error = "invalid".into();
        state.credits_page = 3;
        for _ in 0..2 {
            state.show_credits();
            assert!(state.visible && state.credits_visible);
            assert_eq!(state.credits_page, 0);
            assert!(state.coordinate_field.is_none() && state.coordinate_error.is_empty());
            assert_eq!(state.selected, selected);
        }
    }

    #[test]
    fn details_preserve_local_dem_and_snow_lines_without_widening_the_sidebar() {
        let state = WorldMapState {
            selected_terrain: "Land | ~2400 m MSL\nGlobal preview: ~20 km spacing\nLocal DEM has priority in flight".into(),
            selected_climate: "Alpine | -4.2 C\nPrecip 2.1 mm/day | cloud 65%\nSnow cue 80% (derived)\nExtra climate status".into(),
            ..default()
        };
        let details =
            format_world_map_text(WorldMapText::Details, &state, &WorldMapRaster::default());
        assert_eq!(details.lines().count(), 7);
        assert!(details.contains("Local DEM has priority in flight"));
        assert!(details.contains("Snow cue 80% (derived)"));
        assert!(details.lines().all(|line| line.len() <= DETAILS_COLUMNS));
        assert!(
            format_world_map_text(WorldMapText::Legend, &state, &WorldMapRaster::default())
                .contains("Blue: water")
        );
    }

    #[test]
    fn coordinate_messages_preserve_batched_order_and_repeated_digit_presses() {
        let mut app = coordinate_input_app();
        let mut repeated = typed(KeyCode::Digit1, "1");
        repeated.repeat = true;
        send_keys(
            &mut app,
            [
                typed(KeyCode::Digit2, "2"),
                typed(KeyCode::Digit1, "1"),
                repeated,
                typed(KeyCode::Period, "."),
                typed(KeyCode::Digit2, "2"),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "211.2"
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "211.2"
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn batched_ctrl_a_release_then_digits_obeys_event_order_not_final_modifier_state() {
        let mut app = coordinate_input_app();
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .coordinate_draft = "35.55".into();
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
                typed(KeyCode::KeyA, "a"),
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Released,
                ),
                typed(KeyCode::Digit2, "2"),
                typed(KeyCode::Digit1, "1"),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "21"
        );
        // A Ctrl press at the end must not suppress digits that preceded it.
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .coordinate_draft
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlRight);
        send_keys(
            &mut app,
            [
                typed(KeyCode::Digit2, "2"),
                typed(KeyCode::Digit1, "1"),
                key_event(
                    KeyCode::ControlRight,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "21"
        );
        send_keys(
            &mut app,
            [typed(KeyCode::KeyA, "a"), typed(KeyCode::Digit9, "9")],
        );
        app.update();
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_draft
                .is_empty()
        );
    }

    #[test]
    fn both_ctrl_keys_and_cross_frame_modifier_history_are_tracked_independently() {
        let mut app = coordinate_input_app();
        app.world_mut()
            .resource_mut::<WorldMapState>()
            .coordinate_draft = "123".into();
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
                key_event(
                    KeyCode::ControlRight,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
            ],
        );
        app.update();
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlLeft,
                    Key::Control,
                    None,
                    ButtonState::Released,
                ),
                typed(KeyCode::KeyA, "a"),
                typed(KeyCode::Digit9, "9"),
            ],
        );
        app.update();
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_draft
                .is_empty()
        );
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlRight,
                    Key::Control,
                    None,
                    ButtonState::Released,
                ),
                typed(KeyCode::Digit2, "2"),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "2"
        );
    }

    #[test]
    fn numpad_logical_text_decimal_and_multichar_payload_preserve_input() {
        let mut app = coordinate_input_app();
        send_keys(
            &mut app,
            [
                typed(KeyCode::NumpadSubtract, "-"),
                key_event(
                    KeyCode::Numpad2,
                    Key::Character("2".into()),
                    None,
                    ButtonState::Pressed,
                ),
                typed(KeyCode::Numpad1, "1"),
                typed(KeyCode::NumpadDecimal, ","),
                typed(KeyCode::Numpad3, "3"),
                typed(KeyCode::Numpad0, "0"),
                // Text takes priority when the logical character differs.
                key_event(
                    KeyCode::Digit4,
                    Key::Character("9".into()),
                    Some("45"),
                    ButtonState::Pressed,
                ),
                key_event(KeyCode::Numpad2, Key::ArrowDown, None, ButtonState::Pressed),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "-21.3045"
        );
    }

    #[test]
    fn ordered_backspace_delete_and_repeated_enter_never_start_a_flight() {
        let mut app = coordinate_input_app();
        let mut backspace = key_event(
            KeyCode::Backspace,
            Key::Backspace,
            None,
            ButtonState::Pressed,
        );
        backspace.repeat = true;
        send_keys(
            &mut app,
            [
                typed(KeyCode::Digit2, "21.11"),
                backspace.clone(),
                backspace,
                typed(KeyCode::Digit9, "9"),
            ],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "21.9"
        );
        let enter = key_event(KeyCode::NumpadEnter, Key::Enter, None, ButtonState::Pressed);
        let mut repeated_enter = enter.clone();
        repeated_enter.repeat = true;
        send_keys(
            &mut app,
            [
                key_event(KeyCode::Delete, Key::Delete, None, ButtonState::Pressed),
                typed(KeyCode::Digit4, "-45.12"),
                enter,
                repeated_enter,
                typed(KeyCode::Digit9, "9"),
            ],
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::NumpadEnter);
        app.update();
        let state = app.world().resource::<WorldMapState>();
        near(state.selected.latitude.to_degrees().get(), -45.12);
        assert!(state.coordinate_field.is_none());
        assert_eq!(state.coordinate_draft, "-45.12");
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn inactive_editor_drains_messages_and_focus_loss_clears_ctrl_history() {
        let mut app = input_app();
        send_keys(
            &mut app,
            [
                key_event(
                    KeyCode::ControlRight,
                    Key::Control,
                    None,
                    ButtonState::Pressed,
                ),
                typed(KeyCode::Digit9, "9"),
            ],
        );
        app.update();
        {
            let mut state = app.world_mut().resource_mut::<WorldMapState>();
            state.visible = true;
            state.begin_coordinate(CoordinateField::Latitude);
        }
        send_keys(&mut app, [typed(KeyCode::KeyA, "a")]);
        app.update();
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_draft
                .is_empty()
        );
        app.world_mut()
            .write_message(KeyboardFocusLost)
            .expect("registered focus messages");
        send_keys(&mut app, [typed(KeyCode::Digit1, "1")]);
        app.update();
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_draft
                .is_empty()
        );
        send_keys(&mut app, [typed(KeyCode::Digit2, "2")]);
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "2"
        );
    }

    #[test]
    fn ordered_entry_remains_bounded_and_release_events_do_not_type_characters() {
        let mut app = coordinate_input_app();
        let mut release = typed(KeyCode::Digit9, "9");
        release.state = ButtonState::Released;
        send_keys(
            &mut app,
            [release, typed(KeyCode::Digit1, "12345678901234567890")],
        );
        app.update();
        assert_eq!(
            app.world().resource::<WorldMapState>().coordinate_draft,
            "123456789012345?"
        );
        let original = app.world().resource::<WorldMapState>().selected;
        send_keys(
            &mut app,
            [key_event(
                KeyCode::Enter,
                Key::Enter,
                None,
                ButtonState::Pressed,
            )],
        );
        app.update();
        assert_eq!(app.world().resource::<WorldMapState>().selected, original);
        assert!(
            app.world()
                .resource::<WorldMapState>()
                .coordinate_field
                .is_some()
        );
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn unicode_minus_is_normalized_without_losing_a_negative_coordinate() {
        for text in ["\u{2212}13.52", "\u{ff0d}13.52"] {
            let mut app = coordinate_input_app();
            send_keys(
                &mut app,
                [
                    typed(KeyCode::Minus, text),
                    key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                ],
            );
            app.update();
            let state = app.world().resource::<WorldMapState>();
            near(state.selected.latitude.to_degrees().get(), -13.52);
            assert_eq!(state.coordinate_draft, "-13.52");
            assert!(state.coordinate_field.is_none());
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        }
    }

    #[test]
    fn decimal_comma_text_is_normalized_consistently_outside_the_numpad() {
        let mut app = coordinate_input_app();
        send_keys(
            &mut app,
            [
                typed(KeyCode::Digit0, "0,5"),
                key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
            ],
        );
        app.update();
        let state = app.world().resource::<WorldMapState>();
        near(state.selected.latitude.to_degrees().get(), 0.5);
        assert_eq!(state.coordinate_draft, "0.5");
        assert!(state.coordinate_field.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }

    #[test]
    fn unsupported_mixed_text_is_visibly_invalid_and_cannot_commit_a_different_number() {
        for text in [
            "12x3",
            "1e1",
            "12N",
            "\u{2212}13x52",
            "13.52\n",
            "13\u{00a0}52",
        ] {
            let mut app = coordinate_input_app();
            let original = app.world().resource::<WorldMapState>().selected;
            send_keys(
                &mut app,
                [
                    typed(KeyCode::Digit1, text),
                    key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                ],
            );
            app.update();
            let state = app.world().resource::<WorldMapState>();
            assert_eq!(state.selected, original);
            assert!(state.coordinate_draft.contains('?'));
            assert!(state.coordinate_draft.is_ascii());
            assert!(!state.coordinate_draft.contains('\n'));
            assert!(state.coordinate_field.is_some());
            assert!(!state.coordinate_error.is_empty());
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        }
    }

    #[test]
    fn coordinate_overflow_invalidates_even_an_otherwise_valid_decimal_prefix() {
        for (draft, input) in [("", "0.12345678901234567890"), ("0.12345678901234", "5")] {
            let mut app = coordinate_input_app();
            app.world_mut()
                .resource_mut::<WorldMapState>()
                .coordinate_draft = draft.into();
            let original = app.world().resource::<WorldMapState>().selected;
            send_keys(
                &mut app,
                [
                    typed(KeyCode::Digit5, input),
                    key_event(KeyCode::Enter, Key::Enter, None, ButtonState::Pressed),
                ],
            );
            app.update();
            let state = app.world().resource::<WorldMapState>();
            assert_eq!(state.coordinate_draft.len(), 16);
            assert!(state.coordinate_draft.ends_with('?'));
            assert_eq!(state.selected, original);
            assert!(state.coordinate_field.is_some());
            assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        }
    }
}
