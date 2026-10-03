//! Opt-in lifecycle evidence, separate from frame-time or GPU-memory claims.

use bevy::light::GeneratedEnvironmentMapLight;
use bevy::pbr::generate::{IntermediateTextures, RenderEnvironmentMap};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use std::time::Instant;

use super::{GraphicsQuality, GraphicsQualityCamera, QualityEnvironment, apply_quality};

/// Enabled by app's explicit `--render-stats` option; silent by default.
#[derive(Resource, Debug, Default, Clone, ExtractResource)]
pub struct GraphicsQualityDiagnostics {
    pub enabled: bool,
}

#[derive(Resource, Default)]
struct ReportTime(f64);

pub(super) fn configure(app: &mut App) {
    app.init_resource::<GraphicsQualityDiagnostics>()
        .init_resource::<ReportTime>()
        .add_plugins(ExtractResourcePlugin::<GraphicsQualityDiagnostics>::default())
        .add_systems(PostUpdate, report_main.after(apply_quality));
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(Render, report_gpu.in_set(RenderSystems::Cleanup));
    }
}

fn report_main(world: &mut World) {
    if !world.resource::<GraphicsQualityDiagnostics>().enabled {
        return;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let mut last_report = world.resource_mut::<ReportTime>();
    if now - last_report.0 < 5.0 {
        return;
    }
    last_report.0 = now;
    let quality = *world.resource::<GraphicsQuality>();
    let cameras: Vec<_> = world.query_filtered::<(Entity, &Camera, Option<&EnvironmentMapLight>), With<GraphicsQualityCamera>>()
        .iter(world).map(|(entity, camera, map)| (entity, camera.is_active, map.is_some())).collect();
    let helpers: Vec<_> = world
        .query_filtered::<(
            Entity,
            Option<&GeneratedEnvironmentMapLight>,
            Option<&EnvironmentMapLight>,
        ), With<QualityEnvironment>>()
        .iter(world)
        .map(|(entity, generated, map)| {
            let mut images = Vec::with_capacity(3);
            if let Some(generated) = generated {
                images.push(generated.environment_map.id());
            }
            if let Some(map) = map {
                images.extend([map.diffuse_map.id(), map.specular_map.id()]);
            }
            (entity, images)
        })
        .collect();
    let image_count = world.get_resource::<Assets<Image>>().map_or(0, Assets::len);
    let mesh_count = world.get_resource::<Assets<Mesh>>().map_or(0, Assets::len);
    let material_count = world
        .get_resource::<Assets<StandardMaterial>>()
        .map_or(0, Assets::len);
    info!(
        "graphics main assets: tier {}, flight cameras (id,active,IBL) {:?}, helpers (id,owned image ids) {:?}; total images {image_count}, meshes {mesh_count}, standard materials {material_count}; image handles do not prove GPU readiness",
        quality.name(),
        cameras,
        helpers
    );
}

fn report_gpu(
    options: Res<GraphicsQualityDiagnostics>,
    images: Res<RenderAssets<GpuImage>>,
    helpers: Query<(Entity, &MainEntity), With<RenderEnvironmentMap>>,
    intermediate: Query<(), With<IntermediateTextures>>,
    mut last_report: Local<Option<Instant>>,
) {
    if !options.enabled || last_report.is_some_and(|last| last.elapsed().as_secs_f64() < 5.0) {
        return;
    }
    *last_report = Some(Instant::now());
    let helper_entities: Vec<_> = helpers
        .iter()
        .map(|(render, main)| (render, main.id()))
        .collect();
    info!(
        "graphics render assets: GPU images {}, generated helpers (render,main) {:?}, intermediate owners {}; excludes retained TextureCache/driver allocations; no GPU timing claim",
        images.iter().count(),
        helper_entities,
        intermediate.iter().count()
    );
}
