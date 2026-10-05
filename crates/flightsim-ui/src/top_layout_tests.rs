//! Real Taffy/Cosmic Text regression for the native jet notice/stall collision.
//! No renderer, GPU, or operating-system window is needed.
use super::*;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::RunSystemOnce;
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
    .init_resource::<DataAttribution>()
    .init_resource::<FlightGuidance>()
    .init_resource::<TutorialState>()
    .init_resource::<TutorialVisibility>()
    .init_resource::<Paused>()
    .init_resource::<CrashNotice>()
    .add_systems(
        Startup,
        (
            spawn_hud,
            replay::spawn_replay_banner,
            spawn_tutorial_prompt,
            pause::spawn_pause_overlay,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (
            update_hud,
            replay::update_replay_banner,
            update_help_for_replay,
            update_flight_log_display,
            update_data_attribution_display,
            update_tutorial_prompt,
            pause::update_pause_overlay,
            pause::update_pause_reference,
            pause::scroll_pause_reference,
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

fn cockpit_layout_app(size: UVec2, state: HudState, status: ReplayStatus) -> App {
    let mut app = layout_app(size, state, status);
    app.init_asset::<attitude::AttitudeMaterial>().add_systems(
        Update,
        (
            instruments::update_instrument_visibility,
            instruments::update_instruments,
        ),
    );
    app.world_mut()
        .run_system_once(instruments::spawn_instrument_panel)
        .unwrap();
    app.update();
    app
}

#[test]
fn narrow_cockpit_replay_help_leaves_all_six_dials_readable() {
    // The native 830x582 replay capture put compact help over the V/S dial.
    // Previous complete-HUD tests did not spawn the separate cockpit panel.
    let size = UVec2::new(830, 582);
    let mut state = flight_hud(false);
    state.stall_warning = false;
    state.view_mode = "COCKPIT";
    state.draw_distance = "STANDARD 4.5km";
    let mut app = cockpit_layout_app(
        size,
        state,
        ReplayStatus {
            active: true,
            finished: true,
            ..default()
        },
    );
    app.world_mut()
        .resource_mut::<DataAttribution>()
        .set(FULL_CREDIT);
    app.update();
    assert_complete_hud_clear(&mut app, size);
    let world = app.world_mut();
    let cockpit = entity::<instruments::InstrumentPanel>(world);
    assert_eq!(
        world.get::<Visibility>(cockpit),
        Some(&Visibility::Inherited)
    );
    let dial_bounds = rect(world, cockpit);
    for (name, panel) in [
        ("help", entity::<HudHelp>(world)),
        ("HUD", entity::<HudText>(world)),
        ("log", entity::<HudLog>(world)),
        ("notice", entity::<replay::ReplayBannerPanel>(world)),
        ("credit", entity::<DataAttributionDisplay>(world)),
    ] {
        assert!(
            dial_bounds.intersect(rect(world, panel)).is_empty(),
            "cockpit overlaps {name}: {dial_bounds:?}, {:?}",
            rect(world, panel)
        );
    }
}

fn assert_cockpit_replay_help_and_notices_clear(app: &mut App, size: UVec2) {
    let unavailable = app.world().resource::<HudState>().stall_warning_unavailable;
    assert_notice_readable(app, size, unavailable);
    assert_complete_hud_clear(app, size);
    let world = app.world_mut();
    let cockpit = entity::<instruments::InstrumentPanel>(world);
    let dial_bounds = rect(world, cockpit);
    assert_eq!(
        world.get::<Visibility>(cockpit),
        Some(&Visibility::Inherited)
    );
    for (name, panel) in [
        ("help", entity::<HudHelp>(world)),
        ("log", entity::<HudLog>(world)),
        ("credit", entity::<DataAttributionDisplay>(world)),
    ] {
        assert!(
            dial_bounds.intersect(rect(world, panel)).is_empty(),
            "cockpit overlaps {name} at {size:?}: {dial_bounds:?}, {:?}",
            rect(world, panel)
        );
    }
    // 640px retains a separate pre-existing HUD/long-notice versus dial
    // collision. This scoped help fix does not claim full cockpit clearance
    // there; the text panels themselves must still remain readable and apart.
    if size.x >= 830 {
        let notice = entity::<replay::ReplayBannerPanel>(world);
        assert!(dial_bounds.intersect(rect(world, notice)).is_empty());
    }
    let help = entity::<HudHelp>(world);
    let banner = entity::<ReplayBanner>(world);
    for key in [
        "F5", "F6", "F7", "F8", "C", "M", "F10", "F11", "F12", "ignored",
    ] {
        assert!(
            world.get::<Text>(help).unwrap().contains(key),
            "replay help lost {key}"
        );
    }
    assert_eq!(
        world.get::<Text>(banner).unwrap().as_str(),
        format_replay_banner(world.resource::<ReplayStatus>())
    );
    // Every control and notice byte must produce a real glyph after wrapping;
    // retaining only the source string, or clipping/hiding it, is not a pass.
    for text_entity in [help, banner] {
        let mut ancestor = Some(text_entity);
        while let Some(entity) = ancestor {
            assert_ne!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
            assert_ne!(world.get::<Node>(entity).unwrap().display, Display::None);
            ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
        }
        let measured = world.get::<TextLayoutInfo>(text_entity).unwrap();
        let bounds = rect(world, text_entity);
        assert!(!measured.glyphs.is_empty());
        assert!(measured.size.x <= bounds.width() + 1.0);
        assert!(measured.size.y <= bounds.height() + 1.0);
        for (line_index, line) in world.get::<Text>(text_entity).unwrap().lines().enumerate() {
            for (index, _) in line
                .bytes()
                .enumerate()
                .filter(|(_, b)| !b.is_ascii_whitespace())
            {
                assert!(
                    measured.glyphs.iter().any(|g| g.line_index == line_index
                        && (g.byte_index..g.byte_index + g.byte_length).contains(&index)),
                    "byte {index} on line {line_index} was not laid out"
                );
            }
        }
    }
    let readouts: Vec<_> = world
        .query_filtered::<Entity, With<instruments::InstrumentReadout>>()
        .iter(world)
        .collect();
    assert_eq!(readouts.len(), 6);
    for readout in readouts {
        let measured = world.get::<TextLayoutInfo>(readout).unwrap();
        assert!(!measured.glyphs.is_empty());
        assert!(measured.size.x <= rect(world, readout).width() + 1.0);
        assert!((world.get::<TextFont>(readout).unwrap().font_size - 11.0).abs() < f32::EPSILON);
    }
}

#[test]
fn cockpit_replay_help_wraps_with_complete_status_and_compatibility_notices() {
    for size in [
        UVec2::new(640, 480),
        UVec2::new(830, 582),
        // Both sides of the compact/full reference boundary must clear dials.
        UVec2::new(899, 600),
        UVec2::new(900, 600),
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
    ] {
        for phase in 0..6 {
            let status = ReplayStatus {
                active: true,
                paused: phase == 1,
                seeking: phase == 2,
                finished: phase == 0 || phase == 4,
                fault: (phase == 5)
                    .then(|| "REPLAY STOPPED: state outside aircraft operating envelope".into()),
                notice: match phase {
                    0 => None,
                    1 => Some(RECORDING_NOTICE.into()),
                    2 => Some(JET_NOTICE.into()),
                    _ => Some(TURBOPROP_NOTICE.into()),
                },
                speed: 1.0,
                elapsed: Seconds(65.0),
                total: Seconds(195.0),
            };
            let mut state = flight_hud(phase >= 3);
            state.view_mode = "COCKPIT";
            let mut app = cockpit_layout_app(size, state, status);
            app.world_mut()
                .resource_mut::<DataAttribution>()
                .set(FULL_CREDIT);
            app.update();
            assert_cockpit_replay_help_and_notices_clear(&mut app, size);
        }
    }
}

#[test]
fn cockpit_replay_help_resizes_in_one_update_and_restores_other_views_and_live_help() {
    let mut state = flight_hud(false);
    state.view_mode = "COCKPIT";
    let mut app = cockpit_layout_app(
        UVec2::new(1280, 720),
        state,
        ReplayStatus {
            active: true,
            notice: Some(RECORDING_NOTICE.into()),
            ..default()
        },
    );
    app.world_mut()
        .resource_mut::<DataAttribution>()
        .set(FULL_CREDIT);
    for size in [
        UVec2::new(830, 582),
        UVec2::new(640, 480),
        UVec2::new(900, 600),
        UVec2::new(899, 600),
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
        UVec2::new(830, 582),
    ] {
        let world = app.world_mut();
        for mut camera in world.query::<&mut Camera>().iter_mut(world) {
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
        }
        app.update();
        assert_cockpit_replay_help_and_notices_clear(&mut app, size);
    }
    let size = UVec2::new(830, 582);
    for active in [true, false] {
        for view_mode in ["CHASE", "FREE", "TOWER", "COCKPIT"] {
            if active && view_mode == "COCKPIT" {
                continue;
            }
            let world = app.world_mut();
            world.resource_mut::<HudState>().view_mode = view_mode;
            world.resource_mut::<ReplayStatus>().active = active;
            let state = *world.resource::<HudState>();
            let status = world.resource::<ReplayStatus>().clone();
            app.update();
            let mut unchanged = layout_app(size, state, status);
            unchanged
                .world_mut()
                .resource_mut::<DataAttribution>()
                .set(FULL_CREDIT);
            unchanged.update();
            let actual_help = entity::<HudHelp>(app.world_mut());
            let expected_help = entity::<HudHelp>(unchanged.world_mut());
            assert_eq!(
                rect(app.world(), actual_help),
                rect(unchanged.world(), expected_help)
            );
            assert_eq!(
                app.world().get::<Text>(actual_help),
                unchanged.world().get::<Text>(expected_help)
            );
            assert_eq!(
                app.world().get::<Node>(actual_help).unwrap().max_width,
                Val::Percent(100.0)
            );
        }
    }
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

#[test]
fn turboprop_state_and_explicit_trim_notice_wrap_without_covering_stall_warning() {
    const NOTICE: &str = "Numerical turboprop fixture (original synthetic data) | power 32% | modeled turbine x 0.322 | shaft 180.00 rad/s | blade 23.78 deg | PARK OFF [B] | brake 0% | EXPERIMENTAL; static rotor / synthetic audio | ROLL TRIM +0.01420 | YAW TRIM +0.00109 [J/L U/O 0.01/s; Shift 0.002/s; K reset]";
    for size in [
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
        UVec2::new(640, 480),
    ] {
        for active in [false, true] {
            let status = ReplayStatus {
                active,
                notice: Some(NOTICE.into()),
                ..default()
            };
            let mut app = layout_app(size, flight_hud(true), status);
            assert_notice_readable(&mut app, size, true);
        }
    }
}

const FULL_CREDIT: &str = "Terrain: NOAA/Natural Earth/Copernicus GLO-90 | Climate: NOAA PSL 1991-2020 | (c) OpenStreetMap contributors / ODbL: openstreetmap.org/copyright";
const TURBOPROP_NOTICE: &str = "Numerical turboprop fixture (original synthetic data) | power 32% | modeled turbine x 0.322 | shaft 180.00 rad/s | blade 23.78 deg | PARK OFF [B] | brake 0% | EXPERIMENTAL; static rotor / synthetic audio | ROLL TRIM +0.01420 | YAW TRIM +0.00109 [J/L U/O 0.01/s; Shift 0.002/s; K reset]";
const TURBOPROP_COMPACT: &str = "W/S pitch; A/D roll; Q/E yaw\nPageUp/Down power; F/G flaps\n[/] pitch trim; J/L roll; U/O yaw\nShift fine; K reset roll/yaw\nSpace/B brakes; C view; M map\nR restart; F9 save replay\nEsc pause / complete controls";

fn assert_complete_hud_clear(app: &mut App, size: UVec2) {
    let world = app.world_mut();
    let help = entity::<HudHelp>(world);
    let help_rect = rect(world, help);
    assert!((help_rect.max.x - (size.as_vec2().x - 12.0)).abs() < 1.0);
    if size.x >= 900 {
        // Ordinary desktop help must leave the central 40..60% scene band
        // clear, not merely avoid the other text boxes. The native aircraft
        // occupies this area in the chase view used by the regression.
        assert!(
            help_rect.min.x >= size.as_vec2().x * 0.6,
            "desktop help covers the central flight scene at {size:?}: {help_rect:?}"
        );
    }
    let mut panels = vec![
        ("instruments", entity::<HudText>(world)),
        ("help", entity::<HudHelp>(world)),
        ("log", entity::<HudLog>(world)),
        ("credit", entity::<DataAttributionDisplay>(world)),
    ];
    for (name, panel) in [
        ("notice", entity::<replay::ReplayBannerPanel>(world)),
        ("tutorial", entity::<TutorialPrompt>(world)),
    ] {
        if world.get::<Node>(panel).unwrap().display != Display::None {
            panels.push((name, panel));
        }
    }
    for (index, (name, panel)) in panels.iter().enumerate() {
        let bounds = rect(world, *panel);
        assert!(
            bounds.min.x >= 11.0
                && bounds.max.x <= size.as_vec2().x - 11.0
                && bounds.min.y >= 11.0
                && bounds.max.y <= size.as_vec2().y - 11.0,
            "{name} leaves {size:?}: {bounds:?}"
        );
        for (other_name, other) in &panels[index + 1..] {
            assert!(
                bounds.intersect(rect(world, *other)).is_empty(),
                "{name} overlaps {other_name} at {size:?}: {bounds:?}, {:?}",
                rect(world, *other)
            );
        }
        if let Some(measured) = world.get::<TextLayoutInfo>(*panel) {
            assert!(
                !measured.glyphs.is_empty(),
                "{name} must use real font layout"
            );
            assert!(
                measured.size.x <= bounds.width() + 1.0,
                "{name} glyphs exceed width"
            );
            assert!(
                measured.size.y <= bounds.height() + 1.0,
                "{name} glyphs exceed height"
            );
        }
    }
    for (marker, expected_size) in [
        (entity::<HudText>(world), 18.0),
        (entity::<HudHelp>(world), 14.0),
    ] {
        assert!(
            (world.get::<TextFont>(marker).unwrap().font_size - expected_size).abs() < f32::EPSILON
        );
    }
}

#[test]
fn complete_hud_reserves_instruments_notices_help_log_and_wrapped_credit() {
    for size in [
        UVec2::new(1280, 720),
        UVec2::new(1180, 812),
        UVec2::new(640, 480),
    ] {
        for phase in 0..4 {
            let status = ReplayStatus {
                active: phase == 3,
                notice: match phase {
                    0 => None,
                    1 => Some("ROLL TRIM +0.01420 | YAW TRIM +0.00109 [J/L U/O 0.01/s; Shift 0.002/s; K reset]".into()),
                    _ => Some(TURBOPROP_NOTICE.into()),
                },
                ..default()
            };
            let mut app = layout_app(size, flight_hud(phase >= 2), status);
            app.world_mut()
                .resource_mut::<DataAttribution>()
                .set(FULL_CREDIT);
            if phase >= 2 {
                app.insert_resource(FlightGuidance {
                    live_help: Some(format!("RUNNING TURBOPROP CONTROLS\n{TURBOPROP_COMPACT}\n\n0% power is running idle, not shutdown.\nTurbine response x is modeled, not N1.\nShaft speed and blade pitch evolve physically.\nNo automatic roll/yaw trim or attitude hold.\nExperimental numerical model; no landing grading.\nModel rotor is static; audio is synthetic.")),
                    compact_live_help: Some(TURBOPROP_COMPACT.into()),
                    tutorial_enabled: false,
                });
            }
            app.update();
            assert_complete_hud_clear(&mut app, size);
        }
    }
}

#[test]
fn resizing_and_changing_flight_guidance_keeps_every_panel_separate_in_same_update() {
    let mut app = layout_app(
        UVec2::new(1280, 720),
        flight_hud(false),
        ReplayStatus::default(),
    );
    app.world_mut()
        .resource_mut::<DataAttribution>()
        .set(FULL_CREDIT);
    for (size, replay) in [
        (UVec2::new(640, 480), false),
        (UVec2::new(1180, 812), true),
        (UVec2::new(1280, 720), false),
        (UVec2::new(640, 480), true),
    ] {
        let world = app.world_mut();
        for mut camera in world.query::<&mut Camera>().iter_mut(world) {
            camera.computed.target_info.as_mut().unwrap().physical_size = size;
            camera.viewport.as_mut().unwrap().physical_size = size;
        }
        world.resource_mut::<ReplayStatus>().active = replay;
        app.update();
        assert_complete_hud_clear(&mut app, size);
        let world = app.world_mut();
        let help = entity::<HudHelp>(world);
        let text = world.get::<Text>(help).unwrap().as_str();
        if !replay && size.x >= 900 {
            assert_eq!(text, help_text());
        } else if !replay {
            for key in [
                "S/Down",
                "W/Up",
                "A/D",
                "Left/Right",
                "Q/E",
                "PageUp/Down",
                "F/G",
                "[/]",
                "Space",
                "C view",
                "H guide",
                "M map",
                ",/.",
                "R restart",
                "F9",
                "F10/11",
                "Esc",
            ] {
                assert!(text.contains(key), "compact help lost {key}");
            }
        }
    }
}

#[test]
fn complete_pause_reference_is_readable_scrollable_and_resets_on_resume() {
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    for replay in [false, true] {
        let size = UVec2::new(640, 480);
        let mut app = layout_app(
            size,
            flight_hud(false),
            ReplayStatus {
                active: replay,
                paused: replay,
                notice: replay.then(|| TURBOPROP_NOTICE.into()),
                ..default()
            },
        );
        // Production Esc pause stays live-only. This explicitly supplied combined
        // state checks reusable UI behavior, not a new replay input/visibility path.
        app.world_mut().resource_mut::<Paused>().0 = true;
        app.world_mut()
            .resource_mut::<DataAttribution>()
            .set(FULL_CREDIT);
        app.update();
        let world = app.world_mut();
        let panel = entity::<PauseOverlay>(world);
        let reference = entity::<pause::PauseReference>(world);
        let scroll = entity::<pause::PauseReferenceScroll>(world);
        assert_eq!(world.get::<Visibility>(panel), Some(&Visibility::Visible));
        let footer = entity::<DataAttributionDisplay>(world);
        assert!(rect(world, panel).max.y < rect(world, footer).min.y);
        let reference_text = world.get::<Text>(reference).unwrap();
        if replay {
            assert!(reference_text.starts_with(replay_help_text()));
            assert!(!reference_text.contains("restart this flight"));
        } else {
            assert!(reference_text.starts_with(&help_text()));
        }
        assert!(world.get::<Text>(reference).unwrap().contains("Shift+F4"));
        let viewport = rect(world, scroll);
        assert!(viewport.min.y >= 24.0 && viewport.max.y <= 456.0);
        assert!(
            rect(world, reference).height() > viewport.height(),
            "must exercise scrolling"
        );
        world.write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: -100.0,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let world = app.world_mut();
        assert!(world.get::<ScrollPosition>(scroll).unwrap().y > 0.0);
        assert!(
            rect(world, reference).max.y <= viewport.max.y + 1.0,
            "end of reference remains inaccessible"
        );
        world.resource_mut::<Paused>().0 = false;
        app.update();
        assert!(app.world().get::<ScrollPosition>(scroll).unwrap().y.abs() < f32::EPSILON);
        assert_eq!(
            app.world().get::<Visibility>(panel),
            Some(&Visibility::Hidden)
        );
    }
}

#[test]
fn paused_overlay_reserves_wrapped_attribution_during_resize_in_the_same_update() {
    let mut app = layout_app(
        UVec2::new(1280, 720),
        flight_hud(false),
        ReplayStatus::default(),
    );
    let long_credit = format!("{FULL_CREDIT} | {FULL_CREDIT}");
    app.world_mut().resource_mut::<Paused>().0 = true;
    for size in [
        UVec2::new(640, 480),
        UVec2::new(1180, 812),
        UVec2::new(1280, 720),
    ] {
        for credit in [long_credit.as_str(), "", long_credit.as_str()] {
            let world = app.world_mut();
            for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                camera.computed.target_info.as_mut().unwrap().physical_size = size;
                camera.viewport.as_mut().unwrap().physical_size = size;
            }
            world.resource_mut::<DataAttribution>().set(credit);
            app.update();
            let world = app.world_mut();
            let panel = entity::<PauseOverlay>(world);
            let body = entity::<HudBody>(world);
            let footer = entity::<DataAttributionDisplay>(world);
            let panel_rect = rect(world, panel);
            let body_rect = rect(world, body);
            assert_eq!(world.get::<Visibility>(panel), Some(&Visibility::Visible));
            assert!(
                (world
                    .get::<BackgroundColor>(panel)
                    .unwrap()
                    .0
                    .to_srgba()
                    .alpha
                    - 1.0)
                    .abs()
                    < f32::EPSILON,
                "underlying HUD text must not bleed through the complete reference"
            );
            assert_eq!(world.get::<ChildOf>(panel).unwrap().parent(), body);
            assert!(panel_rect.min.y >= body_rect.min.y + 11.0);
            assert!(panel_rect.max.y <= body_rect.max.y - 11.0);
            assert!(panel_rect.min.x >= 23.0 && panel_rect.max.x <= size.as_vec2().x - 23.0);
            if !credit.is_empty() {
                let footer_rect = rect(world, footer);
                assert_eq!(world.get::<Text>(footer).unwrap().as_str(), credit);
                let measured = world.get::<TextLayoutInfo>(footer).unwrap();
                assert!(!measured.glyphs.is_empty());
                assert!(
                    measured.size.y > 24.0,
                    "credit must really wrap at {size:?}"
                );
                assert!(measured.size.x <= footer_rect.width() - 11.0);
                assert!(measured.size.y <= footer_rect.height() - 5.0);
                assert!(
                    panel_rect.max.y < footer_rect.min.y,
                    "opaque pause panel covers wrapped credit at {size:?}: {panel_rect:?}, {footer_rect:?}"
                );
                assert!((footer_rect.max.y - (size.as_vec2().y - 12.0)).abs() < 1.0);
            }
        }
    }
}
