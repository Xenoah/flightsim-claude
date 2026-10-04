//! Local visual detail controls. Physical terrain and flight conditions are untouched.

use bevy::prelude::*;
use flightsim_world::draw_distance::DrawDistancePreset;

use crate::{Startup, TerrainStreaming, scenery_runtime, world_runtime};

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct DrawDistanceSettings(pub DrawDistancePreset);

pub(super) fn configure(app: &mut App) {
    app.init_resource::<DrawDistanceSettings>().add_systems(
        Update,
        (select_draw_distance, apply_draw_distance)
            .chain()
            .after(world_runtime::capture_map_input)
            .before(flightsim_render::RenderSet::Terrain)
            .before(super::publish_hud),
    );
}

fn select_draw_distance(
    keyboard: Res<ButtonInput<KeyCode>>,
    capture: Option<Res<world_runtime::MapCapture>>,
    mut settings: ResMut<DrawDistanceSettings>,
) {
    if capture.is_some_and(|capture| capture.captured) || !keyboard.just_pressed(KeyCode::F2) {
        return;
    }
    settings.0 = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
        DrawDistancePreset::Standard
    } else {
        settings.0.next()
    };
}

fn apply_draw_distance(
    settings: Res<DrawDistanceSettings>,
    streaming: Option<ResMut<TerrainStreaming>>,
    scenery: Option<ResMut<scenery_runtime::SceneryRuntime>>,
    startup: Option<ResMut<Startup>>,
    config: Option<ResMut<flightsim_render::TerrainRenderConfig>>,
) {
    if !settings.is_changed() {
        return;
    }
    let policy = settings.0.policy();
    if let Some(mut streaming) = streaming {
        // Keep the displayed cut/cache until the normal bounded terrain
        // transaction has prepared a complete replacement.
        streaming.selector = policy.apply_to_selector(streaming.selector);
    }
    if let Some(mut scenery) = scenery {
        scenery.set_draw_distance(policy);
    }
    if let Some(mut startup) = startup {
        startup.draw_distance = settings.0;
    }
    if let Some(mut config) = config {
        config.screen_space_error = policy.screen_space_error();
    }
    info!(
        "local detail distance: {} scenery_m={} detail_m={} refinement_cap_m={:?} sse_px={}; coarse horizon and physics unchanged",
        settings.0,
        policy.scenery_radius().get(),
        policy.terrain_detail_radius().get(),
        policy.refinement_radius().map(|radius| radius.get()),
        policy.screen_space_error(),
    );
}

pub(super) const fn label(preset: DrawDistancePreset) -> &'static str {
    match preset {
        DrawDistancePreset::Short => "SHORT 2.25km",
        DrawDistancePreset::Standard => "STANDARD 4.5km",
        DrawDistancePreset::Long => "LONG 9km",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_selects_only_visual_distance_and_rejects_bad_input() {
        for preset in DrawDistancePreset::ALL {
            let (startup, _) =
                crate::parse_arguments_from(["--draw-distance".to_owned(), preset.to_string()]);
            assert_eq!(startup.draw_distance, preset);
            assert_eq!(startup.cloud_quality, default());
            assert_eq!(startup.graphics_quality, default());
            assert!(startup.graphics_error.is_none());
            let clock = world_runtime::startup_clock(&startup);
            let (baseline, _) = crate::parse_arguments_from(std::iter::empty());
            assert_eq!(
                crate::recording_conditions(&startup, &clock),
                crate::recording_conditions(&baseline, &clock),
                "visual distance must not change recorded flight conditions"
            );
        }
        for value in ["", "LONG", "infinite", "--wind"] {
            let (startup, _) =
                crate::parse_arguments_from(["--draw-distance", value].map(str::to_owned));
            assert!(startup.graphics_error.is_some());
        }
    }

    #[test]
    fn map_capture_blocks_cycle_and_shift_restores_standard() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<DrawDistanceSettings>()
            .init_resource::<world_runtime::MapCapture>()
            .add_systems(Update, select_draw_distance);
        app.world_mut()
            .resource_mut::<world_runtime::MapCapture>()
            .captured = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        app.update();
        assert_eq!(
            app.world().resource::<DrawDistanceSettings>().0,
            DrawDistancePreset::Standard
        );
        app.world_mut()
            .resource_mut::<world_runtime::MapCapture>()
            .captured = false;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::F2);
        }
        app.update();
        assert_eq!(
            app.world().resource::<DrawDistanceSettings>().0,
            DrawDistancePreset::Long
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::F2);
        }
        app.update();
        assert_eq!(
            app.world().resource::<DrawDistanceSettings>().0,
            DrawDistancePreset::Standard
        );
    }
}
