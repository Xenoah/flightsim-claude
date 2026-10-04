//! Aircraft-only scene ownership. Hidden candidates never participate in active
//! camera, model fitting or visibility queries before a new-flight commit.
use super::*;
use bevy::asset::{LoadState, RecursiveDependencyLoadState};
use bevy::ecs::world::CommandQueue;
use bevy::scene::{SceneInstance, SceneSpawner};
use std::time::{Duration, Instant};

#[derive(Component)]
struct StagedExterior;
#[derive(Component)]
struct StagedInterior;
#[derive(Component, Default)]
struct OwnedAircraftAssets {
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
}

pub(super) struct AircraftScene {
    pub root: Entity,
    model: Option<(Entity, Handle<Scene>)>,
    document: Option<Handle<bevy::gltf::Gltf>>,
    fit: ModelFit,
    ready_seen: bool,
    began: Instant,
}

/// Startup and new flights share geometry construction, while their activation
/// markers and readiness rules remain explicit. Only generated assets are owned
/// here; GLB assets follow the asset server's strong-handle lifecycle.
pub(super) fn spawn(
    commands: &mut Commands,
    server: &AssetServer,
    startup: &Startup,
    simulation: &FlightSession,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    staged: bool,
) -> AircraftScene {
    let mut owned = OwnedAircraftAssets::default();
    let mut parts = Vec::new();
    let model = startup.model.as_ref().map(|path| {
        let handle = server.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
        let mut entity = commands.spawn((
            Transform::from_rotation(startup.model_fit.rotation()),
            Visibility::Inherited,
            Name::new("aircraft model"),
        ));
        if staged {
            entity.insert(StagedExterior);
        } else {
            entity.insert((
                SceneRoot(handle.clone()),
                ExteriorModel,
                PendingModelFit(startup.model_fit),
            ));
        }
        let entity = entity.id();
        parts.push(entity);
        (entity, handle)
    });
    if model.is_none() {
        for part in simulation.placeholder_parts() {
            let mesh = meshes.add(part.mesh);
            let material = materials.add(StandardMaterial {
                base_color: part.color,
                perceptual_roughness: 0.5,
                ..default()
            });
            owned.meshes.push(mesh.clone());
            owned.materials.push(material.clone());
            let mut entity = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                part.transform,
                Visibility::Inherited,
                Name::new(part.name),
            ));
            if staged {
                entity.insert(StagedExterior);
            } else {
                entity.insert(ExteriorModel);
            }
            parts.push(entity.id());
        }
    }
    let interior = if startup.aircraft.is_jet() {
        Vec::new()
    } else {
        flightsim_render::cockpit::interior_parts(startup.aircraft.camera_eye())
    };
    for part in interior {
        let mesh = meshes.add(part.mesh);
        let material = materials.add(StandardMaterial {
            base_color: part.color,
            perceptual_roughness: 0.85,
            emissive: if part.emissive {
                LinearRgba::from(part.color) * 0.6
            } else {
                LinearRgba::BLACK
            },
            ..default()
        });
        owned.meshes.push(mesh.clone());
        owned.materials.push(material.clone());
        let mut entity = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            part.transform,
            Visibility::Hidden,
            Name::new(part.name),
        ));
        if staged {
            entity.insert(StagedInterior);
        } else {
            entity.insert(InteriorModel);
        }
        parts.push(entity.id());
    }
    let mut root = commands.spawn((
        Transform::default(),
        if staged {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
        owned,
        Name::new(if staged {
            "pending aircraft"
        } else {
            "aircraft"
        }),
    ));
    if !staged {
        root.insert((
            Aircraft,
            WorldPosition(simulation.state().position),
            WorldOrientation(simulation.state().orientation),
        ));
    }
    root.add_children(&parts);
    AircraftScene {
        root: root.id(),
        model,
        document: if staged {
            startup.model.as_ref().map(|path| server.load(path.clone()))
        } else {
            None
        },
        fit: startup.model_fit,
        ready_seen: false,
        began: Instant::now(),
    }
}

pub(super) fn stage(
    world: &mut World,
    prepared: &world_runtime::PreparedWorldFlight,
) -> Result<AircraftScene, String> {
    distribution::validate_model(
        prepared.startup.model.as_deref(),
        prepared.startup.assets.as_deref(),
        true,
    )?;
    let server = world
        .get_resource::<AssetServer>()
        .ok_or_else(|| "Aircraft asset loader is unavailable".to_owned())?
        .clone();
    let mut meshes = world
        .remove_resource::<Assets<Mesh>>()
        .ok_or_else(|| "Aircraft mesh storage is unavailable".to_owned())?;
    let Some(mut materials) = world.remove_resource::<Assets<StandardMaterial>>() else {
        world.insert_resource(meshes);
        return Err("Aircraft material storage is unavailable".into());
    };
    let mut queue = CommandQueue::default();
    let scene = spawn(
        &mut Commands::new(&mut queue, world),
        &server,
        &prepared.startup,
        &prepared.simulation,
        &mut meshes,
        &mut materials,
        true,
    );
    queue.apply(world);
    world.insert_resource(meshes);
    world.insert_resource(materials);
    Ok(scene)
}

impl AircraftScene {
    /// Wait for the complete scene, then one transform/bounds propagation pass.
    /// A first AABB is not proof that all the model's primitives were spawned.
    pub fn poll(&mut self, world: &mut World) -> Result<bool, String> {
        if self.began.elapsed() > Duration::from_secs(30) {
            return Err("Aircraft model preparation timed out; current flight unchanged".into());
        }
        let Some((model, handle)) = &self.model else {
            return Ok(true);
        };
        let server = world.resource::<AssetServer>();
        // A failed GLB parent can leave a requested Scene0 label pending rather
        // than giving that label its own failure. Hold/check the document too.
        if let Some(document) = &self.document {
            match server.get_load_states(document.id()) {
                Some((LoadState::Failed(error), _, _))
                | Some((_, _, RecursiveDependencyLoadState::Failed(error))) => {
                    return Err(format!("Aircraft model could not load: {error}"));
                }
                _ => {}
            }
            if let Some(gltf) = world.resource::<Assets<bevy::gltf::Gltf>>().get(document)
                && gltf.scenes.is_empty()
            {
                return Err("Aircraft model contains no scene0".into());
            }
        }
        match server.get_load_states(handle.id()) {
            Some((LoadState::Failed(error), _, _))
            | Some((_, _, RecursiveDependencyLoadState::Failed(error))) => {
                return Err(format!("Aircraft model could not load: {error}"));
            }
            _ => {}
        }
        if !server.is_loaded_with_dependencies(handle.id()) {
            return Ok(false);
        }
        if world.get::<SceneRoot>(*model).is_none() {
            let scene = world
                .resource::<Assets<Scene>>()
                .get(handle)
                .ok_or_else(|| "Aircraft scene0 is unavailable".to_owned())?;
            // Entity visibility cannot disable a camera or light. Reject those
            // unsupported scene owners BEFORE instantiation, including a Scene
            // shared with a currently loaded custom model.
            let mut entities = scene
                .world
                .try_query::<EntityRef>()
                .expect("entity-only query needs no component registration");
            let unsupported = entities.iter(&scene.world).any(|entity| {
                entity.contains::<Camera>()
                    || entity.contains::<Camera3d>()
                    || entity.contains::<PointLight>()
                    || entity.contains::<SpotLight>()
                    || entity.contains::<DirectionalLight>()
            });
            if unsupported {
                return Err("Aircraft scene contains an unsupported camera or light".into());
            }
            world.entity_mut(*model).insert(SceneRoot(handle.clone()));
            return Ok(false);
        }
        let spawned = world.get::<SceneInstance>(*model).is_some_and(|instance| {
            world
                .resource::<SceneSpawner>()
                .instance_is_ready(**instance)
        });
        if !spawned {
            return Ok(false);
        }
        if !std::mem::replace(&mut self.ready_seen, true) {
            return Ok(false);
        }
        let global = world
            .get::<GlobalTransform>(*model)
            .ok_or_else(|| "Aircraft model has no transform".to_owned())?
            .affine();
        if !global.is_finite() || global.matrix3.determinant().abs() < 1e-12 {
            return Err("Aircraft model transform is invalid".into());
        }
        let mut bounds = Vec::new();
        for entity in descendants(world, *model) {
            let transform = world
                .get::<GlobalTransform>(entity)
                .ok_or_else(|| "Aircraft scene contains an untransformed entity".to_owned())?;
            if !transform.affine().is_finite() {
                return Err("Aircraft scene contains a nonfinite transform".into());
            }
            if let Some(mesh) = world.get::<Mesh3d>(entity) {
                if world.resource::<Assets<Mesh>>().get(&mesh.0).is_none() {
                    return Err("Aircraft scene mesh is unavailable".into());
                }
                let aabb = world
                    .get::<Aabb>(entity)
                    .ok_or_else(|| "Aircraft scene mesh has no bounds".to_owned())?;
                if !aabb.center.is_finite()
                    || !aabb.half_extents.is_finite()
                    || aabb.half_extents.min_element() < 0.0
                {
                    return Err("Aircraft scene mesh bounds are invalid".into());
                }
                bounds.push((*aabb, transform.affine()));
            }
        }
        let extents = extents_in_model_space(global.inverse(), bounds)
            .ok_or_else(|| "Aircraft scene contains no bounded meshes".to_owned())?;
        let length = (extents * self.fit.forward.to_vec3()).length();
        let scale = self.fit.scale_for(extents);
        if !extents.is_finite()
            || !length.is_finite()
            || length < 1e-6
            || !scale.is_finite()
            || scale <= 0.0
        {
            return Err("Aircraft scene cannot be fitted to its profile".into());
        }
        world
            .get_mut::<Transform>(*model)
            .expect("scene transform checked")
            .scale = Vec3::splat(scale);
        info!("new-flight model fitted: length {length:.2} m -> scale {scale:.4}");
        Ok(true)
    }

    pub fn cancel(self, world: &mut World) {
        despawn(world, self.root);
    }

    /// All readiness checks precede this exclusive, infallible ownership swap.
    pub fn commit(self, world: &mut World, startup: &Startup, simulation: &FlightSession) {
        info!(
            "new-flight aircraft: {} ({}); model {}",
            startup.aircraft.name(),
            startup.aircraft.id(),
            startup.model.as_deref().unwrap_or("explicit placeholder")
        );
        let previous: Vec<_> = world
            .query_filtered::<Entity, With<Aircraft>>()
            .iter(world)
            .collect();
        let mode = world
            .get_resource::<ViewMode>()
            .copied()
            .unwrap_or(startup.view);
        for entity in descendants(world, self.root) {
            if world.get::<StagedExterior>(entity).is_some() {
                world.entity_mut(entity).remove::<StagedExterior>().insert((
                    ExteriorModel,
                    if shows_exterior(mode) || startup.aircraft.is_jet() {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            }
            if world.get::<StagedInterior>(entity).is_some() {
                world.entity_mut(entity).remove::<StagedInterior>().insert((
                    InteriorModel,
                    if shows_exterior(mode) {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    },
                ));
            }
        }
        for old in previous {
            despawn(world, old);
        }
        world.entity_mut(self.root).insert((
            Aircraft,
            Visibility::Inherited,
            Name::new("aircraft"),
            WorldPosition(simulation.state().position),
            WorldOrientation(simulation.state().orientation),
        ));
    }
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut result = Vec::new();
    let mut remaining = vec![root];
    while let Some(parent) = remaining.pop() {
        if let Some(children) = world.get::<Children>(parent) {
            for child in children.iter() {
                result.push(child);
                remaining.push(child);
            }
        }
    }
    result
}

fn despawn(world: &mut World, root: Entity) {
    let owned = world
        .get_entity_mut(root)
        .ok()
        .and_then(|mut entity| entity.take::<OwnedAircraftAssets>())
        .unwrap_or_default();
    if let Ok(entity) = world.get_entity_mut(root) {
        entity.despawn();
    }
    for mesh in owned.meshes {
        world.resource_mut::<Assets<Mesh>>().remove(&mesh);
    }
    for material in owned.materials {
        world
            .resource_mut::<Assets<StandardMaterial>>()
            .remove(&material);
    }
}
