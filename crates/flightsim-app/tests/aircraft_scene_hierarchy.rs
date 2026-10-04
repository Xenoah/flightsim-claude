//! Load the shipped GLBs through Bevy's real loader and scene spawner without a GPU.
//!
//! B0004 is checked by component insertion hooks, before a cloned scene is ready.
//! This test checks the complete final hierarchy rather than treating warning counts
//! or a filtered model-fit query as proof that every mesh was instantiated correctly.

use bevy::app::TaskPoolPlugin;
use bevy::asset::{AssetPlugin, LoadState, RecursiveDependencyLoadState};
use bevy::camera::{primitives::Aabb, visibility::VisibilityPlugin};
use bevy::gltf::{GltfAssetLabel, GltfPlugin};
use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
use bevy::mesh::MeshPlugin;
use bevy::prelude::*;
use bevy::scene::{SceneInstance, ScenePlugin, SceneSpawner};
use bevy::transform::TransformPlugin;
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn headless_asset_app() -> App {
    let mut app = App::new();
    // Match the pinned bevy_gltf loader tests, with real file assets and the
    // production transform/visibility systems. Neither RenderPlugin nor Winit
    // is needed. These two original assets have no textures or animations.
    app.insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
        .add_plugins((
            bevy::log::LogPlugin::default(),
            TaskPoolPlugin::default(),
            AssetPlugin {
                file_path: format!("{}/../../assets", env!("CARGO_MANIFEST_DIR")),
                ..default()
            },
            ScenePlugin,
            MeshPlugin,
            GltfPlugin::default(),
            TransformPlugin,
            VisibilityPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        // MaterialPlugin normally registers this generic component; derived
        // non-generic scene components use reflect_auto_register as in the app.
        .register_type::<MeshMaterial3d<StandardMaterial>>();
    app.finish();
    app.cleanup();
    app
}

fn wait_for_scene(app: &mut App, root: Entity, handle: &Handle<Scene>, asset: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.update();
        let server = app.world().resource::<AssetServer>();
        let states = server.get_load_states(handle.id());
        if let Some((LoadState::Failed(error), _, _)) = &states {
            panic!("{asset}: load failed: {error}");
        }
        if let Some((_, _, RecursiveDependencyLoadState::Failed(error))) = &states {
            panic!("{asset}: dependency load failed: {error}");
        }
        let spawned = app.world().get::<SceneInstance>(root).is_some_and(|id| {
            app.world()
                .resource::<SceneSpawner>()
                .instance_is_ready(**id)
        });
        if server.is_loaded_with_dependencies(handle.id()) && spawned {
            // Complete another propagation pass after readiness, independently
            // of the schedule in which this Bevy version marks the scene ready.
            app.update();
            return;
        }
        assert!(Instant::now() < deadline, "{asset}: timed out: {states:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assert_complete_scene(app: &App, root: Entity, mesh_count: usize, visible: bool) {
    let world = app.world();
    let instance = world.get::<SceneInstance>(root).expect("spawned scene");
    let instantiated: HashSet<_> = world
        .resource::<SceneSpawner>()
        .iter_instance_entities(**instance)
        .collect();
    // Both GLBs currently contain flat mesh nodes, each with one primitive.
    // Bevy adds a scene root and a primitive entity beneath every authored node.
    assert_eq!(instantiated.len(), 1 + 2 * mesh_count);

    let mut descendants = HashSet::new();
    let mut remaining = vec![root];
    while let Some(parent) = remaining.pop() {
        if let Some(children) = world.get::<Children>(parent) {
            for child in children.iter() {
                assert!(
                    descendants.insert(child),
                    "duplicate/cyclic child {child:?}"
                );
                remaining.push(child);
            }
        }
    }
    // Enumerate through both APIs: missing components must not filter an
    // entity out of the assertions, and detached instance entities must fail.
    assert_eq!(descendants, instantiated);

    let meshes = world.resource::<Assets<Mesh>>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut found_meshes = 0;
    for entity in instantiated {
        let local = world.get::<Transform>(entity).expect("local transform");
        let global = world
            .get::<GlobalTransform>(entity)
            .expect("global transform on every scene entity");
        let inherited = world
            .get::<InheritedVisibility>(entity)
            .expect("inherited visibility on every scene entity");
        assert!(world.get::<Visibility>(entity).is_some());
        let parent = world
            .get::<ChildOf>(entity)
            .expect("attached entity")
            .parent();
        let parent_global = world
            .get::<GlobalTransform>(parent)
            .expect("every GlobalTransform parent has GlobalTransform");
        assert!(
            world.get::<InheritedVisibility>(parent).is_some(),
            "every InheritedVisibility parent has InheritedVisibility"
        );
        assert!(world.get::<Children>(parent).unwrap().contains(&entity));
        assert!(local.to_matrix().is_finite());
        assert!(global.to_matrix().is_finite());
        assert!(global.affine().matrix3.determinant().abs() > 1e-8);
        let expected = parent_global.mul_transform(*local);
        assert!(
            global.to_matrix().abs_diff_eq(expected.to_matrix(), 1e-4),
            "unpropagated transform for {entity:?}: {global:?}, expected {expected:?}"
        );
        assert_eq!(inherited.get(), visible, "visibility for {entity:?}");

        if let Some(mesh) = world.get::<Mesh3d>(entity) {
            found_meshes += 1;
            assert!(meshes.get(&mesh.0).expect("loaded mesh").count_vertices() > 0);
            let material = world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .expect("primitive material");
            assert!(materials.get(&material.0).is_some());
            let bounds = world.get::<Aabb>(entity).expect("mesh bounds");
            assert!(bounds.center.is_finite() && bounds.half_extents.is_finite());
            assert!(bounds.half_extents.length_squared() > 0.0);
        }
    }
    assert_eq!(found_meshes, mesh_count, "all authored primitives present");
}

#[test]
fn original_aircraft_glbs_have_complete_spawned_hierarchies() {
    let mut app = headless_asset_app();
    for (asset, mesh_count) in [
        ("aircraft/kestrel_jet_trainer.glb", 53),
        ("aircraft/swift_sport.glb", 31),
    ] {
        let handle: Handle<Scene> = app
            .world()
            .resource::<AssetServer>()
            .load(GltfAssetLabel::Scene(0).from_asset(asset));
        // Non-identity ancestors make missing transform propagation observable.
        let aircraft = app
            .world_mut()
            .spawn((
                Transform::from_xyz(13.0, 27.0, -9.0).with_rotation(Quat::from_rotation_y(0.37)),
                Visibility::default(),
            ))
            .id();
        let root = app
            .world_mut()
            .spawn((
                SceneRoot(handle.clone()),
                Transform::from_rotation(Quat::from_rotation_x(0.2)).with_scale(Vec3::splat(1.25)),
                ChildOf(aircraft),
            ))
            .id();
        wait_for_scene(&mut app, root, &handle, asset);
        assert_complete_scene(&app, root, mesh_count, true);

        // Exercise the same hierarchy after it is already instantiated.
        app.world_mut()
            .get_mut::<Transform>(aircraft)
            .unwrap()
            .translation
            .x += 7.0;
        *app.world_mut().get_mut::<Visibility>(aircraft).unwrap() = Visibility::Hidden;
        app.update();
        assert_complete_scene(&app, root, mesh_count, false);
        *app.world_mut().get_mut::<Visibility>(aircraft).unwrap() = Visibility::Inherited;
        app.update();
        assert_complete_scene(&app, root, mesh_count, true);
        println!(
            "{asset}: {} scene entities, {mesh_count} meshes; hierarchy, transforms, materials, bounds and hide/show propagation passed",
            1 + 2 * mesh_count
        );
        app.world_mut().entity_mut(aircraft).despawn();
        app.update();
    }
}
