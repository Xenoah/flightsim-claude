//! Independent, reversible cloud rendering controls. No flight-physics writes.

use bevy::prelude::*;
use flightsim_render::cloud_volume::{CloudQuality, CloudVolumeDiagnostics};

use crate::{Startup, world_runtime};

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        select_cloud_quality
            .after(world_runtime::capture_map_input)
            .before(flightsim_render::RenderSet::Weather)
            .before(super::publish_hud)
            .run_if(world_runtime::flight_controls_active),
    )
    .add_systems(Update, log_cloud_status);
}

fn log_cloud_status(
    startup: Option<Res<Startup>>,
    status: Option<Res<CloudVolumeDiagnostics>>,
    layer: Option<Res<flightsim_render::CloudLayer>>,
    time: Option<Res<Time<Real>>>,
    mut last: Local<Option<(f64, CloudQuality, CloudQuality, &'static str)>>,
) {
    let (Some(startup), Some(status), Some(layer), Some(time)) = (startup, status, layer, time)
    else {
        return;
    };
    if !startup.render_stats {
        return;
    }
    let now = time.elapsed_secs_f64();
    if last.is_some_and(|(previous, requested, effective, message)| {
        now - previous < 5.0
            && requested == status.requested
            && effective == status.effective
            && message == status.status
    }) {
        return;
    }
    *last = Some((now, status.requested, status.effective, status.status));
    info!(
        "cloud stats requested={} effective={} ready={} status={} cover={} base_m={} top_m={} visibility_m={} resolution={}x{} view_steps={} sun_steps={} target_bytes={} noise_bytes={} uniform_bytes={} source_bytes={} noise_generations={} density_uploads={} target_allocations={} last_upload_bytes={}",
        status.requested.name(),
        status.effective.name(),
        status.ready,
        status.status,
        layer.cover,
        layer.base.get(),
        layer.top.get(),
        layer.visibility.get(),
        status.target_size.x,
        status.target_size.y,
        status.view_samples,
        status.sun_samples,
        status.target_bytes,
        status.noise_bytes,
        status.uniform_bytes,
        status.source_bytes,
        status.noise_generation_count,
        status.density_upload_count,
        status.target_allocation_count,
        status.last_upload_bytes,
    );
}

fn select_cloud_quality(keyboard: Res<ButtonInput<KeyCode>>, mut quality: ResMut<CloudQuality>) {
    if keyboard.just_pressed(KeyCode::F3) {
        *quality = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
        {
            CloudQuality::Light
        } else {
            quality.next()
        };
    }
}

pub(super) const fn source_label(startup: &Startup) -> &'static str {
    if startup.clouds_were_given {
        "USER MODEL"
    } else if startup.world.climate_enabled {
        "MONTHLY MODEL"
    } else {
        "CLEAR"
    }
}

pub(super) fn quality_label(
    quality: CloudQuality,
    status: &CloudVolumeDiagnostics,
) -> &'static str {
    if quality.is_volume() && (!status.ready || status.effective_quality() != quality) {
        match quality {
            CloudQuality::High => "HIGH (LIGHT)",
            CloudQuality::Ultra => "ULTRA (LIGHT)",
            _ => quality.name(),
        }
    } else {
        quality.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_arguments_from;

    #[test]
    fn cloud_cli_is_independent_of_graphics_and_manual_weather() {
        for (name, expected) in [
            ("off", CloudQuality::Off),
            ("light", CloudQuality::Light),
            ("high", CloudQuality::High),
            ("ultra", CloudQuality::Ultra),
        ] {
            let (startup, _) = parse_arguments_from(
                [
                    "--cloud-quality",
                    name,
                    "--graphics-quality",
                    "light",
                    "--cloud-cover",
                    "0.6",
                ]
                .map(str::to_owned),
            );
            assert_eq!(startup.cloud_quality, expected);
            assert_eq!(
                startup.graphics_quality,
                flightsim_render::graphics_quality::GraphicsQuality::Light
            );
            assert_eq!(startup.clouds.cover.to_bits(), 0.6_f32.to_bits());
            assert!(startup.clouds_were_given);
            assert!(startup.graphics_error.is_none());
            assert_eq!(source_label(&startup), "USER MODEL");
        }
        let (startup, _) = parse_arguments_from(std::iter::empty());
        assert_eq!(startup.cloud_quality, CloudQuality::Light);
        assert_eq!(source_label(&startup), "MONTHLY MODEL");
        for value in ["", "HIGH", "medium", "--wind"] {
            let (startup, _) = parse_arguments_from(["--cloud-quality", value].map(str::to_owned));
            assert!(startup.graphics_error.is_some());
        }
        let (startup, _) = parse_arguments_from(["--cloud-quality".to_owned()]);
        assert!(startup.graphics_error.is_some());
    }

    #[test]
    fn cloud_cycle_and_reset_do_not_change_weather() {
        let original = flightsim_render::CloudLayer::default();
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<CloudQuality>()
            .insert_resource(original)
            .add_systems(Update, select_cloud_quality);
        for expected in [
            CloudQuality::High,
            CloudQuality::Ultra,
            CloudQuality::Off,
            CloudQuality::Light,
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F3);
            app.update();
            assert_eq!(*app.world().resource::<CloudQuality>(), expected);
            assert_eq!(
                *app.world().resource::<flightsim_render::CloudLayer>(),
                original
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
        for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
            app.insert_resource(CloudQuality::Ultra);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(shift);
            keys.press(KeyCode::F3);
            app.update();
            assert_eq!(*app.world().resource::<CloudQuality>(), CloudQuality::Light);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
    }

    #[test]
    fn map_captures_cloud_keys_and_pause_allows_them() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<CloudQuality>()
            .init_resource::<flightsim_ui::WorldMapState>()
            .init_resource::<world_runtime::MapCapture>()
            .insert_resource(flightsim_ui::Paused(true))
            .add_systems(Update, world_runtime::capture_map_input);
        configure(&mut app);
        for (map_visible, expected) in [
            (true, CloudQuality::Light),
            (false, CloudQuality::Light),
            (false, CloudQuality::High),
        ] {
            app.world_mut()
                .resource_mut::<flightsim_ui::WorldMapState>()
                .visible = map_visible;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F3);
            app.update();
            assert_eq!(*app.world().resource::<CloudQuality>(), expected);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
    }

    #[test]
    fn off_request_reaches_weather_before_the_same_frames_render_preparation() {
        #[derive(Resource, Default)]
        struct Observed(CloudQuality);
        fn observe(quality: Res<CloudQuality>, mut observed: ResMut<Observed>) {
            observed.0 = *quality;
        }
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(CloudQuality::Ultra)
            .init_resource::<Observed>()
            .init_resource::<flightsim_ui::WorldMapState>()
            .init_resource::<world_runtime::MapCapture>()
            .add_systems(Update, world_runtime::capture_map_input)
            .add_systems(Update, observe.in_set(flightsim_render::RenderSet::Weather));
        configure(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F3);
        app.update();
        assert_eq!(app.world().resource::<Observed>().0, CloudQuality::Off);
    }
}
