//! Real Taffy/Cosmic Text layout regressions; no renderer, GPU or window needed.
use super::*;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};

const FULL_CREDIT: &str = "Terrain: NOAA/NE/Copernicus | Climate: NOAA normals | (c) OpenStreetMap contributors / ODbL: openstreetmap.org/copyright; procedural heights/trees";

fn layout_app(size: UVec2, credit: &str, replay: bool) -> App {
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
    .insert_resource(DataAttribution::new(credit))
    .insert_resource(ReplayStatus {
        active: replay,
        ..default()
    })
    .init_resource::<HudState>()
    .add_systems(Startup, spawn_hud)
    .add_systems(
        Update,
        (
            update_help_for_replay,
            update_flight_log_display,
            update_data_attribution_display,
        ),
    );
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: size,
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: size,
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

fn entity<T: Component>(world: &mut World) -> Entity {
    world
        .query_filtered::<Entity, With<T>>()
        .single(world)
        .unwrap()
}

fn rect(world: &World, entity: Entity) -> Rect {
    let node = world.get::<ComputedNode>(entity).unwrap();
    let transform = world.get::<bevy::ui::UiGlobalTransform>(entity).unwrap();
    Rect::from_center_size(transform.translation, node.size())
}

fn assert_footer_clear(app: &mut App, size: UVec2) -> f32 {
    let world = app.world_mut();
    let footer = entity::<DataAttributionDisplay>(world);
    let help = entity::<HudHelp>(world);
    let log = entity::<HudLog>(world);
    let footer_rect = rect(world, footer);
    let screen = size.as_vec2();
    assert!(
        footer_rect.min.x >= 11.0 && footer_rect.max.x <= screen.x - 11.0,
        "footer exceeds viewport: {footer_rect:?}"
    );
    assert!(
        (footer_rect.max.y - (screen.y - 12.0)).abs() < 1.0,
        "footer bottom margin: {footer_rect:?}"
    );
    assert!(footer_rect.min.y >= 0.0);
    for panel in [help, log] {
        let panel_rect = rect(world, panel);
        assert!(
            panel_rect.max.y <= footer_rect.min.y - 7.0,
            "panel overlaps footer: {panel_rect:?}, {footer_rect:?}"
        );
        assert!(
            panel_rect.min.x >= 11.0 && panel_rect.max.x <= screen.x - 11.0,
            "panel exceeds horizontal viewport: {panel_rect:?}"
        );
    }
    let measured = world.get::<bevy::text::TextLayoutInfo>(footer).unwrap();
    assert!(
        !measured.glyphs.is_empty(),
        "test must exercise real font layout"
    );
    assert!(
        measured.size.x <= footer_rect.width() - 12.0 + 1.0,
        "credit glyphs exceed padded width: {:?}, {footer_rect:?}",
        measured.size
    );
    assert!(measured.size.y <= footer_rect.height() - 6.0 + 1.0);
    assert_eq!(world.get::<Text>(footer).unwrap().as_str(), FULL_CREDIT);
    footer_rect.height()
}

#[test]
fn real_layout_reserves_wrapped_credit_height_for_live_and_replay_help() {
    for replay in [false, true] {
        let mut heights = Vec::new();
        for width in [1280, 640, 320] {
            let size = UVec2::new(width, 720);
            let mut app = layout_app(size, FULL_CREDIT, replay);
            heights.push(assert_footer_clear(&mut app, size));
        }
        assert!(heights[1] > heights[0], "narrow footer must actually wrap");
        assert!(heights[2] > heights[1]);
    }
}

#[test]
fn resizing_and_switching_replay_reserves_new_height_in_the_same_update() {
    let mut app = layout_app(UVec2::new(1280, 720), FULL_CREDIT, false);
    for (size, replay) in [
        (UVec2::new(320, 720), true),
        (UVec2::new(640, 480), false),
        (UVec2::new(1280, 720), true),
    ] {
        let world = app.world_mut();
        for mut camera in world.query::<&mut Camera>().iter_mut(world) {
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
        }
        world.resource_mut::<ReplayStatus>().active = replay;
        app.update();
        assert_footer_clear(&mut app, size);
    }
}

#[test]
fn clearing_credit_restores_both_original_bottom_margins_without_changing_text() {
    let size = UVec2::new(1280, 720);
    let mut app = layout_app(size, FULL_CREDIT, false);
    assert_footer_clear(&mut app, size);
    app.world_mut().resource_mut::<DataAttribution>().clear();
    app.update();
    let world = app.world_mut();
    let help = entity::<HudHelp>(world);
    let log = entity::<HudLog>(world);
    let footer = entity::<DataAttributionDisplay>(world);
    for panel in [help, log] {
        assert!((rect(world, panel).max.y - 708.0).abs() < 1.0);
    }
    assert_eq!(world.get::<Text>(help).unwrap().as_str(), help_text());
    assert_eq!(world.get::<Node>(footer).unwrap().display, Display::None);
    assert_eq!(world.get::<Visibility>(footer), Some(&Visibility::Hidden));
    app.world_mut()
        .resource_mut::<DataAttribution>()
        .set(FULL_CREDIT);
    app.update();
    assert_footer_clear(&mut app, size);
}
