//! Exercise the real render plugin, including its mode-dependent registration.

use bevy::{core_pipeline::tonemapping::Tonemapping, prelude::*};
use flightsim_core::Geodetic;
use flightsim_render::{FlightsimRenderPlugin, RenderOrigin};

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, FlightsimRenderPlugin))
        .insert_resource(RenderOrigin::new(Geodetic::from_degrees(35.0, 139.0, 0.0)))
        // Match Core3dPlugin's required default without starting a GPU.
        .register_required_components::<Camera3d, Tonemapping>();
    app
}

fn expected_lut_method(method: Tonemapping) -> Tonemapping {
    if cfg!(feature = "analytic-tonemapping") {
        Tonemapping::Reinhard
    } else {
        method
    }
}

#[test]
fn tonemapping_mode_covers_startup_scene_and_postupdate_camera_producers() {
    let mut app = harness();
    fn spawn(mut commands: Commands) {
        commands.spawn(Camera3d::default());
    }
    app.add_systems(Startup, spawn)
        .add_systems(
            SpawnScene,
            spawn.run_if(bevy::ecs::schedule::common_conditions::run_once),
        )
        .add_systems(
            PostUpdate,
            spawn.run_if(bevy::ecs::schedule::common_conditions::run_once),
        );
    assert_eq!(Tonemapping::default(), Tonemapping::TonyMcMapface);
    app.update();
    let methods: Vec<_> = app
        .world_mut()
        .query_filtered::<&Tonemapping, With<Camera3d>>()
        .iter(app.world())
        .copied()
        .collect();
    assert_eq!(methods.len(), 3);
    assert!(
        methods
            .iter()
            .all(|&method| method == expected_lut_method(Tonemapping::TonyMcMapface))
    );
}

#[derive(Resource)]
struct LateMethod(Tonemapping);

#[test]
fn tonemapping_mode_covers_late_method_changes_and_preserves_all_2d_cameras() {
    let mut app = harness();
    let flight = app.world_mut().spawn(Camera3d::default()).id();
    let methods = [
        Tonemapping::AgX,
        Tonemapping::TonyMcMapface,
        Tonemapping::BlenderFilmic,
    ];
    let map_methods = [Tonemapping::None, Tonemapping::TonyMcMapface];
    let maps: Vec<_> = map_methods
        .iter()
        .map(|&method| app.world_mut().spawn((Camera2d, method)).id())
        .collect();
    app.insert_resource(LateMethod(methods[0])).add_systems(
        PostUpdate,
        |method: Res<LateMethod>, mut cameras: Query<&mut Tonemapping, With<Camera3d>>| {
            for mut tone in &mut cameras {
                *tone = method.0;
            }
        },
    );
    for method in methods {
        app.world_mut().resource_mut::<LateMethod>().0 = method;
        app.update();
        assert_eq!(
            app.world().get::<Tonemapping>(flight),
            Some(&expected_lut_method(method))
        );
        for (&map, method) in maps.iter().zip(&map_methods) {
            assert_eq!(app.world().get::<Tonemapping>(map), Some(method));
        }
    }
}

#[test]
fn tonemapping_mode_preserves_explicit_linear_and_analytical_methods() {
    let mut app = harness();
    let methods = [
        Tonemapping::None,
        Tonemapping::Reinhard,
        Tonemapping::ReinhardLuminance,
        Tonemapping::AcesFitted,
        Tonemapping::SomewhatBoringDisplayTransform,
    ];
    let entities: Vec<_> = methods
        .iter()
        .map(|&method| app.world_mut().spawn((Camera3d::default(), method)).id())
        .collect();
    for _ in 0..2 {
        app.update();
        for (&entity, method) in entities.iter().zip(&methods) {
            assert_eq!(app.world().get::<Tonemapping>(entity), Some(method));
        }
    }
}
