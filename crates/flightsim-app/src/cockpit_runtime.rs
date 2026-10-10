//! App-owned cockpit interaction and presentation. Physics still consumes only
//! the existing fixed-step SampledPilotInput; replay bytes and laws are unchanged.
use super::*;
use bevy::math::Affine3A;
use bevy::window::PrimaryWindow;
use flightsim_render::cockpit::{self, CockpitControl, CockpitState, Display, Motion};

#[derive(Component)]
pub(super) struct AnimatedPart {
    pub base: Transform,
    pub motion: Motion,
}
#[derive(Component)]
pub(super) struct PanelDisplay {
    pub display: Display,
    pub image: Handle<Image>,
}
#[derive(Component)]
pub(super) struct HitTarget {
    pub action: CockpitControl,
    pub size: Vec2,
    pub inverse_rest_rotation: Quat,
}
#[derive(Component)]
struct CockpitHint;
#[derive(Component)]
pub(super) struct IlluminatedNeedle;
#[derive(Component)]
pub(super) struct CabinMaterial(pub Color);
type OrdinaryHud = (
    Or<(
        With<flightsim_ui::HudText>,
        With<flightsim_ui::HudHelp>,
        With<flightsim_ui::HudLog>,
    )>,
    Without<CockpitHint>,
);
#[derive(Resource, Debug)]
pub(super) struct CockpitInteraction {
    enabled: bool,
    scene: Option<Entity>,
    held: Option<CockpitControl>,
    anchor: Vec2,
    previous_cursor: Option<Vec2>,
    hovered: Option<CockpitControl>,
    yaw: Radians,
    pitch: Radians,
    lighting: bool,
    show_hud: bool,
}
impl Default for CockpitInteraction {
    fn default() -> Self {
        Self {
            enabled: false,
            scene: None,
            held: None,
            anchor: Vec2::ZERO,
            previous_cursor: None,
            hovered: None,
            yaw: Radians(5f64.to_radians()),
            pitch: Radians(15f64.to_radians()),
            lighting: true,
            show_hud: false,
        }
    }
}
impl CockpitInteraction {
    pub(super) fn view_rotation(&self) -> Quat {
        if !self.enabled {
            return Quat::IDENTITY;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "bounded view angles are renderer f32"
        )]
        let rotation = Quat::from_rotation_z(self.yaw.get() as f32)
            * Quat::from_rotation_y(-self.pitch.get() as f32);
        rotation
    }
    fn release(&mut self) {
        self.held = None;
        self.hovered = None;
        self.previous_cursor = None;
    }
    fn center(&mut self) {
        self.yaw = Radians(5f64.to_radians());
        self.pitch = Radians(15f64.to_radians());
    }
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<CockpitInteraction>()
        .init_resource::<flightsim_ui::instruments::CockpitPanel3d>()
        .add_systems(Startup, spawn_hint)
        .add_systems(
            Update,
            sample_pointer
                .after(flightsim_input::InputSystems::Sample)
                .after(world_runtime::capture_map_input)
                .after(world_runtime::apply_world_map_start)
                .before(control_flight)
                .before(suspend_pilot_controls)
                .before(update_camera),
        )
        .add_systems(
            Update,
            publish_panel
                .after(publish_hud)
                .before(update_camera)
                .before(flightsim_ui::instruments::update_instrument_visibility),
        );
}
fn spawn_hint(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(32.0),
            top: Val::Px(90.0),
            max_width: Val::Percent(65.0),
            padding: UiRect::all(Val::Px(5.0)),
            ..default()
        },
        Text::new("3D COCKPIT: right-drag look | Home center | V HUD | hover a control"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgb(0.94, 0.92, 0.80)),
        BackgroundColor(Color::srgba(0.015, 0.024, 0.03, 0.82)),
        Visibility::Hidden,
        CockpitHint,
    ));
}

/// Rectangular front surface in target-local coordinates (normal -X). Keep the
/// incoming direction unnormalised after inverse transformation: t remains the
/// original world-ray distance, so differently scaled targets compare correctly.
fn ray_hit(origin: Vec3, direction: Vec3, target: Affine3A, size: Vec2) -> Option<f32> {
    if !origin.is_finite()
        || !direction.is_finite()
        || !target.is_finite()
        || !size.is_finite()
        || size.min_element() <= 0.0
        || target.matrix3.determinant().abs() < 1e-10
    {
        return None;
    }
    let inverse = target.inverse();
    let origin = inverse.transform_point3(origin);
    let direction = inverse.transform_vector3(direction);
    if direction.x <= 1e-6 {
        return None;
    }
    let distance = -origin.x / direction.x;
    if !distance.is_finite() || distance <= 0.0 {
        return None;
    }
    let hit = origin + direction * distance;
    (hit.y.abs() <= size.x * 0.5 && hit.z.abs() <= size.y * 0.5).then_some(distance)
}

fn merge_command(action: CockpitControl, delta: Vec2, keys: &mut flightsim_input::PilotKeys) {
    let up = delta.y < -4.0;
    let down = delta.y > 4.0;
    match action {
        CockpitControl::Throttle => {
            keys.throttle_up |= up;
            keys.throttle_down |= down;
        }
        CockpitControl::Flaps => {
            keys.flaps_extend |= down;
            keys.flaps_retract |= up;
        }
        CockpitControl::Trim => {
            keys.trim_up |= down;
            keys.trim_down |= up;
        }
        CockpitControl::Brake => keys.brakes = true,
        CockpitControl::RudderLeft => keys.yaw_left = true,
        CockpitControl::RudderRight => keys.yaw_right = true,
        CockpitControl::Yoke => {
            keys.roll_left |= delta.x < -4.0;
            keys.roll_right |= delta.x > 4.0;
            keys.pitch_up |= down;
            keys.pitch_down |= up;
        }
        CockpitControl::Lighting | CockpitControl::CenterView | CockpitControl::Hud => {}
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "cockpit picking explicitly gates every independently owned simulation and UI state"
)]
fn sample_pointer(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<world_runtime::FlightCamera>>,
    targets: Query<(&HitTarget, &GlobalTransform, &Transform), With<InteriorModel>>,
    aircraft: Query<Entity, With<Aircraft>>,
    simulation: Res<FlightSimulation>,
    playback: Option<Res<ReplayPlayback>>,
    mode: Res<ViewMode>,
    paused: Res<flightsim_ui::Paused>,
    capture: Res<world_runtime::MapCapture>,
    mut pointer: ResMut<CockpitInteraction>,
    mut sample: ResMut<SampledPilotInput>,
) {
    let scene = aircraft.single().ok();
    let changed_scene = scene != pointer.scene;
    if changed_scene {
        pointer.scene = scene;
        pointer.release();
        pointer.center();
    }
    pointer.enabled = !targets.is_empty() && !simulation.0.uses_bounded_model();
    if changed_scene {
        return;
    }
    let Ok(window) = windows.single() else {
        pointer.release();
        return;
    };
    if !pointer.enabled || *mode != ViewMode::Cockpit || capture.captured || !window.focused {
        pointer.release();
        return;
    }
    if keyboard.just_pressed(KeyCode::Home) || keyboard.just_pressed(KeyCode::KeyR) {
        pointer.center();
        pointer.release();
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyV) {
        pointer.show_hud = !pointer.show_hud;
    }
    let Some(cursor) = window.cursor_position() else {
        pointer.release();
        return;
    };
    // Looking is safe in pause/replay, but never turns a held control into input.
    if mouse.pressed(MouseButton::Right) {
        pointer.held = None;
        pointer.hovered = None;
        if let Some(previous) = pointer.previous_cursor {
            let delta = cursor - previous;
            let (yaw, pitch) = flightsim_input::camera::clamp_look_around(
                Radians(pointer.yaw.get() + f64::from(delta.x) * 0.0035),
                Radians(pointer.pitch.get() + f64::from(delta.y) * 0.0035),
            );
            pointer.yaw = yaw;
            pointer.pitch = pitch;
        }
        pointer.previous_cursor = Some(cursor);
        return;
    }
    pointer.previous_cursor = None;
    pointer.hovered = None;
    if let Ok((camera, global)) = cameras.single()
        && camera.is_active
        && let Ok(ray) = camera.viewport_to_world(global, cursor)
    {
        let mut nearest = f32::INFINITY;
        for (target, global, local) in &targets {
            // The symmetric trim wheel spins through full revolutions; its hit
            // proxy must remain reachable at every trim setting.
            let rotation = if target.action == CockpitControl::Trim {
                local.rotation.inverse()
            } else {
                target.inverse_rest_rotation
            };
            let surface = global.affine() * Affine3A::from_quat(rotation);
            if let Some(distance) = ray_hit(ray.origin, *ray.direction, surface, target.size)
                && distance < nearest
            {
                nearest = distance;
                pointer.hovered = Some(target.action);
            }
        }
    }
    let blocked = paused.is_paused()
        || simulation.0.crashed()
        || simulation.0.diverged()
        || playback.is_some()
        || simulation.0.is_replay()
        || simulation.0.terminal_message().is_some();
    // Presentation switches remain usable in pause/replay; flight commands do not.
    if mouse.just_pressed(MouseButton::Left) {
        pointer.anchor = cursor;
        match pointer.hovered {
            Some(CockpitControl::Lighting) => {
                pointer.lighting = !pointer.lighting;
                pointer.held = None;
            }
            Some(CockpitControl::Hud) => {
                pointer.show_hud = !pointer.show_hud;
                pointer.held = None;
            }
            Some(CockpitControl::CenterView) => {
                pointer.center();
                pointer.held = None;
            }
            action => pointer.held = if blocked { None } else { action },
        }
    }
    if blocked || !mouse.pressed(MouseButton::Left) {
        pointer.held = None;
    }
    if let Some(action) = pointer.held {
        merge_command(action, cursor - pointer.anchor, &mut sample.keys);
    }
}

#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    reason = "presentation binds HUD units, recorded controls, textures and aircraft-local geometry without cross-crate dependencies"
)]
fn publish_panel(
    hud: Res<HudState>,
    controls: Res<PilotControls>,
    simulation: Res<FlightSimulation>,
    playback: Option<Res<ReplayPlayback>>,
    pointer: Res<CockpitInteraction>,
    (capture, paused): (Res<world_runtime::MapCapture>, Res<flightsim_ui::Paused>),
    time: Res<Time>,
    mut next_texture: Local<f64>,
    mut parts: Query<(&AnimatedPart, &mut Transform), With<InteriorModel>>,
    displays: Query<(
        &PanelDisplay,
        &MeshMaterial3d<StandardMaterial>,
        Ref<InteriorModel>,
    )>,
    (needles, cabin): (
        Query<&MeshMaterial3d<StandardMaterial>, (With<InteriorModel>, With<IlluminatedNeedle>)>,
        Query<(&CabinMaterial, &MeshMaterial3d<StandardMaterial>), With<InteriorModel>>,
    ),
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fallback: ResMut<flightsim_ui::instruments::CockpitPanel3d>,
    mut hints: Query<(&mut Text, &mut Visibility), With<CockpitHint>>,
    mut hud_elements: Query<&mut Visibility, OrdinaryHud>,
) {
    fallback.0 = !parts.is_empty();
    let effective = playback.as_ref().map_or_else(
        || {
            simulation
                .0
                .last_controls()
                .unwrap_or_else(|| controls.to_control_inputs())
        },
        |p| p.last_controls,
    );
    let replaying = playback.is_some() || simulation.0.is_replay();
    let mechanical = mechanical_controls(&controls, effective, replaying);
    let state = CockpitState {
        airspeed: hud.equivalent_airspeed,
        altitude: hud.altitude,
        agl: hud.agl,
        vertical_speed: hud.vertical_speed,
        heading: hud.heading,
        pitch: hud.pitch,
        roll: hud.roll,
        yaw_rate: flightsim_core::RadiansPerSecond(simulation.0.state().angular_velocity.z),
        ground_speed: simulation.0.state().ground_speed(),
        wind_speed: hud.wind_speed,
        wind_from: hud.wind_from,
        controls: mechanical,
        trim: hud.trim,
        replay: playback.is_some() || simulation.0.is_replay(),
        stall: hud.stall_warning,
        stall_unavailable: hud.stall_warning_unavailable,
        on_ground: hud.on_ground,
        lighting: pointer.lighting,
        show_hud: pointer.show_hud,
    };
    for (part, mut transform) in &mut parts {
        *transform = cockpit::animated_transform(part.base, part.motion, &state);
    }
    let update = time.elapsed_secs_f64() >= *next_texture;
    if update {
        *next_texture = time.elapsed_secs_f64() + 0.05;
    }
    // At night, the switch actually controls readable instrument illumination.
    let daylight = ((hud.sun_elevation.get() + 0.12) / 0.30).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "display brightness is a bounded colour multiplier"
    )]
    let brightness = if pointer.lighting {
        0.92
    } else {
        (0.025 + daylight * 0.78) as f32
    };
    for (base, handle) in &cabin {
        if let Some(material) = materials.get_mut(&handle.0) {
            let fill = daylight as f32 * 0.30 + if pointer.lighting { 0.025 } else { 0.0 };
            let emissive = LinearRgba::from(base.0) * fill;
            if material.emissive != emissive {
                material.emissive = emissive;
            }
        }
    }
    for material in &needles {
        if let Some(material) = materials.get_mut(&material.0) {
            let color = Color::srgb(brightness * 0.97, brightness * 0.95, brightness * 0.83);
            if !material.unlit {
                material.unlit = true;
            }
            if material.base_color != color {
                material.base_color = color;
            }
        }
    }
    for (display, material, active) in &displays {
        if (update || active.is_added())
            && display.display.dynamic()
            && let Some(image) = images.get_mut(&display.image)
        {
            *image = cockpit::display_image(display.display, &state);
        }
        if let Some(material) = materials.get_mut(&material.0) {
            let color = Color::srgb(brightness, brightness, brightness);
            if material.base_color != color {
                material.base_color = color;
            }
        }
    }
    for mut visibility in &mut hud_elements {
        *visibility = if !pointer.enabled
            || hud.view_mode != "COCKPIT"
            || pointer.show_hud
            || paused.is_paused()
        {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut text, mut visibility) in &mut hints {
        *visibility = if pointer.enabled && hud.view_mode == "COCKPIT" && !capture.captured {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let hint = if hud.stall_warning_unavailable {
            "STALL WARNING UNAVAILABLE | V shows complete flight HUD"
        } else if paused.is_paused() {
            "PAUSED: flight controls locked | right-drag look | Home center | Esc resume"
        } else if state.replay {
            "3D COCKPIT: replay controls locked | right-drag look | Home center"
        } else {
            pointer.held.or(pointer.hovered).map_or(
                "3D COCKPIT: right-drag look | Home center | V HUD | hover a control",
                CockpitControl::hint,
            )
        };
        if text.as_str() != hint {
            **text = hint.to_owned();
        }
    }
}

fn mechanical_controls(
    controls: &PilotControls,
    effective: flightsim_fdm::ControlInputs,
    replaying: bool,
) -> flightsim_fdm::ControlInputs {
    if replaying {
        effective
    } else {
        flightsim_fdm::ControlInputs::new(
            controls.aileron.value(),
            controls.elevator.value(),
            controls.rudder.value(),
            effective.throttle(),
            effective.flaps(),
        )
        .with_brakes(effective.brakes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_hits_reject_behind_off_target_backface_and_invalid() {
        let transform = Affine3A::from_translation(Vec3::X * 2.0);
        assert_eq!(
            ray_hit(Vec3::ZERO, Vec3::X, transform, Vec2::ONE),
            Some(2.0)
        );
        assert!(ray_hit(Vec3::Y, Vec3::X, transform, Vec2::ONE).is_none());
        assert!(ray_hit(Vec3::X * 3.0, Vec3::X, transform, Vec2::ONE).is_none());
        assert!(ray_hit(Vec3::X * 3.0, -Vec3::X, transform, Vec2::ONE).is_none());
        assert!(ray_hit(Vec3::ZERO, Vec3::NAN, transform, Vec2::ONE).is_none());
        assert!(
            ray_hit(
                Vec3::ZERO,
                Vec3::X,
                Affine3A::from_scale(Vec3::ZERO),
                Vec2::ONE
            )
            .is_none()
        );
    }
    #[test]
    fn scaled_rotated_targets_keep_world_distance() {
        let transform = Affine3A::from_scale_rotation_translation(
            Vec3::new(2.0, 3.0, 1.0),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Vec3::Y * 3.0,
        );
        assert!((ray_hit(Vec3::ZERO, Vec3::Y, transform, Vec2::ONE).unwrap() - 3.0).abs() < 1e-5);
    }
    #[test]
    fn pointer_commands_have_correct_signs_and_no_ramp_mutation() {
        let mut keys = flightsim_input::PilotKeys::default();
        merge_command(CockpitControl::Throttle, Vec2::new(0.0, -30.0), &mut keys);
        merge_command(CockpitControl::Flaps, Vec2::new(0.0, 30.0), &mut keys);
        merge_command(CockpitControl::Trim, Vec2::new(0.0, 30.0), &mut keys);
        merge_command(CockpitControl::Yoke, Vec2::new(-30.0, 30.0), &mut keys);
        merge_command(CockpitControl::Brake, Vec2::ZERO, &mut keys);
        assert!(
            keys.throttle_up
                && keys.flaps_extend
                && keys.trim_up
                && keys.roll_left
                && keys.pitch_up
                && keys.brakes
        );
        assert!(
            !keys.throttle_down
                && !keys.flaps_retract
                && !keys.trim_down
                && !keys.roll_right
                && !keys.pitch_down
        );
    }
    #[test]
    fn look_yaw_right_and_pitch_down_are_body_axis_rotations() {
        let p = CockpitInteraction {
            enabled: true,
            yaw: Radians(0.2),
            pitch: Radians(0.3),
            ..default()
        };
        let forward = p.view_rotation() * Vec3::X;
        assert!(forward.y > 0.0 && forward.z > 0.0);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;
    pub(super) fn pointer_app() -> App {
        let mut app = crate::controls_runtime_tests::control_app("light-single");
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(ViewMode::Cockpit)
            .init_resource::<CockpitInteraction>();
        let aircraft = app
            .world_mut()
            .spawn((Aircraft, Transform::default(), GlobalTransform::default()))
            .id();
        app.world_mut().spawn((
            InteriorModel,
            HitTarget {
                action: CockpitControl::Brake,
                size: Vec2::ONE,
                inverse_rest_rotation: Quat::IDENTITY,
            },
            Transform::from_xyz(2.0, 0.0, 0.0),
            GlobalTransform::from_translation(Vec3::X * 2.0),
        ));
        let mut window = Window {
            focused: true,
            ..default()
        };
        window.set_cursor_position(Some(Vec2::new(100.0, 100.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        let mut pointer = app.world_mut().resource_mut::<CockpitInteraction>();
        pointer.scene = Some(aircraft);
        pointer.held = Some(CockpitControl::Brake);
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.clear();
        app
    }
    fn sample(app: &mut App) {
        app.world_mut().resource_mut::<SampledPilotInput>().keys = default();
        app.world_mut().run_system_once(sample_pointer).unwrap();
    }
    #[test]
    fn held_pointer_drops_on_pause_map_focus_view_and_cursor_loss_and_does_not_relatch() {
        for gate in 0..5 {
            let mut app = pointer_app();
            sample(&mut app);
            assert!(app.world().resource::<SampledPilotInput>().keys.brakes);
            match gate {
                0 => app
                    .world_mut()
                    .resource_mut::<flightsim_ui::Paused>()
                    .toggle(),
                1 => {
                    app.world_mut()
                        .resource_mut::<world_runtime::MapCapture>()
                        .captured = true
                }
                2 => {
                    let mut windows = app.world_mut().query::<&mut Window>();
                    windows.single_mut(app.world_mut()).unwrap().focused = false;
                }
                3 => *app.world_mut().resource_mut::<ViewMode>() = ViewMode::Chase,
                _ => {
                    let mut windows = app.world_mut().query::<&mut Window>();
                    windows
                        .single_mut(app.world_mut())
                        .unwrap()
                        .set_cursor_position(None);
                }
            }
            sample(&mut app);
            assert!(!app.world().resource::<SampledPilotInput>().keys.brakes);
            assert!(app.world().resource::<CockpitInteraction>().held.is_none());
            *app.world_mut().resource_mut::<flightsim_ui::Paused>() = default();
            app.world_mut()
                .resource_mut::<world_runtime::MapCapture>()
                .captured = false;
            *app.world_mut().resource_mut::<ViewMode>() = ViewMode::Cockpit;
            {
                let mut windows = app.world_mut().query::<&mut Window>();
                let mut window = windows.single_mut(app.world_mut()).unwrap();
                window.focused = true;
                window.set_cursor_position(Some(Vec2::new(100.0, 100.0)));
            }
            sample(&mut app);
            assert!(
                !app.world().resource::<SampledPilotInput>().keys.brakes,
                "gate {gate} relatched"
            );
        }
    }
    #[test]
    fn restart_and_scene_change_drop_drag_before_the_first_new_input() {
        let mut app = pointer_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        sample(&mut app);
        assert!(!app.world().resource::<SampledPilotInput>().keys.brakes);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().resource_mut::<CockpitInteraction>().held = Some(CockpitControl::Brake);
        let id = app
            .world_mut()
            .query_filtered::<Entity, With<Aircraft>>()
            .single(app.world())
            .unwrap();
        app.world_mut().entity_mut(id).despawn();
        app.world_mut().spawn(Aircraft);
        sample(&mut app);
        assert!(!app.world().resource::<SampledPilotInput>().keys.brakes);
    }
    #[test]
    fn cockpit_commands_preserve_fixed_step_and_replay_recording_across_frame_rates() {
        let run = |fps: u32| {
            let mut app = crate::controls_runtime_tests::control_app("light-single");
            let mut before = Duration::ZERO;
            for frame in 0..fps * 2 {
                let mut keys = flightsim_input::PilotKeys::default();
                if frame < fps {
                    merge_command(CockpitControl::Throttle, Vec2::new(0.0, -30.0), &mut keys);
                } else {
                    merge_command(CockpitControl::Trim, Vec2::new(0.0, 30.0), &mut keys);
                    merge_command(CockpitControl::Flaps, Vec2::new(0.0, 30.0), &mut keys);
                }
                let now = Duration::from_secs_f64(f64::from(frame + 1) / f64::from(fps));
                crate::controls_runtime_tests::tick(&mut app, now - before, keys);
                before = now;
            }
            app
        };
        let reference = run(60);
        let frames = reference.world().resource::<FlightRecorder>().0.recording();
        for fps in [6, 30, 144] {
            let result = run(fps);
            assert_eq!(
                reference.world().resource::<FlightSimulation>().0.state(),
                result.world().resource::<FlightSimulation>().0.state()
            );
            assert_eq!(
                frames.frames(),
                result
                    .world()
                    .resource::<FlightRecorder>()
                    .0
                    .recording()
                    .frames()
            );
        }
    }
    #[test]
    fn zero_step_frame_cannot_advance_a_cockpit_ramp() {
        let mut app = crate::controls_runtime_tests::control_app("light-single");
        let throttle = app.world().resource::<PilotControls>().throttle.value();
        let mut keys = flightsim_input::PilotKeys::default();
        merge_command(CockpitControl::Throttle, Vec2::new(0.0, -50.0), &mut keys);
        crate::controls_runtime_tests::tick(&mut app, Duration::ZERO, keys);
        assert_eq!(
            app.world()
                .resource::<PilotControls>()
                .throttle
                .value()
                .to_bits(),
            throttle.to_bits()
        );
        assert!(
            app.world()
                .resource::<FlightRecorder>()
                .0
                .recording()
                .frames()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod picking_tests {
    use super::*;
    use bevy::camera::{CameraProjection, RenderTargetInfo, Viewport};
    use bevy::ecs::system::RunSystemOnce;
    fn camera() -> Camera {
        let mut camera = Camera {
            viewport: Some(Viewport {
                physical_position: UVec2::new(40, 20),
                physical_size: UVec2::new(1200, 680),
                ..default()
            }),
            ..default()
        };
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new(1280, 720),
            scale_factor: 2.0,
        });
        let mut projection = PerspectiveProjection {
            fov: 60f32.to_radians(),
            ..default()
        };
        projection.update(1200.0, 680.0);
        camera.computed.clip_from_view = projection.get_clip_from_view();
        camera
    }
    #[test]
    fn production_viewport_pick_selects_nearest_control_and_applies_actual_command() {
        let mut app = super::integration_tests::pointer_app();
        let camera = camera();
        let global = GlobalTransform::from(Transform::from_rotation(
            flightsim_render::body_to_camera_rotation(),
        ));
        let cursor = camera.world_to_viewport(&global, Vec3::X * 2.0).unwrap();
        app.world_mut()
            .spawn((camera, global, world_runtime::FlightCamera));
        {
            let mut q = app.world_mut().query::<&mut Window>();
            q.single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(Some(cursor));
        }
        app.world_mut().resource_mut::<CockpitInteraction>().held = None;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().run_system_once(sample_pointer).unwrap();
        assert_eq!(
            app.world().resource::<CockpitInteraction>().held,
            Some(CockpitControl::Brake)
        );
        assert!(app.world().resource::<SampledPilotInput>().keys.brakes);
        app.world_mut().spawn((
            InteriorModel,
            HitTarget {
                action: CockpitControl::Lighting,
                size: Vec2::ONE,
                inverse_rest_rotation: Quat::IDENTITY,
            },
            Transform::from_xyz(1.0, 0.0, 0.0),
            GlobalTransform::from_translation(Vec3::X),
        ));
        app.world_mut()
            .resource_mut::<flightsim_ui::Paused>()
            .toggle();
        app.world_mut().resource_mut::<SampledPilotInput>().keys = default();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().run_system_once(sample_pointer).unwrap();
        assert!(!app.world().resource::<SampledPilotInput>().keys.brakes);
        assert!(!app.world().resource::<CockpitInteraction>().lighting);
        assert!(app.world().resource::<CockpitInteraction>().held.is_none());
    }
    #[test]
    fn every_production_control_center_is_reachable_without_other_control_occlusion() {
        let eye = Vec3::new(0.0, -0.25, 0.0);
        let parts = cockpit::interior_parts([Meters(0.0), Meters(-0.25), Meters(0.0)]);
        let camera = camera();
        let targets: Vec<_> = parts
            .iter()
            .filter_map(|part| part.control.map(|(action, size)| (action, size, part)))
            .collect();
        for (wanted, _, selected) in &targets {
            let center = selected.transform.translation;
            let view = GlobalTransform::from(
                Transform::from_translation(eye).looking_at(center, Vec3::NEG_Z),
            );
            let cursor = camera.world_to_viewport(&view, center).unwrap();
            let ray = camera.viewport_to_world(&view, cursor).unwrap();
            let hit = targets
                .iter()
                .filter_map(|(action, size, part)| {
                    let surface = part.transform.compute_affine()
                        * Affine3A::from_quat(part.transform.rotation.inverse());
                    ray_hit(ray.origin, *ray.direction, surface, *size)
                        .map(|distance| (*action, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            assert_eq!(
                hit.0, *wanted,
                "{} is occluded by {:?}",
                selected.name, hit.0
            );
        }
    }
    #[test]
    fn trim_proxy_is_reachable_for_every_trim_rotation() {
        let eye = Vec3::ZERO;
        let wheel = Vec3::new(0.46, 0.0, 0.575);
        let direction = wheel.normalize();
        for trim in -100i16..=100 {
            let rotation = Quat::from_rotation_y(f32::from(trim) / 100.0 * 6.0);
            let global = Affine3A::from_rotation_translation(rotation, wheel);
            let stable = global * Affine3A::from_quat(rotation.inverse());
            assert!(
                ray_hit(eye, direction, stable, Vec2::new(0.072, 0.14)).is_some(),
                "trim {trim}"
            );
        }
    }
    #[test]
    fn keyboard_presentation_actions_do_not_require_a_cursor() {
        let mut app = super::integration_tests::pointer_app();
        {
            let mut q = app.world_mut().query::<&mut Window>();
            q.single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(None);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyV);
        app.world_mut().run_system_once(sample_pointer).unwrap();
        assert!(app.world().resource::<CockpitInteraction>().show_hud);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Home);
        app.world_mut().resource_mut::<CockpitInteraction>().pitch = Radians(0.9);
        app.world_mut().run_system_once(sample_pointer).unwrap();
        assert!(
            (app.world().resource::<CockpitInteraction>().pitch.get() - 15f64.to_radians()).abs()
                < 1e-9
        );
    }
    #[test]
    fn live_trim_does_not_displace_yoke_but_replay_retains_effective_commands() {
        let mut controls = PilotControls::default();
        controls.trim.set(0.7);
        controls.aileron_trim.set(0.1);
        controls.rudder_trim.set(-0.1);
        let effective = controls.to_control_inputs();
        let live = mechanical_controls(&controls, effective, false);
        assert!(
            live.elevator().abs() < 1e-10
                && live.aileron().abs() < 1e-10
                && live.rudder().abs() < 1e-10
        );
        let replay = mechanical_controls(&controls, effective, true);
        assert_eq!(replay, effective);
        assert!(replay.elevator() > 0.6);
    }
}
