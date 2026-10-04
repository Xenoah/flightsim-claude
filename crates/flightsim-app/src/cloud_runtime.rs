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
    .add_systems(
        Update,
        log_cloud_status.after(flightsim_render::RenderSet::Weather),
    );
}

fn log_cloud_status(
    startup: Option<Res<Startup>>,
    status: Option<Res<CloudVolumeDiagnostics>>,
    layer: Option<Res<flightsim_render::CloudLayer>>,
    time: Option<Res<Time<Real>>>,
    weather: Option<Res<flightsim_render::RenderWeather>>,
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
    if let Some(weather) = weather
        && let flightsim_sim::weather::WeatherSelection::Modeled(scenario) = weather.selection
    {
        let p = scenario.parameters();
        info!(
            "cloud stats requested={} effective={} ready={} status={} source={} authored_not_live=true elapsed_s={} seed={} schema={} model_revision={} departure_lat_rad={} departure_lon_rad={} departure_height_m={} morphology={:?} cover={} base_m={:?} top_m={:?} cloud_visibility_m={:?} ambient_visibility_m={} fog_bottom_m={:?} fog_top_m={:?} fog_visibility_m={:?} precipitation={:?} water_equivalent_mps={} resolution={}x{} view_steps={} sun_steps={} target_bytes={} noise_bytes={} uniform_bytes={} source_bytes={} noise_generations={} density_uploads={} target_allocations={} last_upload_bytes={}",
            status.requested.name(),
            status.effective.name(),
            status.ready,
            status.status,
            super::weather_runtime::preset_label(p.preset),
            weather.elapsed.get(),
            p.seed,
            p.parameter_schema,
            p.model_revision,
            p.departure_reference.latitude.get(),
            p.departure_reference.longitude.get(),
            p.departure_reference.altitude.get(),
            p.cloud.map(|cloud| cloud.morphology),
            p.cloud.map_or(0.0, |cloud| cloud.coverage),
            p.cloud.map(|cloud| cloud.base.get()),
            p.cloud.map(|cloud| cloud.top.get()),
            p.cloud.map(|cloud| cloud.visibility.get()),
            p.ambient_visibility.get(),
            p.fog.map(|fog| fog.bottom.get()),
            p.fog.map(|fog| fog.top.get()),
            p.fog.map(|fog| fog.visibility.get()),
            p.precipitation_kind,
            p.precipitation_rate.0,
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
        return;
    }
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

pub(super) fn source_label(startup: &Startup) -> &'static str {
    effective_source_label(Some(startup.weather.selection), Some(startup))
}

pub(super) fn effective_source_label(
    selection: Option<flightsim_sim::weather::WeatherSelection>,
    startup: Option<&Startup>,
) -> &'static str {
    if let Some(flightsim_sim::weather::WeatherSelection::Modeled(scenario)) = selection {
        return super::weather_runtime::preset_label(scenario.parameters().preset);
    }
    startup.map_or("MODEL", |startup| {
        if startup.clouds_were_given {
            "USER MODEL"
        } else if startup.world.climate_enabled {
            "MONTHLY MODEL"
        } else {
            "CLEAR"
        }
    })
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
    fn authored_hud_labels_and_cloud_tier_controls_preserve_weather_identity() {
        use flightsim_sim::weather::{WeatherPreset, WeatherScenario, WeatherSelection};
        for (preset, label) in [
            (WeatherPreset::Clear, "MODELED CLEAR"),
            (WeatherPreset::Cloud, "MODELED CLOUD"),
            (WeatherPreset::Fog, "MODELED FOG"),
            (WeatherPreset::Rain, "MODELED RAIN"),
            (WeatherPreset::Snow, "MODELED SNOW"),
            (WeatherPreset::Storm, "MODELED STORM"),
        ] {
            let scenario = WeatherScenario::from_preset(
                preset,
                flightsim_core::Geodetic::from_degrees(31.5, 35.5, -430.0),
                75,
            )
            .unwrap();
            let mut startup = Startup::default();
            startup.weather.selection = WeatherSelection::Modeled(scenario);
            assert_eq!(source_label(&startup), label);
            let original = flightsim_render::RenderWeather {
                selection: startup.weather.selection,
                elapsed: flightsim_core::Seconds(17.0),
            };
            let mut app = App::new();
            app.insert_resource(original)
                .init_resource::<CloudQuality>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_systems(Update, select_cloud_quality);
            for _ in 0..5 {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .reset_all();
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::F3);
                app.update();
                assert_eq!(
                    *app.world().resource::<flightsim_render::RenderWeather>(),
                    original
                );
            }
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
