//! Real Taffy/Cosmic Text regression for the native jet notice/stall collision.
//! No renderer, GPU, or operating-system window is needed.
use super::*;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::text::TextLayoutInfo;

const JET_NOTICE: &str = "Kestrel Jet Trainer (fictional) | DRY THRUST 100% | PARK OFF [B] | brake 0% | Mach 0.00..0.35 | F9 OFF: manual clouds | RECORDING STOPPED: visual time rate changed; F9 saves the valid prefix; R starts a new recording";
const RECORDING_NOTICE: &str = "RECORDING DISABLED: manual cloud weather; F9 unavailable. LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified";

fn flight_hud(unavailable: bool) -> HudState {
    HudState {
        equivalent_airspeed: MetersPerSecond(28.0),
        altitude: Meters(304.8),
        agl: Meters(304.8),
        terrain_available: true,
        stall_warning: !unavailable,
        stall_warning_unavailable: unavailable,
        view_mode: "CHASE",
        water_quality: "LIGHT",
        draw_distance: "SHORT 2.25km",
        graphics_quality: "LIGHT",
        cloud_quality: "OFF",
        cloud_source: "MODELED CLEAR",
        ..default()
    }
}

fn layout_app(size: UVec2, state: HudState, status: ReplayStatus) -> App {
    let mut app = App::new();
    // Start with current instruments, not a wall-clock-dependent first refresh
    // during font loading; later comparisons can then check stable exact text.
    let mut smoothing = HudSmoothing::default();
    smoothing.update(Seconds(1.0), &state);
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
    .insert_resource(state)
    .insert_resource(status)
    .insert_resource(smoothing)
    .add_systems(Startup, (spawn_hud, replay::spawn_replay_banner).chain())
    .add_systems(Update, (update_hud, replay::update_replay_banner));
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

fn assert_warning_clear(app: &mut App, size: UVec2, unavailable: bool) -> (Rect, Rect) {
    let world = app.world_mut();
    let hud = entity::<HudText>(world);
    let panel = entity::<replay::ReplayBannerPanel>(world);
    let hud_rect = rect(world, hud);
    let panel_rect = rect(world, panel);
    assert!(
        panel_rect.min.x >= hud_rect.max.x + 11.0 || panel_rect.min.y >= hud_rect.max.y + 7.0,
        "notice overlaps instruments at {size:?}: {panel_rect:?}, {hud_rect:?}"
    );
    assert!((hud_rect.min.x - 12.0).abs() < 1.0);
    assert!((hud_rect.min.y - 12.0).abs() < 1.0);
    assert!((world.get::<TextFont>(hud).unwrap().font_size - 18.0).abs() < f32::EPSILON);
    let text = world.get::<Text>(hud).unwrap().as_str();
    let first_line = text.lines().next().unwrap();
    assert!(first_line.ends_with(if unavailable {
        "STALL WARN N/A"
    } else {
        "STALL WARN"
    }));
    let glyphs = &world.get::<TextLayoutInfo>(hud).unwrap().glyphs;
    // Check every visible byte of the EAS/warning line, including N/A, was
    // actually laid out. Checking only Text or the node rectangle misses lost
    // glyphs or overflowing text, which caused the original visible failure.
    for (index, byte) in first_line
        .bytes()
        .enumerate()
        .filter(|(_, b)| !b.is_ascii_whitespace())
    {
        let glyph = glyphs
            .iter()
            .find(|g| {
                g.line_index == 0 && (g.byte_index..g.byte_index + g.byte_length).contains(&index)
            })
            .unwrap_or_else(|| panic!("warning byte {byte} at {index} was not laid out"));
        let bounds = Rect::from_center_size(hud_rect.min + glyph.position, glyph.size);
        assert!(
            bounds.min.x >= 11.0 && bounds.max.x <= size.as_vec2().x - 11.0,
            "warning outside viewport: {bounds:?}"
        );
        assert!(
            bounds.min.y >= 11.0 && bounds.max.y <= size.as_vec2().y - 11.0,
            "warning outside viewport: {bounds:?}"
        );
        assert!(
            bounds.intersect(panel_rect).is_empty(),
            "notice covers warning: {bounds:?}, {panel_rect:?}"
        );
    }
    (hud_rect, panel_rect)
}

fn assert_notice_readable(app: &mut App, size: UVec2, unavailable: bool) {
    let (_, panel_rect) = assert_warning_clear(app, size, unavailable);
    let world = app.world_mut();
    let banner = entity::<ReplayBanner>(world);
    let panel = entity::<replay::ReplayBannerPanel>(world);
    let text_rect = rect(world, banner);
    let measured = world.get::<TextLayoutInfo>(banner).unwrap();
    assert_eq!(world.get::<Visibility>(panel), Some(&Visibility::Visible));
    assert!((world.get::<TextFont>(banner).unwrap().font_size - 16.0).abs() < f32::EPSILON);
    assert_eq!(
        world.get::<Text>(banner).unwrap().as_str(),
        format_replay_banner(world.resource::<ReplayStatus>())
    );
    assert!(!measured.glyphs.is_empty());
    for (line_index, line) in world.get::<Text>(banner).unwrap().lines().enumerate() {
        for (index, _) in line
            .bytes()
            .enumerate()
            .filter(|(_, b)| !b.is_ascii_whitespace())
        {
            assert!(
                measured.glyphs.iter().any(|g| g.line_index == line_index
                    && (g.byte_index..g.byte_index + g.byte_length).contains(&index)),
                "notice byte {index} on line {line_index} was not laid out"
            );
        }
    }
    assert!(panel_rect.min.x >= 11.0 && panel_rect.max.x <= size.as_vec2().x - 11.0);
    assert!(
        panel_rect.min.y >= 11.0 && panel_rect.max.y <= size.as_vec2().y - 11.0,
        "notice outside viewport at {size:?}: {panel_rect:?}"
    );
    assert!(
        measured.size.x <= text_rect.width() + 1.0,
        "glyph width exceeds notice: {:?}, {text_rect:?}",
        measured.size
    );
    assert!(measured.size.y <= text_rect.height() + 1.0);
    assert!(text_rect.min.x >= panel_rect.min.x + 7.0);
    assert!(text_rect.max.x <= panel_rect.max.x - 7.0);
}

#[test]
fn live_and_replay_notices_leave_full_stall_warning_readable_at_desktop_sizes() {
    for size in [
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
        UVec2::new(640, 480),
    ] {
        for unavailable in [false, true] {
            for phase in 0..7 {
                let status = ReplayStatus {
                    active: phase >= 2,
                    paused: phase == 3,
                    seeking: phase == 4,
                    finished: phase == 5,
                    fault: (phase == 6).then(|| {
                        "REPLAY STOPPED: state outside aircraft operating envelope".into()
                    }),
                    notice: Some(
                        if phase == 1 {
                            RECORDING_NOTICE
                        } else {
                            JET_NOTICE
                        }
                        .into(),
                    ),
                    elapsed: Seconds(65.0),
                    total: Seconds(195.0),
                    speed: 1.0,
                };
                let mut app = layout_app(size, flight_hud(unavailable), status);
                assert_notice_readable(&mut app, size, unavailable);
            }
        }
    }
}

#[test]
fn resizing_and_warning_changes_reserve_new_width_in_the_same_update() {
    let mut app = layout_app(
        UVec2::new(1280, 720),
        flight_hud(false),
        ReplayStatus::default(),
    );
    for (size, unavailable) in [
        (UVec2::new(640, 480), true),
        (UVec2::new(1180, 812), false),
        (UVec2::new(1280, 720), true),
    ] {
        let world = app.world_mut();
        for mut camera in world.query::<&mut Camera>().iter_mut(world) {
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
        }
        *world.resource_mut::<HudState>() = flight_hud(unavailable);
        *world.resource_mut::<ReplayStatus>() = ReplayStatus {
            active: true,
            notice: Some(JET_NOTICE.into()),
            ..default()
        };
        app.update();
        assert_notice_readable(&mut app, size, unavailable);
    }
}

#[test]
fn very_narrow_windows_stack_notices_below_instruments_without_covering_warning() {
    // 320x480 cannot contain all instruments, help, and a long notice. Preserve
    // readable warning glyphs and horizontal bounds, without claiming the
    // entire flight UI fits this below-desktop-size viewport.
    for unavailable in [false, true] {
        let size = UVec2::new(320, 480);
        let mut app = layout_app(
            size,
            flight_hud(unavailable),
            ReplayStatus {
                notice: Some(JET_NOTICE.into()),
                ..default()
            },
        );
        let (hud, panel) = assert_warning_clear(&mut app, size, unavailable);
        assert!(panel.min.y >= hud.max.y + 7.0);
        assert!(panel.min.x >= 11.0 && panel.max.x <= 309.0);
    }
}

#[test]
fn no_notice_preserves_legacy_light_hud_geometry_and_text_after_clearing() {
    let mut state = flight_hud(false);
    state.stall_warning = false;
    state.view_mode = "COCKPIT";
    state.draw_distance = "STANDARD 4.5km";
    state.cloud_source = "MONTHLY MODEL";
    state.cloud_quality = "LIGHT";
    let mut app = layout_app(UVec2::new(1280, 720), state, ReplayStatus::default());
    let world = app.world_mut();
    let hud = entity::<HudText>(world);
    let panel = entity::<replay::ReplayBannerPanel>(world);
    let before = rect(world, hud);
    let text = world.get::<Text>(hud).unwrap().clone();
    assert!((before.min - Vec2::splat(12.0)).length() < 1.0);
    assert_eq!(world.get::<Node>(panel).unwrap().display, Display::None);
    // Compare against the original independently positioned HUD, rather than
    // merely comparing two states produced by the new shared layout.
    let legacy = world
        .spawn((
            text.clone(),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
        ))
        .id();
    app.update();
    let world = app.world_mut();
    assert_eq!(rect(world, legacy), before);
    let old_glyphs = &world.get::<TextLayoutInfo>(legacy).unwrap().glyphs;
    let new_glyphs = &world.get::<TextLayoutInfo>(hud).unwrap().glyphs;
    assert_eq!(old_glyphs.len(), new_glyphs.len());
    for (old, new) in old_glyphs.iter().zip(new_glyphs) {
        assert_eq!(old.position, new.position);
        assert_eq!(old.size, new.size);
    }
    world.resource_mut::<ReplayStatus>().notice = Some(JET_NOTICE.into());
    app.update();
    app.world_mut().resource_mut::<ReplayStatus>().notice = None;
    app.update();
    assert_eq!(rect(app.world(), hud), before);
    assert_eq!(app.world().get::<Text>(hud), Some(&text));
    assert_eq!(
        app.world().get::<Node>(panel).unwrap().display,
        Display::None
    );
}
