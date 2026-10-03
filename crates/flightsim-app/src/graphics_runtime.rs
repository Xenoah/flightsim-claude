//! Input boundary for reversible rendering presets. No simulation writes.

use bevy::prelude::*;
use flightsim_render::graphics_quality::GraphicsQuality;

use crate::world_runtime;

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        select_quality
            .after(world_runtime::capture_map_input)
            .before(super::publish_hud)
            .run_if(world_runtime::flight_controls_active),
    );
}

fn select_quality(keyboard: Res<ButtonInput<KeyCode>>, mut quality: ResMut<GraphicsQuality>) {
    if keyboard.just_pressed(KeyCode::F4) {
        *quality = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
        {
            GraphicsQuality::Light
        } else {
            quality.next()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_arguments_from;

    #[test]
    fn quality_cli_defaults_to_light_and_rejects_ambiguous_values() {
        let (default, _) = parse_arguments_from(std::iter::empty());
        assert_eq!(default.graphics_quality, GraphicsQuality::Light);
        for (value, expected) in [
            ("light", GraphicsQuality::Light),
            ("high", GraphicsQuality::High),
            ("ultra", GraphicsQuality::Ultra),
        ] {
            let (startup, _) = parse_arguments_from(
                ["--graphics-quality", value, "--surface-detail", "off"].map(str::to_owned),
            );
            assert_eq!(startup.graphics_quality, expected);
            assert!(startup.graphics_error.is_none());
            assert!(!startup.surface_detail);
        }
        for value in ["", "highest", "HIGH", "--surface-detail"] {
            let (startup, _) =
                parse_arguments_from(["--graphics-quality", value].map(str::to_owned));
            assert!(startup.graphics_error.is_some());
        }
        let (startup, _) = parse_arguments_from(["--graphics-quality".to_owned()]);
        assert!(startup.graphics_error.is_some());
    }

    #[test]
    fn repeated_key_presses_cycle_and_shift_reset_is_idempotent() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<GraphicsQuality>()
            .add_systems(Update, select_quality);
        for expected in [
            GraphicsQuality::High,
            GraphicsQuality::Ultra,
            GraphicsQuality::Light,
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F4);
            app.update();
            assert_eq!(*app.world().resource::<GraphicsQuality>(), expected);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
        for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
            for selected in [GraphicsQuality::Ultra, GraphicsQuality::Light] {
                app.insert_resource(selected);
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(shift);
                keys.press(KeyCode::F4);
                app.update();
                assert_eq!(
                    *app.world().resource::<GraphicsQuality>(),
                    GraphicsQuality::Light
                );
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .reset_all();
            }
        }
    }

    #[test]
    fn modal_map_blocks_quality_input_but_pause_does_not() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<GraphicsQuality>()
            .init_resource::<flightsim_ui::WorldMapState>()
            .init_resource::<world_runtime::MapCapture>()
            .insert_resource(flightsim_ui::Paused(true))
            .add_systems(Update, world_runtime::capture_map_input);
        configure(&mut app);
        app.world_mut()
            .resource_mut::<flightsim_ui::WorldMapState>()
            .visible = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        assert_eq!(
            *app.world().resource::<GraphicsQuality>(),
            GraphicsQuality::Light
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<flightsim_ui::WorldMapState>()
            .visible = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        // Closing frame remains captured, including a simultaneous F4 press.
        assert_eq!(
            *app.world().resource::<GraphicsQuality>(),
            GraphicsQuality::Light
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        assert_eq!(
            *app.world().resource::<GraphicsQuality>(),
            GraphicsQuality::High
        );
    }
}
