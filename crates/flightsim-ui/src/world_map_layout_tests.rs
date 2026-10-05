// Taffy/Cosmic Text, clipping and actual Bevy pointer regressions for small maps.
use super::*;
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::window::PrimaryWindow;

fn full_state() -> WorldMapState {
    let mut state = aircraft_picker_state();
    state.active_aircraft = "Kestrel Jet Trainer (fictional)".into();
    state.aircraft_choices[0].label = "Launch: Kestrel Jet Trainer (fictional)".into();
    state.aircraft_choices[0].note = "Launch profile with its model and sound choices".into();
    state.selected_terrain = vec!["T".repeat(DETAILS_COLUMNS); TERRAIN_DETAIL_LINES].join("\n");
    state.selected_climate = vec!["C".repeat(DETAILS_COLUMNS); CLIMATE_DETAIL_LINES].join("\n");
    state.navigation_note = "New flight: 1000 m AGL\nUnsaved recording will be reset".into();
    state.weather_note =
        "Weather: MONTHLY / LEGACY [F12]\nAuthored / monthly model, not live; applies on Start"
            .into();
    state.source_credits = (0..40)
        .map(|n| format!("{} NOTICE {n}", "W".repeat(78)))
        .collect::<Vec<_>>()
        .join("\n");
    state.wind_settings = WindSettingsView {
        wind_from: "123.5".into(),
        wind_speed: "12.3".into(),
        turbulence: "Horizontal bound: 2.3456789 m/s\nDeterministic seed: 987654321".into(),
        turbulence_seed: 987654321,
        note: "Pending new-flight wind; active flight is unchanged".into(),
        error: "Wind speed: enter 0 to 300 knots".into(),
        enabled: true,
    };
    state.weather_settings = WeatherSettingsView {
        background_visibility: "200000.000".into(),
        cloud_base: Some("29500.000".into()),
        cloud_base_max: Some(Meters(29_500.0)),
        retained: "Retained layer thickness: 1500.000 m\nRetained precipitation: Rain 5.000 mm/h\nWind / turbulence unchanged".into(),
        enabled: true,
        error: "Cloud base: enter 0 to 30000 m above departure ground reference".into(),
    };
    state.regions.set_installed((0..8).map(|n| RegionSummary {
        key: format!("region-{n}@1.0.0"),
        name: "W".repeat(80),
    }));
    state.regions.set_downloads((0..8).map(|n| RegionSummary {
        key: format!("download-{n}@1.0.0"),
        name: "W".repeat(80),
    }));
    state.regions.downloads_enabled = true;
    state.regions.status = "W".repeat(200);
    state.regions.error = "W".repeat(200);
    state.regions.store = "W".repeat(400);
    state.regions.credits = vec!["W".repeat(60); 34].join("\n");
    state.regions.download_credits = state.regions.credits.clone();
    state
}

fn surface_entity(world: &mut World, target: ScrollSurface) -> Entity {
    world
        .query::<(Entity, &ScrollSurface)>()
        .iter(world)
        .find_map(|(entity, surface)| (*surface == target).then_some(entity))
        .unwrap()
}

fn descendant(world: &World, entity: Entity, parent: Entity) -> bool {
    let mut next = world.get::<ChildOf>(entity).map(ChildOf::parent);
    while let Some(ancestor) = next {
        if ancestor == parent {
            return true;
        }
        next = world.get::<ChildOf>(ancestor).map(ChildOf::parent);
    }
    false
}

fn max_scroll(world: &World, entity: Entity) -> f32 {
    let node = world.get::<ComputedNode>(entity).unwrap();
    ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0)
}

fn wheel(app: &mut App, y: f32, unit: MouseScrollUnit) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(app.world())
        .next()
        .unwrap_or(Entity::PLACEHOLDER);
    app.world_mut().write_message(MouseWheel {
        unit,
        x: 0.0,
        y,
        window,
    });
    app.update();
}

fn assert_reachable_content(app: &mut App, size: UVec2, target: ScrollSurface) {
    let world = app.world_mut();
    let surface = surface_entity(world, target);
    let bounds = computed_rect(world, surface);
    assert!(
        bounds.min.x >= 0.0
            && bounds.min.y >= 0.0
            && bounds.max.x <= size.as_vec2().x
            && bounds.max.y <= size.as_vec2().y,
        "surface outside window: {bounds:?}"
    );
    if target == ScrollSurface::Map {
        let root = world
            .query_filtered::<Entity, With<WorldMapRoot>>()
            .single(world)
            .unwrap();
        let children = world.get::<Children>(root).unwrap().to_vec();
        let header = computed_rect(world, children[0]);
        let footer = computed_rect(world, children[2]);
        assert!(header.max.y <= bounds.min.y && footer.min.y >= bounds.max.y);
        for (entity, text) in world.query::<(Entity, &Text)>().iter(world) {
            if descendant(world, entity, children[0]) || entity == children[2] {
                assert_text_fits(world, entity);
                let rect = computed_rect(world, entity);
                assert!(
                    rect.min.x >= 0.0
                        && rect.max.x <= size.as_vec2().x
                        && rect.min.y >= 0.0
                        && rect.max.y <= size.as_vec2().y,
                    "map header/footer clipped: {text:?}, {rect:?}"
                );
            }
        }
    }
    let max = max_scroll(world, surface);
    let position = world.get::<ScrollPosition>(surface).unwrap().y;
    let canvas = world
        .query_filtered::<Entity, With<WorldMapCanvas>>()
        .single(world)
        .unwrap();
    for (entity, text) in world.query::<(Entity, &Text)>().iter(world) {
        if text.is_empty()
            || !descendant(world, entity, surface)
            || descendant(world, entity, canvas)
        {
            continue;
        }
        assert_text_fits(world, entity);
        let measured = world.get::<bevy::text::TextLayoutInfo>(entity).unwrap();
        for (line_index, line) in text.lines().enumerate() {
            for (index, _) in line
                .bytes()
                .enumerate()
                .filter(|(_, byte)| !byte.is_ascii_whitespace())
            {
                assert!(
                    measured
                        .glyphs
                        .iter()
                        .any(|glyph| glyph.line_index == line_index
                            && (glyph.byte_index..glyph.byte_index + glyph.byte_length)
                                .contains(&index)),
                    "unmeasured byte {index} on line {line_index}: {text:?}"
                );
            }
        }
        let rect = computed_rect(world, entity);
        let mut ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
        while let Some(parent) = ancestor {
            if parent == surface {
                break;
            }
            let parent_node = world.get::<Node>(parent).unwrap();
            let clip = computed_rect(world, parent);
            for glyph in &measured.glyphs {
                let glyph_bounds = Rect::from_center_size(rect.min + glyph.position, glyph.size);
                if matches!(
                    parent_node.overflow.x,
                    OverflowAxis::Clip | OverflowAxis::Hidden
                ) {
                    assert!(
                        glyph_bounds.min.x >= clip.min.x - 1.0
                            && glyph_bounds.max.x <= clip.max.x + 1.0,
                        "glyph clipped by nested ancestor: {text:?}; glyph {glyph_bounds:?}; ancestor {clip:?}; text {rect:?}"
                    );
                }
                if matches!(
                    parent_node.overflow.y,
                    OverflowAxis::Clip | OverflowAxis::Hidden
                ) {
                    assert!(
                        glyph_bounds.min.y >= clip.min.y - 1.0
                            && glyph_bounds.max.y <= clip.max.y + 1.0,
                        "glyph clipped by nested ancestor: {text:?}; glyph {glyph_bounds:?}; ancestor {clip:?}; text {rect:?}"
                    );
                }
            }
            ancestor = world.get::<ChildOf>(parent).map(ChildOf::parent);
        }
        assert!(
            rect.min.x >= bounds.min.x - 1.0 && rect.max.x <= bounds.max.x + 1.0,
            "horizontal text clipping: {text:?}, {rect:?}, {bounds:?}"
        );
        assert!(
            rect.min.y + position >= bounds.min.y - 1.0
                && rect.max.y + position <= bounds.max.y + max + 1.0,
            "text outside scroll range: {text:?}, {rect:?}, {bounds:?}, max {max}"
        );
    }
    for (entity, button) in world.query::<(Entity, &WorldMapButton)>().iter(world) {
        if !descendant(world, entity, surface) {
            continue;
        }
        let rect = computed_rect(world, entity);
        assert!(
            rect.height() >= MAP_BUTTON_HEIGHT - 1.0,
            "compressed {button:?}: {rect:?}"
        );
        assert!(
            rect.min.x >= bounds.min.x - 1.0 && rect.max.x <= bounds.max.x + 1.0,
            "horizontal button clipping: {button:?}: {rect:?}"
        );
        assert!(
            rect.min.y + position >= bounds.min.y - 1.0
                && rect.max.y + position <= bounds.max.y + max + 1.0,
            "unreachable {button:?}: {rect:?}"
        );
    }
    // Direct siblings must reserve their measured height; a scroll range alone
    // cannot make text usable when a later child covers it.
    for (parent, children) in world.query::<(Entity, &Children)>().iter(world) {
        if parent == canvas
            || descendant(world, parent, canvas)
            || !(parent == surface || descendant(world, parent, surface))
        {
            continue;
        }
        for (index, first) in children.iter().enumerate() {
            let first_rect = computed_rect(world, first);
            for second in children.iter().skip(index + 1) {
                let second_rect = computed_rect(world, second);
                let overlap = first_rect.intersect(second_rect);
                assert!(
                    overlap.width() <= 1.0 || overlap.height() <= 1.0,
                    "overlapping siblings: {:?} {first_rect:?} / {:?} {second_rect:?}",
                    world.get::<Text>(first),
                    world.get::<Text>(second)
                );
            }
        }
    }
}

#[test]
fn measured_map_and_child_panels_keep_every_control_and_notice_reachable() {
    for size in [
        UVec2::new(640, 480),
        UVec2::new(900, 720),
        UVec2::new(900, 600),
        UVec2::new(899, 600),
        UVec2::new(1024, 720),
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
    ] {
        for target in [
            ScrollSurface::Map,
            ScrollSurface::Credits,
            ScrollSurface::Regions,
            ScrollSurface::Wind,
            ScrollSurface::Weather,
        ] {
            let mut state = full_state();
            match target {
                ScrollSurface::Map => {}
                ScrollSurface::Credits => state.show_credits(),
                ScrollSurface::Regions => {
                    state.regions.set_installed((0..8).map(|n| RegionSummary {
                        key: format!("{}@1.0.{n}", "w".repeat(80)),
                        name: "W".repeat(80),
                    }));
                    state.show_regions();
                }
                ScrollSurface::Wind => assert!(state.show_wind_settings()),
                ScrollSurface::Weather => assert!(state.show_weather_settings()),
            }
            let mut app = real_layout_app_at_size(state, size);
            if target == ScrollSurface::Regions {
                let row = button_entity(
                    app.world_mut(),
                    WorldMapButton::Regions(RegionsButton::Row(0)),
                );
                let text = app.world().get::<Children>(row).unwrap()[0];
                let row_rect = computed_rect(app.world(), row);
                let text_rect = computed_rect(app.world(), text);
                let layout = app.world().get::<bevy::text::TextLayoutInfo>(text).unwrap();
                let ink = layout.glyphs.iter().fold(
                    Rect {
                        min: Vec2::splat(f32::INFINITY),
                        max: Vec2::splat(f32::NEG_INFINITY),
                    },
                    |mut ink, glyph| {
                        let glyph_rect =
                            Rect::from_center_size(text_rect.min + glyph.position, glyph.size);
                        ink.min = ink.min.min(glyph_rect.min);
                        ink.max = ink.max.max(glyph_rect.max);
                        ink
                    },
                );
                eprintln!(
                    "region geometry {size:?}: row {row_rect:?}, text {text_rect:?}, shaped {:?}, glyph ink {ink:?}",
                    layout.size
                );
            }
            assert_reachable_content(&mut app, size, target);
            wheel(&mut app, -10000.0, MouseScrollUnit::Line);
            assert_reachable_content(&mut app, size, target);
            let surface = surface_entity(app.world_mut(), target);
            assert_eq!(
                app.world().get::<ScrollPosition>(surface).unwrap().y,
                max_scroll(app.world(), surface)
            );
            wheel(&mut app, 10000.0, MouseScrollUnit::Pixel);
            assert_eq!(app.world().get::<ScrollPosition>(surface).unwrap().y, 0.0);
        }
    }
}

fn interactive_app(size: UVec2) -> (App, Entity) {
    let mut app = real_layout_app_at_size(full_state(), size);
    app.init_resource::<WorldMapActions>()
        .add_systems(Update, handle_world_map_input.before(update_world_map));
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (size.x, size.y).into(),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    for _ in 0..2 {
        app.update();
    }
    (app, window)
}

fn button_entity(world: &mut World, target: WorldMapButton) -> Entity {
    world
        .query::<(Entity, &WorldMapButton)>()
        .iter(world)
        .find_map(|(entity, button)| (*button == target).then_some(entity))
        .unwrap()
}

fn pointer_press(app: &mut App, window: Entity, point: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(point));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    app.update();
}

fn reveal_button(app: &mut App, target: WorldMapButton, surface: ScrollSurface) -> Vec2 {
    let entity = button_entity(app.world_mut(), target);
    let scroll = surface_entity(app.world_mut(), surface);
    let bounds = computed_rect(app.world(), scroll);
    let button = computed_rect(app.world(), entity);
    let delta = button.center().y - bounds.center().y;
    wheel(app, -delta, MouseScrollUnit::Pixel);
    let visible = computed_rect(app.world(), entity);
    assert!(
        bounds.contains(visible.min) && bounds.contains(visible.max),
        "button did not scroll into view: {target:?}, {visible:?}, {bounds:?}"
    );
    visible.center()
}

fn click_reachable(app: &mut App, window: Entity, target: WorldMapButton, surface: ScrollSurface) {
    let point = reveal_button(app, target, surface);
    pointer_press(app, window, point);
}

#[test]
fn clipped_start_and_canvas_ignore_pointer_hits_inside_their_hidden_rectangles() {
    let (mut app, window) = interactive_app(UVec2::new(640, 480));
    let map = surface_entity(app.world_mut(), ScrollSurface::Map);
    let bounds = computed_rect(app.world(), map);
    let root = app
        .world_mut()
        .query_filtered::<Entity, With<WorldMapRoot>>()
        .single(app.world())
        .unwrap();
    let footer = app.world().get::<Children>(root).unwrap()[2];
    let start = button_entity(app.world_mut(), WorldMapButton::Start);
    let original_start = computed_rect(app.world(), start);

    // Place Start across the viewport's bottom edge. The click actually hits
    // its un-clipped rectangle in the footer, rather than missing it entirely.
    let delta = original_start.min.y - (bounds.max.y - 8.0);
    wheel(&mut app, -delta, MouseScrollUnit::Pixel);
    let clipped_start = computed_rect(app.world(), start);
    let point = Vec2::new(clipped_start.center().x, bounds.max.y + 12.0);
    assert!(clipped_start.min.y < bounds.max.y && clipped_start.max.y > bounds.max.y);
    assert!(clipped_start.contains(point));
    assert!(!bounds.contains(point));
    assert!(computed_rect(app.world(), footer).contains(point));
    assert!(computed_rect(app.world(), root).contains(point));
    pointer_press(&mut app, window, point);
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());

    // Move the raster across the top edge, then click a hidden raster point in
    // the header/body gap. Without ancestor clipping this would pick a new
    // geographic departure point even though that part of the map is invisible.
    wheel(&mut app, 10000.0, MouseScrollUnit::Pixel);
    let canvas = app
        .world_mut()
        .query_filtered::<Entity, With<WorldMapCanvas>>()
        .single(app.world())
        .unwrap();
    let original_canvas = computed_rect(app.world(), canvas);
    let delta = original_canvas.min.y - (bounds.min.y - 20.0);
    wheel(&mut app, -delta, MouseScrollUnit::Pixel);
    let clipped_canvas = computed_rect(app.world(), canvas);
    let point = Vec2::new(
        clipped_canvas.min.x + clipped_canvas.width() * 0.25,
        bounds.min.y - 5.0,
    );
    assert!(clipped_canvas.min.y < bounds.min.y && clipped_canvas.max.y > bounds.min.y);
    assert!(clipped_canvas.contains(point));
    assert!(!bounds.contains(point));
    assert!(computed_rect(app.world(), root).contains(point));
    let selected = app.world().resource::<WorldMapState>().selected;
    let un_clipped_position = geodetic_from_map_point(MapPoint {
        x: f64::from((point.x - clipped_canvas.min.x) / clipped_canvas.width()),
        y: f64::from((point.y - clipped_canvas.min.y) / clipped_canvas.height()),
    })
    .unwrap();
    assert_ne!(un_clipped_position, selected);
    pointer_press(&mut app, window, point);
    assert_eq!(app.world().resource::<WorldMapState>().selected, selected);
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
}

#[test]
fn small_map_real_pointer_scroll_start_cancel_and_modal_ownership() {
    let (mut app, window) = interactive_app(UVec2::new(640, 480));
    click_reachable(
        &mut app,
        window,
        WorldMapButton::NextAircraft,
        ScrollSurface::Map,
    );
    assert_eq!(app.world().resource::<WorldMapState>().aircraft_choice, 1);
    let point = reveal_button(
        &mut app,
        WorldMapButton::OpenWindSettings,
        ScrollSurface::Map,
    );
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -10000.0,
        window,
    });
    pointer_press(&mut app, window, point);
    let wind = surface_entity(app.world_mut(), ScrollSurface::Wind);
    assert!(max_scroll(app.world(), wind) > 0.0);
    assert_eq!(app.world().get::<ScrollPosition>(wind).unwrap().y, 0.0);
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .wind_editor
            .is_some()
    );
    let map = surface_entity(app.world_mut(), ScrollSurface::Map);
    let map_position = app.world().get::<ScrollPosition>(map).unwrap().y;
    wheel(&mut app, -10000.0, MouseScrollUnit::Line);
    assert_eq!(
        app.world().get::<ScrollPosition>(map).unwrap().y,
        map_position
    );
    let point = reveal_button(
        &mut app,
        WorldMapButton::Wind(WindSettingsButton::Cancel),
        ScrollSurface::Wind,
    );
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -10000.0,
        window,
    });
    pointer_press(&mut app, window, point);
    assert_eq!(
        app.world().get::<ScrollPosition>(map).unwrap().y,
        map_position
    );
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .wind_editor
            .is_none()
    );
    assert!(
        app.world()
            .resource::<WorldMapActions>()
            .conditions
            .is_none()
    );
    click_reachable(&mut app, window, WorldMapButton::Start, ScrollSurface::Map);
    let request = app
        .world_mut()
        .resource_mut::<WorldMapActions>()
        .start_at
        .take()
        .expect("real visible Start click");
    assert_eq!(request.aircraft_choice, 1);
    let close = button_entity(app.world_mut(), WorldMapButton::Close);
    let close_point = computed_rect(app.world(), close).center();
    pointer_press(&mut app, window, close_point);
    assert!(!app.world().resource::<WorldMapState>().visible);
    assert!(app.world().resource::<WorldMapActions>().generation > request.generation);
    assert_eq!(app.world().get::<ScrollPosition>(map).unwrap().y, 0.0);
    wheel(&mut app, -10000.0, MouseScrollUnit::Line);
    app.world_mut().resource_mut::<WorldMapState>().visible = true;
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(map).unwrap().y, 0.0);
}

#[test]
fn resizing_scrolled_map_reflows_in_same_layout_and_keeps_replay_locked() {
    let (mut app, window) = interactive_app(UVec2::new(640, 480));
    wheel(&mut app, -10000.0, MouseScrollUnit::Line);
    for size in [
        UVec2::new(1180, 812),
        UVec2::new(640, 480),
        UVec2::new(1280, 720),
    ] {
        let world = app.world_mut();
        for mut camera in world.query::<&mut Camera>().iter_mut(world) {
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
        }
        world
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(size.x, size.y);
        app.update();
        let surface = surface_entity(app.world_mut(), ScrollSurface::Map);
        let rendered_scroll = app
            .world()
            .get::<ComputedNode>(surface)
            .unwrap()
            .scroll_position
            .y;
        assert!(rendered_scroll <= max_scroll(app.world(), surface) + 1.0);
        app.update();
        assert_reachable_content(&mut app, size, ScrollSurface::Map);
    }
    let state = &mut app.world_mut().resource_mut::<WorldMapState>();
    state.aircraft_selection_enabled = false;
    state.navigation_enabled = false;
    state.wind_settings.enabled = false;
    app.update();
    click_reachable(
        &mut app,
        window,
        WorldMapButton::NextAircraft,
        ScrollSurface::Map,
    );
    click_reachable(
        &mut app,
        window,
        WorldMapButton::OpenWindSettings,
        ScrollSurface::Map,
    );
    click_reachable(&mut app, window, WorldMapButton::Start, ScrollSurface::Map);
    assert_eq!(app.world().resource::<WorldMapState>().aircraft_choice, 0);
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .wind_editor
            .is_none()
    );
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
}

#[test]
fn small_map_regions_downloads_and_complete_credits_use_real_visible_controls() {
    let (mut app, window) = interactive_app(UVec2::new(640, 480));
    let open = button_entity(app.world_mut(), WorldMapButton::OpenRegions);
    let point = computed_rect(app.world(), open).center();
    pointer_press(&mut app, window, point);
    assert!(app.world().resource::<WorldMapState>().regions.visible);
    click_reachable(
        &mut app,
        window,
        WorldMapButton::Regions(RegionsButton::ToggleDownloads),
        ScrollSurface::Regions,
    );
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .regions
            .downloads_visible
    );
    click_reachable(
        &mut app,
        window,
        WorldMapButton::Regions(RegionsButton::Row(0)),
        ScrollSurface::Regions,
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .regions
            .pending
            .take(),
        Some(RegionAction::SelectDownload("download-0@1.0.0".into()))
    );
    assert!(
        app.world()
            .resource::<WorldMapState>()
            .regions
            .selected
            .is_none()
    );
    app.world_mut()
        .resource_mut::<WorldMapState>()
        .regions
        .download_selected = Some("download-0@1.0.0".into());
    app.update();
    click_reachable(
        &mut app,
        window,
        WorldMapButton::Regions(RegionsButton::Download),
        ScrollSurface::Regions,
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .regions
            .pending
            .take(),
        Some(RegionAction::Download { offline: false })
    );
    app.world_mut().resource_mut::<WorldMapState>().regions.busy = true;
    app.update();
    click_reachable(
        &mut app,
        window,
        WorldMapButton::Regions(RegionsButton::Cancel),
        ScrollSurface::Regions,
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<WorldMapActions>()
            .regions
            .pending
            .take(),
        Some(RegionAction::Cancel)
    );
    click_reachable(
        &mut app,
        window,
        WorldMapButton::Regions(RegionsButton::Close),
        ScrollSurface::Regions,
    );
    assert!(!app.world().resource::<WorldMapState>().regions.visible);
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    let open = button_entity(app.world_mut(), WorldMapButton::OpenCredits);
    let point = computed_rect(app.world(), open).center();
    pointer_press(&mut app, window, point);
    assert!(app.world().resource::<WorldMapState>().credits_visible);
    click_reachable(
        &mut app,
        window,
        WorldMapButton::NextCreditsPage,
        ScrollSurface::Credits,
    );
    assert_eq!(app.world().resource::<WorldMapState>().credits_page, 1);
    assert_reachable_content(&mut app, UVec2::new(640, 480), ScrollSurface::Credits);
    let state = app.world().resource::<WorldMapState>();
    assert!(
        format_world_map_text(WorldMapText::Credits, state, &WorldMapRaster::default())
            .contains("NOTICE 39")
    );
    click_reachable(
        &mut app,
        window,
        WorldMapButton::CloseCredits,
        ScrollSurface::Credits,
    );
    assert!(!app.world().resource::<WorldMapState>().credits_visible);
    assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
}

#[test]
fn nonfinite_and_overflowing_wheel_batches_leave_finite_scroll_unchanged() {
    let mut app = real_layout_app_at_size(full_state(), UVec2::new(640, 480));
    wheel(&mut app, -42.0, MouseScrollUnit::Pixel);
    let map = surface_entity(app.world_mut(), ScrollSurface::Map);
    let before = app.world().get::<ScrollPosition>(map).unwrap().y;
    assert_eq!(before, 42.0);
    for batch in [
        vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        vec![f32::MAX; 4],
        vec![-f32::MAX; 4],
        vec![f32::MAX, f32::MAX, -f32::MAX, -f32::MAX],
    ] {
        for y in batch {
            app.world_mut().write_message(MouseWheel {
                unit: MouseScrollUnit::Pixel,
                x: 0.0,
                y,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
        let position = app.world().get::<ScrollPosition>(map).unwrap().y;
        assert!(position.is_finite());
        assert_eq!(position, before);
    }
}

#[test]
fn weather_child_real_pointer_scroll_cancel_and_reopen_keep_map_ownership() {
    for size in [UVec2::new(640, 480), UVec2::new(1024, 720), UVec2::new(1280, 720)] {
        let (mut app, window) = interactive_app(size);
        let point = reveal_button(&mut app, WorldMapButton::OpenWeatherSettings, ScrollSurface::Map);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line, x: 0.0, y: -10000.0, window,
        });
        pointer_press(&mut app, window, point);
        let weather = surface_entity(app.world_mut(), ScrollSurface::Weather);
        assert_eq!(app.world().get::<ScrollPosition>(weather).unwrap().y, 0.0);
        assert!(app.world().resource::<WorldMapState>().weather_editor.is_some());
        let map = surface_entity(app.world_mut(), ScrollSurface::Map);
        let map_position = app.world().get::<ScrollPosition>(map).unwrap().y;
        let generation = app.world().resource::<WorldMapActions>().generation;
        wheel(&mut app, -10000.0, MouseScrollUnit::Line);
        assert!(app.world().resource::<WorldMapActions>().generation > generation);
        assert_eq!(app.world().get::<ScrollPosition>(map).unwrap().y, map_position);
        assert_reachable_content(&mut app, size, ScrollSurface::Weather);
        let point = reveal_button(&mut app, WorldMapButton::Weather(WeatherSettingsButton::Cancel), ScrollSurface::Weather);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line, x: 0.0, y: -10000.0, window,
        });
        pointer_press(&mut app, window, point);
        assert_eq!(app.world().get::<ScrollPosition>(map).unwrap().y, map_position);
        assert!(app.world().resource::<WorldMapState>().weather_editor.is_none());
        assert!(app.world().resource::<WorldMapActions>().weather.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
        click_reachable(&mut app, window, WorldMapButton::OpenWeatherSettings, ScrollSurface::Map);
        assert_eq!(app.world().get::<ScrollPosition>(weather).unwrap().y, 0.0);
        click_reachable(&mut app, window, WorldMapButton::Weather(WeatherSettingsButton::Apply), ScrollSurface::Weather);
        assert_eq!(app.world().resource::<WorldMapActions>().weather, Some(WorldMapWeatherEdit::default()));
        assert!(app.world().resource::<WorldMapState>().weather_editor.is_none());
        assert!(app.world().resource::<WorldMapActions>().start_at.is_none());
    }
}
