//! Airborne shadow reach must come from the real sun bundle, not Bevy's
//! pedestrian-scale default. These CPU tests run Bevy's actual cascade builder;
//! they do not claim raster, bias/contact, or frame-time acceptance.

use bevy::light::{
    CascadeShadowConfig, Cascades, DirectionalLightShadowMap,
    cascade::{Cascade, build_directional_light_cascades},
};
use bevy::prelude::*;
use flightsim_core::Degrees;
use flightsim_render::{SunDirection, SunLighting, sun_light_bundle};

fn sun(elevation: f64) -> SunDirection {
    SunDirection {
        azimuth: Degrees(135.0).to_radians(),
        elevation: Degrees(elevation).to_radians(),
    }
}

fn cascade_harness(
    camera: Transform,
    elevation: f64,
    use_engine_default: bool,
) -> (CascadeShadowConfig, Vec<Cascade>, DirectionalLight) {
    cascade_harness_with_projection(
        camera,
        elevation,
        use_engine_default,
        std::f32::consts::FRAC_PI_3,
        16.0 / 9.0,
    )
}

fn cascade_harness_with_projection(
    camera: Transform,
    elevation: f64,
    use_engine_default: bool,
    fov: f32,
    aspect_ratio: f32,
) -> (CascadeShadowConfig, Vec<Cascade>, DirectionalLight) {
    let mut app = App::new();
    app.init_resource::<DirectionalLightShadowMap>()
        .add_systems(Update, build_directional_light_cascades);
    let view = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera::default(),
            Projection::Perspective(PerspectiveProjection {
                fov,
                aspect_ratio,
                near: 0.05,
                far: 400_000.0,
                ..default()
            }),
            GlobalTransform::from(camera),
        ))
        .id();
    let light = app
        .world_mut()
        .spawn(sun_light_bundle(&SunLighting::default(), sun(elevation)))
        .id();
    let transform = *app.world().get::<Transform>(light).unwrap();
    app.world_mut()
        .entity_mut(light)
        .insert(GlobalTransform::from(transform));
    if use_engine_default {
        app.world_mut()
            .entity_mut(light)
            .insert(CascadeShadowConfig::default());
    }
    app.update();
    let entity = app.world().entity(light);
    (
        entity.get::<CascadeShadowConfig>().unwrap().clone(),
        entity.get::<Cascades>().unwrap().cascades[&view].clone(),
        *entity.get::<DirectionalLight>().unwrap(),
    )
}

#[test]
fn sun_bundle_has_finite_local_flight_reach_without_more_shadow_maps() {
    let (config, cascades, light) = cascade_harness(Transform::IDENTITY, 45.0, false);
    assert_eq!(config.bounds.len(), 4);
    assert_eq!(cascades.len(), 4);
    assert!(config.minimum_distance > 0.0);
    assert!((0.0..1.0).contains(&config.overlap_proportion));
    assert!(config.bounds.iter().all(|bound| bound.is_finite()));
    assert!(config.bounds.windows(2).all(|pair| pair[0] < pair[1]));
    // Regression: the old bundle inherited Bevy's 150 m camera-depth cutoff.
    // Flight shadows have an explicit 2 km local limit, not the 400 km far plane.
    assert!((config.bounds[3] - 2_000.0).abs() < 0.01);
    assert!(light.shadows_enabled);
    assert_eq!(
        light.shadow_depth_bias.to_bits(),
        DirectionalLight::DEFAULT_SHADOW_DEPTH_BIAS.to_bits()
    );
    assert_eq!(
        light.shadow_normal_bias.to_bits(),
        DirectionalLight::DEFAULT_SHADOW_NORMAL_BIAS.to_bits()
    );
}

#[test]
fn close_ground_cascade_keeps_engine_resolution_and_stabilized_extent() {
    let camera = Transform::from_xyz(0.0, 1.8, 0.0);
    let (before, before_cascades, _) = cascade_harness(camera, 45.0, true);
    let (after, after_cascades, _) = cascade_harness(camera, 45.0, false);
    assert_eq!(before.bounds[0].to_bits(), after.bounds[0].to_bits());
    assert_eq!(
        before.minimum_distance.to_bits(),
        after.minimum_distance.to_bits()
    );
    assert_eq!(
        before.overlap_proportion.to_bits(),
        after.overlap_proportion.to_bits()
    );
    assert_eq!(
        before_cascades[0].texel_size.to_bits(),
        after_cascades[0].texel_size.to_bits()
    );
    assert_eq!(
        before_cascades[0].clip_from_world,
        after_cascades[0].clip_from_world
    );
    // Native default: four 2048-square maps. Enlarging reach must not silently
    // increase allocation or remove the first 10 m near-aircraft cascade.
    assert_eq!(DirectionalLightShadowMap::default().size, 2048);
    assert!(after_cascades[0].texel_size < 0.012);
    assert!(after_cascades[3].texel_size < 2.31);
}

#[test]
fn airborne_ground_receivers_are_inside_real_cascades_past_legacy_cutoff() {
    // Flat-ground geometric controls, not DEM/contact truth. A camera looking
    // at the ground 300 m ahead from 150 m AGL has 335.4 m forward depth.
    for (altitude, ahead) in [(150.0, 300.0), (350.0, 500.0), (1_000.0, 1_000.0)] {
        let receiver = Vec3::new(0.0, 0.0, -ahead);
        let camera = Transform::from_xyz(0.0, altitude, 0.0).looking_at(receiver, Vec3::Y);
        let view_depth = -camera.to_matrix().inverse().transform_point3(receiver).z;
        assert!(view_depth > 150.0 && view_depth < 2_000.0);
        for elevation in [5.0, 30.0, 89.0] {
            let (config, cascades, _) = cascade_harness(camera, elevation, false);
            // Bevy shadows.wgsl selects the first far_bound > -view_z. Beyond
            // the last bound it returns 1.0 (fully lit), regardless of bias.
            let index = config
                .bounds
                .iter()
                .position(|&bound| view_depth < bound)
                .expect("airborne ground must not be outside every sun cascade");
            let cascade = &cascades[index];
            // A 15 m crown above the target is also inside the shadow volume;
            // extending receiver lookup without covering the caster is insufficient.
            for point in [receiver, receiver + Vec3::Y * 15.0] {
                let clip = cascade.clip_from_world.project_point3(point);
                assert!(clip.is_finite());
                assert!((-1.0..=1.0).contains(&clip.x));
                assert!((-1.0..=1.0).contains(&clip.y));
                assert!((0.0..=1.0).contains(&clip.z));
            }
            assert!(cascades.iter().all(|cascade| {
                cascade.clip_from_world.is_finite()
                    && cascade.texel_size.is_finite()
                    && cascade.texel_size > 0.0
            }));
        }
    }
}

#[test]
fn shadow_range_is_view_depth_not_altitude_or_distance_to_sun() {
    let (config, _, _) = cascade_harness(Transform::IDENTITY, 45.0, false);
    let last = *config.bounds.last().unwrap();
    assert!(config.bounds.iter().any(|&bound| 1_999.0 < bound));
    assert!(!config.bounds.iter().any(|&bound| last < bound));
    assert!(!config.bounds.iter().any(|&bound| 2_001.0 < bound));
    // At 60 degrees / 16:9 this is inside the view but over 2 km radially.
    let off_axis_receiver = Vec3::new(1_490.0, 0.0, -1_500.0);
    assert!(off_axis_receiver.length() > 2_000.0);
    assert!(
        config
            .bounds
            .iter()
            .any(|&bound| -off_axis_receiver.z < bound)
    );
}

#[test]
fn projection_and_translated_origin_keep_policy_and_finite_stable_cascades() {
    let receiver = Vec3::new(31.5, 0.0, -208.0);
    let camera = Transform::from_xyz(31.5, 150.0, 92.0).looking_at(receiver, Vec3::Y);
    let offset = Vec3::new(4_096.0, -512.0, 8_192.0);
    let rebased_camera = Transform {
        translation: camera.translation + offset,
        ..camera
    };
    let (reference, _, _) = cascade_harness(camera, 45.0, false);
    for fov in [40.0_f32, 60.0, 100.0] {
        for aspect in [1.0, 16.0 / 9.0, 21.0 / 9.0] {
            let (before_config, before, _) =
                cascade_harness_with_projection(camera, 45.0, false, fov.to_radians(), aspect);
            let (after_config, after, _) = cascade_harness_with_projection(
                rebased_camera,
                45.0,
                false,
                fov.to_radians(),
                aspect,
            );
            assert_eq!(before_config.bounds, reference.bounds);
            assert_eq!(after_config.bounds, reference.bounds);
            for (index, (before, after)) in before.iter().zip(&after).enumerate() {
                assert!(before.clip_from_world.is_finite());
                assert!(after.clip_from_world.is_finite());
                assert_eq!(before.texel_size.to_bits(), after.texel_size.to_bits());
                let near = if index == 0 {
                    before_config.minimum_distance
                } else {
                    before_config.bounds[index - 1] * (1.0 - before_config.overlap_proportion)
                };
                let depth = (near + before_config.bounds[index]) * 0.5;
                let sample = camera
                    .to_matrix()
                    .transform_point3(Vec3::new(0.0, 0.0, -depth));
                let old_clip = before.clip_from_world.project_point3(sample);
                let new_clip = after.clip_from_world.project_point3(sample + offset);
                // Bevy's independently snapped light-space centre can move at
                // most one texel after the common origin translation. Allow
                // f32 matrix rounding without claiming bit-identical shadows.
                let delta = (old_clip - new_clip).abs();
                assert!(delta.x < 2.0 / 2_048.0 + 0.0001, "{delta:?}");
                assert!(delta.y < 2.0 / 2_048.0 + 0.0001, "{delta:?}");
                // Z is not texel-snapped. Express the roundoff allowance in
                // light-space metres rather than reusing an NDC pixel bound.
                let depth_error_metres = delta.z / before.clip_from_cascade.z_axis.z.abs();
                assert!(depth_error_metres < 0.01, "{depth_error_metres}");
            }
        }
    }
}
