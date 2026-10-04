//! CPU/ECS acceptance for authored weather; native visual acceptance is separate.
use bevy::{mesh::VertexAttributeValues, prelude::*};
use flightsim_core::{Geodetic, Meters, Seconds};
use flightsim_render::{
    CameraWorldPosition, CloudLayer, CloudQuality, FlightsimRenderPlugin, PrecipitationDiagnostics,
    RenderOrigin, RenderWeather, TimeOfDay, weather_extinction,
};
use flightsim_sim::weather::{ModeledFogLayer, WeatherPreset, WeatherScenario, WeatherSelection};

fn departure() -> Geodetic {
    Geodetic::from_degrees(35.0, 139.0, 0.0)
}
fn scenario(preset: WeatherPreset) -> WeatherScenario {
    WeatherScenario::from_preset(preset, departure(), 31).unwrap()
}
fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(FlightsimRenderPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(RenderOrigin::new(departure()))
        .insert_resource(CameraWorldPosition(departure()));
    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), Transform::from_xyz(0.0, 100.0, 0.0)))
        .id();
    (app, camera)
}
fn select(app: &mut App, preset: WeatherPreset) {
    *app.world_mut().resource_mut::<RenderWeather>() = RenderWeather {
        selection: WeatherSelection::Modeled(scenario(preset)),
        elapsed: Seconds(17.0),
    };
}
fn density(app: &App, camera: Entity) -> f32 {
    let fog = app.world().get::<DistanceFog>(camera).unwrap();
    match fog.falloff {
        FogFalloff::Exponential { density } => density,
        _ => panic!("expected additive exponential extinction"),
    }
}
#[test]
fn fog_survives_off_light_and_pending_upper_tiers_with_identical_nominal_extinction() {
    let (mut app, camera) = app();
    let mut p = scenario(WeatherPreset::Rain).parameters();
    p.preset = WeatherPreset::Custom;
    p.fog = Some(ModeledFogLayer {
        bottom: Meters(0.0),
        top: Meters(3000.0),
        visibility: Meters(250.0),
    });
    let scenario = WeatherScenario::try_from(p).unwrap();
    *app.world_mut().resource_mut::<RenderWeather>() = RenderWeather {
        selection: WeatherSelection::Modeled(scenario),
        elapsed: Seconds(17.0),
    };
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .y = 1500.0;
    let expected = weather_extinction(
        scenario,
        Seconds(17.0),
        Geodetic::from_degrees(35.0, 139.0, 1500.0),
    )
    .total();
    for quality in [
        CloudQuality::Off,
        CloudQuality::Light,
        CloudQuality::High,
        CloudQuality::Ultra,
        CloudQuality::Off,
    ] {
        *app.world_mut().resource_mut::<CloudQuality>() = quality;
        app.update();
        assert!((density(&app, camera) - expected).abs() < 1e-5);
    }
}
#[test]
fn all_presets_transition_without_leaking_owned_precipitation_assets() {
    let (mut app, _) = app();
    for preset in [
        WeatherPreset::Clear,
        WeatherPreset::Cloud,
        WeatherPreset::Fog,
        WeatherPreset::Rain,
        WeatherPreset::Snow,
        WeatherPreset::Storm,
        WeatherPreset::Clear,
    ] {
        select(&mut app, preset);
        app.update();
        let report = *app.world().resource::<PrecipitationDiagnostics>();
        let active = matches!(
            preset,
            WeatherPreset::Rain | WeatherPreset::Snow | WeatherPreset::Storm
        );
        assert_eq!(report.draws, usize::from(active));
        assert_eq!(report.cpu_mesh_bytes, report.particles * 144);
        assert!(report.particles <= report.particle_cap && report.particle_cap <= 384);
        let cloud = matches!(
            preset,
            WeatherPreset::Cloud | WeatherPreset::Rain | WeatherPreset::Snow | WeatherPreset::Storm
        );
        let owned = usize::from(active) + usize::from(cloud);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), owned);
        assert_eq!(
            app.world().resource::<Assets<StandardMaterial>>().len(),
            owned
        );
    }
    assert!(app.world().resource::<Assets<Image>>().is_empty());
    app.world_mut().resource_mut::<RenderWeather>().selection = WeatherSelection::Legacy;
    app.update();
    assert_eq!(
        *app.world().resource::<PrecipitationDiagnostics>(),
        PrecipitationDiagnostics::default()
    );
}
fn mesh_positions(app: &mut App) -> Vec<[f32; 3]> {
    let handle = app
        .world_mut()
        .query::<(&Name, &Mesh3d)>()
        .iter(app.world())
        .find(|(name, _)| name.as_str() == "authored precipitation")
        .unwrap()
        .1
        .0
        .clone();
    let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(positions)) => positions.clone(),
        _ => panic!("precipitation position buffer"),
    }
}

#[test]
fn precipitation_seek_is_exact_even_when_solar_time_and_update_cadence_change() {
    let (mut app, _) = app();
    select(&mut app, WeatherPreset::Rain);
    app.update();
    let first = mesh_positions(&mut app);
    for time in [17.0, 18.0, 400.0, 0.0] {
        app.world_mut().resource_mut::<RenderWeather>().elapsed = Seconds(time);
        app.world_mut()
            .resource_mut::<TimeOfDay>()
            .advance(Seconds(10000.0));
        app.update();
    }
    app.world_mut().resource_mut::<RenderWeather>().elapsed = Seconds(17.0);
    app.update();
    assert_eq!(first, mesh_positions(&mut app));
}
#[test]
fn authored_clear_overrides_climate_deck_without_mutating_legacy_values() {
    let (mut app, _) = app();
    let legacy =
        CloudLayer::try_new(1.0, Meters(1000.0), Meters(2000.0), Meters(300.0), 1).unwrap();
    app.insert_resource(legacy);
    select(&mut app, WeatherPreset::Clear);
    app.update();
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert_eq!(*app.world().resource::<CloudLayer>(), legacy);
    app.world_mut().resource_mut::<RenderWeather>().selection = WeatherSelection::Legacy;
    app.update();
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
}

#[test]
fn tier_changes_keep_one_lit_depth_writing_precipitation_mesh_and_exact_source_budget() {
    let (mut app, _) = app();
    select(&mut app, WeatherPreset::Storm);
    for (quality, cap) in [
        (CloudQuality::Off, 128),
        (CloudQuality::High, 256),
        (CloudQuality::Ultra, 384),
        (CloudQuality::Light, 128),
    ] {
        *app.world_mut().resource_mut::<CloudQuality>() = quality;
        app.update();
        let report = *app.world().resource::<PrecipitationDiagnostics>();
        assert_eq!(report.particles, cap);
        assert_eq!(report.cpu_mesh_bytes, cap * 144);
        let mut query = app.world_mut().query::<(
            &Name,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Option<&bevy::light::NotShadowCaster>,
            Option<&bevy::light::NotShadowReceiver>,
        )>();
        let particles: Vec<_> = query
            .iter(app.world())
            .filter(|(name, ..)| name.as_str() == "authored precipitation")
            .collect();
        assert_eq!(particles.len(), 1);
        let (_, mesh, material, no_cast, no_receive) = particles[0];
        assert!(no_cast.is_some() && no_receive.is_some());
        let mesh = app.world().resource::<Assets<Mesh>>().get(&mesh.0).unwrap();
        assert_eq!(mesh.get_vertex_buffer_size(), report.cpu_mesh_bytes);
        assert!(mesh.indices().is_none());
        let material = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material.0)
            .unwrap();
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
        assert!(!material.unlit);
        assert_eq!(material.emissive, LinearRgba::BLACK);
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            if quality == CloudQuality::Off { 1 } else { 2 }
        );
    }
    app.world_mut().resource_mut::<RenderWeather>().selection = WeatherSelection::Legacy;
    app.update();
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .is_empty()
    );
}

#[test]
fn inactive_flight_camera_clears_precipitation_and_resume_is_exact() {
    let (mut app, camera) = app();
    app.world_mut()
        .entity_mut(camera)
        .insert(flightsim_render::CloudVolumeCamera);
    select(&mut app, WeatherPreset::Rain);
    app.update();
    let original = mesh_positions(&mut app);
    app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
    // The active 2D map camera must not keep a hidden 3D particle view alive.
    app.world_mut().spawn(Camera2d);
    app.world_mut()
        .spawn((Camera3d::default(), Transform::default()));
    app.update();
    assert_eq!(
        *app.world().resource::<PrecipitationDiagnostics>(),
        PrecipitationDiagnostics::default()
    );
    assert!(
        !app.world_mut()
            .query::<&Name>()
            .iter(app.world())
            .any(|name| name.as_str() == "authored precipitation")
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1); // cloud deck only
    app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = true;
    app.update();
    assert_eq!(mesh_positions(&mut app), original);
}

#[test]
fn cloud_top_exit_releases_precipitation_and_reentry_seek_is_exact() {
    for preset in [
        WeatherPreset::Rain,
        WeatherPreset::Snow,
        WeatherPreset::Storm,
    ] {
        let (mut app, camera) = app();
        // Cloud Off still honors authored precipitation and its fixed ceiling.
        *app.world_mut().resource_mut::<CloudQuality>() = CloudQuality::Off;
        select(&mut app, preset);
        let top = scenario(preset).parameters().cloud.unwrap().top.get();
        let place = |app: &mut App, altitude: f64| {
            let position = Geodetic::from_degrees(35.0, 139.0, altitude);
            let render = app
                .world()
                .resource::<RenderOrigin>()
                .0
                .to_render(position.to_ecef());
            app.world_mut()
                .get_mut::<Transform>(camera)
                .unwrap()
                .translation = render;
            app.world_mut().resource_mut::<CameraWorldPosition>().0 = position;
        };
        place(&mut app, top);
        app.update();
        let boundary = mesh_positions(&mut app);
        let allocated = *app.world().resource::<PrecipitationDiagnostics>();
        assert_eq!(allocated.draws, 1);
        assert_eq!(allocated.cpu_mesh_bytes, allocated.particles * 144);
        for elapsed in [17.0, 25.0, 1000.0, 0.0, 17.0] {
            app.world_mut().resource_mut::<RenderWeather>().elapsed = Seconds(elapsed);
            app.update();
        }
        assert_eq!(mesh_positions(&mut app), boundary);
        // Go well beyond the full field, also exercising repeated cleanup.
        for altitude in [top + 33.0, top + 10000.0] {
            place(&mut app, altitude);
            app.update();
            assert_eq!(
                *app.world().resource::<PrecipitationDiagnostics>(),
                PrecipitationDiagnostics::default()
            );
            assert!(app.world().resource::<Assets<Mesh>>().is_empty());
            assert!(
                app.world()
                    .resource::<Assets<StandardMaterial>>()
                    .is_empty()
            );
            assert!(
                !app.world_mut()
                    .query::<&Name>()
                    .iter(app.world())
                    .any(|name| name.as_str() == "authored precipitation")
            );
        }
        place(&mut app, top);
        app.update();
        assert_eq!(
            *app.world().resource::<PrecipitationDiagnostics>(),
            allocated
        );
        assert_eq!(mesh_positions(&mut app), boundary);

        // Absence of an authored cloud is valid Custom weather and supplies no
        // altitude ceiling, even far above the former preset's top.
        let mut custom = scenario(preset).parameters();
        custom.preset = WeatherPreset::Custom;
        custom.cloud = None;
        app.world_mut().resource_mut::<RenderWeather>().selection =
            WeatherSelection::Modeled(WeatherScenario::try_from(custom).unwrap());
        place(&mut app, top + 10000.0);
        app.update();
        assert_eq!(
            *app.world().resource::<PrecipitationDiagnostics>(),
            allocated
        );
        app.world_mut().resource_mut::<RenderWeather>().selection = WeatherSelection::Legacy;
        app.update();
        assert_eq!(
            *app.world().resource::<PrecipitationDiagnostics>(),
            PrecipitationDiagnostics::default()
        );
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .is_empty()
        );
    }
}
