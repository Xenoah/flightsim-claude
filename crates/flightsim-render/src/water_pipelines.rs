//! Select one same-mesh draw per view; cold or failed water shaders retain baseline.
//! No frame counter, timer, synthetic pipeline descriptor or permanent mesh change.

use super::{WaterDiagnostics, WaterProxy};
use bevy::{
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass},
    pbr::{
        EntitySpecializationTicks, PreparedMaterial, RenderMaterialInstances, RenderMeshInstances,
        SpecializedMaterialPipelineCache, SpecializedPrepassMaterialPipelineCache,
        ViewPrepassSpecializationTicks, ViewSpecializationTicks, queue_material_meshes,
        queue_prepass_material_meshes,
    },
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        erased_render_asset::ErasedRenderAssets,
        mesh::RenderMesh,
        render_asset::RenderAssets,
        render_resource::{CachedPipelineState, PipelineCache},
        sync_world::MainEntity,
        view::{ExtractedView, RenderVisibleEntities, RetainedViewEntity},
    },
    shader::PipelineCacheError,
};
use std::{
    any::TypeId,
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Resource, Default)]
struct WaterPairs(Vec<(MainEntity, MainEntity)>);
#[derive(Debug, Clone, Copy, Default)]
struct GateReport {
    ready: usize,
    fallback: usize,
    failed: bool,
}
#[derive(Resource, Clone, Default)]
struct Feedback(Arc<Mutex<GateReport>>);

pub(super) fn configure(app: &mut App) {
    let feedback = Feedback::default();
    app.insert_resource(feedback.clone())
        .add_systems(Update, receive_report);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .insert_resource(feedback)
            .init_resource::<WaterPairs>()
            .add_systems(ExtractSchedule, extract_pairs)
            // Graphics' environment fallback finishes in PrepareMeshes. Match
            // its final current-view specialization, before main and prepass queue.
            .add_systems(
                Render,
                gate_water_draws
                    .in_set(RenderSystems::QueueMeshes)
                    .before(queue_material_meshes)
                    .before(queue_prepass_material_meshes),
            );
    }
}
fn receive_report(feedback: Res<Feedback>, mut diagnostics: ResMut<WaterDiagnostics>) {
    if let Ok(report) = feedback.0.lock() {
        diagnostics.ready_pairs = report.ready;
        diagnostics.fallback_pairs = report.fallback;
        diagnostics.failed_pipelines = report.failed;
    }
}
fn extract_pairs(mut pairs: ResMut<WaterPairs>, proxies: Extract<Query<(Entity, &WaterProxy)>>) {
    if proxies.is_empty() {
        pairs.0 = Vec::new();
        return;
    }
    pairs.0.clear();
    pairs.0.extend(
        proxies
            .iter()
            .map(|(entity, proxy)| (proxy.baseline.into(), entity.into())),
    );
}

fn pipeline_ready(
    world: &World,
    view: RetainedViewEntity,
    entity: MainEntity,
    needs_prepass: bool,
) -> (bool, bool) {
    let Some(cache) = world
        .resource::<SpecializedMaterialPipelineCache>()
        .get(&view)
    else {
        return (false, false);
    };
    let Some((cached_tick, id)) = cache.get(&entity) else {
        return (false, false);
    };
    let Some(view_tick) = world.resource::<ViewSpecializationTicks>().get(&view) else {
        return (false, false);
    };
    let Some(entity_tick) = world.resource::<EntitySpecializationTicks>().get(&entity) else {
        return (false, false);
    };
    let this_run = world.read_change_tick();
    if view_tick.is_newer_than(*cached_tick, this_run)
        || entity_tick
            .system_tick
            .is_newer_than(*cached_tick, this_run)
    {
        return (false, false);
    }
    let meshes = world.resource::<RenderMeshInstances>();
    let Some(mesh) = meshes.render_mesh_queue_data(entity) else {
        return (false, false);
    };
    if world
        .resource::<RenderAssets<RenderMesh>>()
        .get(mesh.mesh_asset_id)
        .is_none()
    {
        return (false, false);
    }
    let Some(material) = world
        .resource::<RenderMaterialInstances>()
        .instances
        .get(&entity)
    else {
        return (false, false);
    };
    if world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(material.asset_id)
        .is_none()
    {
        return (false, false);
    }
    let (main_ready, main_failed) = pipeline_state(world, *id);
    if !main_ready || !needs_prepass {
        return (main_ready, main_failed);
    }
    let Some(cache) = world
        .resource::<SpecializedPrepassMaterialPipelineCache>()
        .get(&view)
    else {
        return (false, false);
    };
    let Some((cached_tick, id)) = cache.get(&entity) else {
        return (false, false);
    };
    let Some(view_tick) = world
        .resource::<ViewPrepassSpecializationTicks>()
        .get(&view)
    else {
        return (false, false);
    };
    if view_tick.is_newer_than(*cached_tick, this_run)
        || entity_tick
            .system_tick
            .is_newer_than(*cached_tick, this_run)
    {
        return (false, false);
    }
    pipeline_state(world, *id)
}

fn pipeline_state(
    world: &World,
    id: bevy::render::render_resource::CachedRenderPipelineId,
) -> (bool, bool) {
    match world
        .resource::<PipelineCache>()
        .get_render_pipeline_state(id)
    {
        CachedPipelineState::Ok(_) => (true, false),
        CachedPipelineState::Err(
            PipelineCacheError::ShaderNotLoaded(_)
            | PipelineCacheError::ShaderImportNotYetAvailable,
        ) => (false, false),
        CachedPipelineState::Err(_) => (false, true),
        _ => (false, false),
    }
}

fn pair_suppression(baseline_visible: bool, proxy_visible: bool, ready: bool) -> (bool, bool) {
    // No proxy may draw if its baseline wasn't in this view's visible cut.
    // This also protects a just-cancelled or replaced terrain entity.
    (
        baseline_visible && proxy_visible && ready,
        !baseline_visible || !proxy_visible || !ready,
    )
}

fn gate_water_draws(world: &mut World) {
    let mut report = GateReport::default();
    if world.resource::<WaterPairs>().0.is_empty() {
        if let Ok(mut current) = world.resource::<Feedback>().0.lock() {
            *current = report;
        }
        return;
    }
    // Only real cameras; the original terrain remains the sole shadow caster.
    let views: Vec<_> = world
        .query_filtered::<(Entity, &ExtractedView, &RenderVisibleEntities), With<ExtractedCamera>>()
        .iter(world)
        .map(|(entity, view, visible)| {
            (
                entity,
                view.retained_view_entity,
                world.get::<DepthPrepass>(entity).is_some()
                    || world.get::<NormalPrepass>(entity).is_some()
                    || world.get::<MotionVectorPrepass>(entity).is_some()
                    || world.get::<DeferredPrepass>(entity).is_some(),
                visible
                    .iter::<Mesh3d>()
                    .map(|(_, e)| *e)
                    .collect::<HashSet<_>>(),
            )
        })
        .collect();
    for (view_entity, view, needs_prepass, visible) in views {
        let mut suppressed = HashSet::new();
        for &(baseline, proxy) in &world.resource::<WaterPairs>().0 {
            let b = visible.contains(&baseline);
            let p = visible.contains(&proxy);
            if !b && !p {
                continue;
            }
            let (ready, failed) = if b && p {
                pipeline_ready(world, view, proxy, needs_prepass)
            } else {
                (false, false)
            };
            let (suppress_baseline, suppress_proxy) = pair_suppression(b, p, ready);
            if suppress_baseline {
                suppressed.insert(baseline);
                report.ready += 1;
            }
            if suppress_proxy {
                suppressed.insert(proxy);
                if b {
                    report.fallback += 1;
                }
            }
            report.failed |= failed;
        }
        if let Some(mut entities) = world.get_mut::<RenderVisibleEntities>(view_entity)
            && let Some(meshes) = entities.entities.get_mut(&TypeId::of::<Mesh3d>())
        {
            meshes.retain(|(_, entity)| !suppressed.contains(entity));
        }
    }
    if let Ok(mut current) = world.resource::<Feedback>().0.lock() {
        if report.failed && !current.failed {
            error!("water material pipeline failed; retaining original terrain draws");
        }
        if report.ready > 0 && current.ready == 0 {
            info!(
                "water material ready: {} curved terrain draws",
                report.ready
            );
        }
        *current = report;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_missing_failed_or_orphan_pairs_never_hide_baseline_or_double_draw() {
        for b in [false, true] {
            for p in [false, true] {
                for ready in [false, true] {
                    let (hide_b, hide_p) = pair_suppression(b, p, ready);
                    let draws = usize::from(b && !hide_b) + usize::from(p && !hide_p);
                    assert_eq!(draws, usize::from(b));
                    assert_eq!(hide_b, b && p && ready);
                }
            }
        }
    }
}
