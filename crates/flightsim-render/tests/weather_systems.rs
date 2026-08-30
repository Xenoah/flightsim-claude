//! 雲中視程の ECS 結線を、実際のカメラ位置まで含めて確かめる。

use bevy::prelude::*;
use flightsim_core::{Geodetic, Meters};
use flightsim_render::{
    CameraWorldPosition, CloudDistanceFog, CloudLayer, FlightsimRenderPlugin, RenderOrigin,
};

fn tokyo() -> Geodetic {
    Geodetic::from_degrees(35.6895, 139.6917, 0.0)
}

fn camera_fog_alpha(app: &App, camera: Entity) -> f32 {
    app.world()
        .entity(camera)
        .get::<DistanceFog>()
        .expect("weather should attach distance fog to every 3D camera")
        .color
        .alpha()
}

#[test]
fn fog_follows_the_camera_altitude_not_the_aircraft_resource() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(FlightsimRenderPlugin)
        .insert_resource(RenderOrigin::new(tokyo()))
        // The aircraft stays below the layer throughout this test. If weather accidentally
        // reads this resource, the camera can never enter the cloud.
        .insert_resource(CameraWorldPosition(Geodetic::from_degrees(
            35.6895, 139.6917, 100.0,
        )))
        .insert_resource(
            CloudLayer::try_new(1.0, Meters(1_000.0), Meters(2_000.0), Meters(300.0), 7)
                .expect("test cloud layer is valid"),
        );

    let camera = app
        .world_mut()
        .spawn((Camera3d::default(), Transform::from_xyz(0.0, 1_500.0, 0.0)))
        .id();
    app.update();

    assert!(app.world().entity(camera).contains::<CloudDistanceFog>());
    assert!(
        camera_fog_alpha(&app, camera) > 0.99,
        "a camera in overcast should get full fog"
    );

    app.world_mut()
        .entity_mut(camera)
        .get_mut::<Transform>()
        .expect("camera has a transform")
        .translation
        .y = 500.0;
    app.update();

    assert_eq!(
        camera_fog_alpha(&app, camera).to_bits(),
        0.0_f32.to_bits(),
        "a camera below the cloud should have no cloud fog"
    );
}
