//! Keep existing material draws visible during first-use sky IBL compilation.
//!
//! Bevy 0.18.1 adds ENVIRONMENT_MAP to the view's material pipeline key when an
//! environment is attached. Queueing an uncompiled variant skips that mesh.
//! We let Bevy specialize the exact upper variants, then select its baseline
//! variants for this view until every currently visible variant is available.
//! No descriptors, material layouts or shader definitions are reproduced here.

use std::collections::HashSet;

use bevy::pbr::{
    EntitySpecializationTicks, MeshPipelineKey, PreparedMaterial, RenderMaterialInstances,
    RenderMeshInstances, SpecializedMaterialPipelineCache, SpecializedMaterialViewPipelineCache,
    ViewKeyCache, ViewSpecializationTicks, queue_material_meshes, specialize_material_meshes,
};
use bevy::prelude::*;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::mesh::RenderMesh;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{CachedPipelineState, CachedRenderPipelineId, PipelineCache};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RenderVisibleEntities, RetainedViewEntity};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::PipelineCacheError;

use super::{CameraBaseline, GraphicsQualityCamera, QualityRuntime};

/// Replaced at extraction, so cancelled switches, inactive views and replacement
/// camera generations cannot retain a stale warmup request.
#[derive(Resource, Default)]
struct PipelineRequests(Vec<MainEntity>);

#[derive(Resource, Default)]
struct GateStatus {
    waiting: bool,
    failed: bool,
}

pub(super) fn configure(app: &mut App) {
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .init_resource::<PipelineRequests>()
            .init_resource::<GateStatus>()
            .add_systems(ExtractSchedule, extract_requests)
            .add_systems(
                Render,
                gate_environment_pipelines
                    .in_set(RenderSystems::PrepareMeshes)
                    .after(specialize_material_meshes)
                    .before(queue_material_meshes),
            );
    }
}

fn extract_requests(
    mut requests: ResMut<PipelineRequests>,
    runtime: Extract<Res<QualityRuntime>>,
    cameras: Extract<Query<(Entity, &CameraBaseline), With<GraphicsQualityCamera>>>,
) {
    requests.0.clear();
    if !runtime.environment_attached || !runtime.flight_renderable || runtime.helper.is_none() {
        return;
    }
    requests.0.extend(
        cameras
            .iter()
            // An authored environment already uses the same pipeline bit. Only
            // the app-owned addition to a no-environment baseline needs a gate.
            .filter(|(_, baseline)| baseline.environment.is_none())
            .map(|(entity, _)| MainEntity::from(entity)),
    );
}

/// Empty or incomplete specialization is pending, never vacuously ready. IDs
/// are deduplicated and retained only for this frame's visible flight meshes.
fn required_pipelines(
    entities: impl Iterator<Item = MainEntity>,
    specialized: Option<&SpecializedMaterialViewPipelineCache>,
) -> Option<HashSet<CachedRenderPipelineId>> {
    let specialized = specialized?;
    let ids: Option<HashSet<_>> = entities
        .map(|entity| specialized.get(&entity).map(|(_, id)| *id))
        .collect();
    ids.filter(|ids| !ids.is_empty())
}

fn current_view_pipelines(
    world: &World,
    view: RetainedViewEntity,
    visible: &RenderVisibleEntities,
) -> Option<HashSet<CachedRenderPipelineId>> {
    let caches = world.resource::<SpecializedMaterialPipelineCache>();
    let cache = caches.get(&view)?;
    let view_tick = world.resource::<ViewSpecializationTicks>().get(&view)?;
    let entity_ticks = world.resource::<EntitySpecializationTicks>();
    let mesh_instances = world.resource::<RenderMeshInstances>();
    let material_instances = world.resource::<RenderMaterialInstances>();
    let meshes = world.resource::<RenderAssets<RenderMesh>>();
    let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
    let this_run = world.read_change_tick();
    // Match the native queue's renderable cohort. An entity without a prepared
    // mesh/material cannot draw in either tier; it joins the gate when prepared.
    let entities: Vec<_> = visible
        .iter::<Mesh3d>()
        .filter_map(|(_, entity)| {
            let mesh = mesh_instances.render_mesh_queue_data(*entity)?;
            let material = material_instances.instances.get(entity)?;
            meshes.get(mesh.mesh_asset_id)?;
            materials.get(material.asset_id)?;
            Some(*entity)
        })
        .collect();
    for entity in &entities {
        let entity_tick = entity_ticks.get(entity)?.system_tick;
        let (cached_tick, _) = cache.get(entity)?;
        // Native specialization can leave an old entry while assets are being
        // replaced. Apply the same tick rule before calling that entry current.
        if view_tick.is_newer_than(*cached_tick, this_run)
            || entity_tick.is_newer_than(*cached_tick, this_run)
        {
            return None;
        }
    }
    required_pipelines(entities.into_iter(), Some(cache))
}

fn terminal_error(state: &CachedPipelineState) -> Option<&PipelineCacheError> {
    match state {
        CachedPipelineState::Err(
            PipelineCacheError::ShaderNotLoaded(_)
            | PipelineCacheError::ShaderImportNotYetAvailable,
        ) => None,
        CachedPipelineState::Err(error) => Some(error),
        _ => None,
    }
}

fn gate_environment_pipelines(world: &mut World) {
    if world.resource::<PipelineRequests>().0.is_empty() {
        *world.resource_mut::<GateStatus>() = GateStatus::default();
        return;
    }

    let mut views = world.query::<(&ExtractedView, &RenderVisibleEntities)>();
    let requests = world.resource::<PipelineRequests>();
    let keys = world.resource::<ViewKeyCache>();
    let pipelines = world.resource::<PipelineCache>();
    let mut fallback_views = Vec::new();
    let mut requested_variants = 0;
    let mut checked_views = 0;
    let mut first_failure = None;
    for (view, visible) in views.iter(world) {
        let id = view.retained_view_entity;
        if !requests.0.contains(&id.main_entity)
            || !keys
                .get(&id)
                .is_some_and(|key| key.contains(MeshPipelineKey::ENVIRONMENT_MAP))
        {
            continue;
        }
        checked_views += 1;
        let required = current_view_pipelines(world, id, visible);
        requested_variants += required.as_ref().map_or(0, HashSet::len);
        if let Some(ids) = &required {
            for id in ids {
                if let Some(error) = terminal_error(pipelines.get_render_pipeline_state(*id)) {
                    first_failure.get_or_insert_with(|| (*id, error.to_string()));
                }
            }
        }
        if required.is_none_or(|ids| {
            ids.iter()
                .any(|id| pipelines.get_render_pipeline(*id).is_none())
        }) {
            fallback_views.push(id);
        }
    }

    let waiting = !fallback_views.is_empty();
    let was_waiting = world.resource::<GateStatus>().waiting;
    let was_failed = world.resource::<GateStatus>().failed;
    if waiting {
        // The public view-layout conversion ignores ENVIRONMENT_MAP, so the
        // prepared environment bindings also support the no-environment shader.
        // Change only that bit; MSAA/prepass/filter/atmosphere keys are preserved.
        // Removing this view's cache forces exact current mesh/material/layout
        // specialization, rather than replaying stale per-entity pipeline IDs.
        select_baseline_keys(world, &fallback_views);
        world
            .run_system_cached(specialize_material_meshes)
            .expect("graphics pipeline gate requires Bevy material specialization resources");
        // Leave the baseline view key in place. On the next render frame Bevy
        // derives ENVIRONMENT_MAP from the real view again and updates its tick,
        // retrying the exact upper pipelines. Its global specialized cache
        // deduplicates them. Shader/pipeline errors remain engine errors and
        // never qualify as readiness; there is no timed or frame-count bypass.
    }
    if let Some((id, error)) = &first_failure
        && !was_failed
    {
        error!("graphics sky pipeline {id:?} failed; retaining baseline material draws: {error}");
    }
    if waiting && !was_waiting {
        info!(
            "graphics sky pipelines warming: retaining baseline material draws ({requested_variants} unique visible variants)"
        );
    } else if !waiting && was_waiting && checked_views > 0 {
        info!(
            "graphics sky pipelines ready: using generated sky illumination ({requested_variants} unique visible variants)"
        );
    }
    *world.resource_mut::<GateStatus>() = GateStatus {
        waiting,
        failed: first_failure.is_some(),
    };
}

fn select_baseline_keys(world: &mut World, views: &[RetainedViewEntity]) {
    for view in views {
        if let Some(key) = world.resource_mut::<ViewKeyCache>().get_mut(view) {
            key.remove(MeshPipelineKey::ENVIRONMENT_MAP);
        }
        world
            .resource_mut::<SpecializedMaterialPipelineCache>()
            .remove(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::change_detection::Tick;

    fn view(entity: Entity) -> RetainedViewEntity {
        RetainedViewEntity {
            main_entity: entity.into(),
            auxiliary_entity: Entity::PLACEHOLDER.into(),
            subview_index: 0,
        }
    }

    #[test]
    fn missing_late_visible_materials_block_readiness_and_ids_are_deduplicated() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let late = world.spawn_empty().id();
        let mut visible = Vec::<MainEntity>::new();
        let mut cache = SpecializedMaterialViewPipelineCache::default();
        assert!(required_pipelines(visible.iter().copied(), Some(&cache)).is_none());
        visible.push(first.into());
        assert!(required_pipelines(visible.iter().copied(), None).is_none());
        assert!(required_pipelines(visible.iter().copied(), Some(&cache)).is_none());
        cache.insert(
            first.into(),
            (Tick::new(1), CachedRenderPipelineId::INVALID),
        );
        assert_eq!(
            required_pipelines(visible.iter().copied(), Some(&cache))
                .unwrap()
                .len(),
            1
        );
        visible.push(late.into());
        assert!(required_pipelines(visible.iter().copied(), Some(&cache)).is_none());
        cache.insert(late.into(), (Tick::new(2), CachedRenderPipelineId::INVALID));
        assert_eq!(
            required_pipelines(visible.iter().copied(), Some(&cache))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn baseline_retry_preserves_other_views_and_all_other_pipeline_bits() {
        let mut world = World::new();
        let flight = view(world.spawn_empty().id());
        let map = view(world.spawn_empty().id());
        let baseline = MeshPipelineKey::ATMOSPHERE
            | MeshPipelineKey::from_msaa_samples(4)
            | MeshPipelineKey::SHADOW_FILTER_METHOD_GAUSSIAN;
        let desired = baseline | MeshPipelineKey::ENVIRONMENT_MAP;
        let mut keys = ViewKeyCache::default();
        keys.insert(flight, desired);
        keys.insert(map, desired);
        let mut caches = SpecializedMaterialPipelineCache::default();
        caches.insert(flight, SpecializedMaterialViewPipelineCache::default());
        caches.insert(map, SpecializedMaterialViewPipelineCache::default());
        world.insert_resource(keys);
        world.insert_resource(caches);

        select_baseline_keys(&mut world, &[flight]);

        assert_eq!(world.resource::<ViewKeyCache>()[&flight], baseline);
        assert_eq!(world.resource::<ViewKeyCache>()[&map], desired);
        let caches = world.resource::<SpecializedMaterialPipelineCache>();
        assert!(!caches.contains_key(&flight));
        assert!(caches.contains_key(&map));
    }

    #[test]
    fn retryable_shader_loading_is_distinct_from_a_failed_shader() {
        assert!(terminal_error(&CachedPipelineState::Queued).is_none());
        assert!(
            terminal_error(&CachedPipelineState::Err(
                PipelineCacheError::ShaderNotLoaded(AssetId::<Shader>::invalid()),
            ))
            .is_none()
        );
        assert!(
            terminal_error(&CachedPipelineState::Err(
                PipelineCacheError::ShaderImportNotYetAvailable,
            ))
            .is_none()
        );
        let failed = CachedPipelineState::Err(PipelineCacheError::CreateShaderModule(
            "test module validation failure".to_owned(),
        ));
        assert!(terminal_error(&failed).is_some());
    }
}
