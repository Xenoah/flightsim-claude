//! Reversible visual quality for the existing scene, independent of simulation.
//!
//! LIGHT is the authored legacy renderer. Upper tiers change sampling and sky
//! illumination, never material assets, terrain selection, geometry or physics.

#[path = "graphics_quality_metrics.rs"]
mod metrics;
pub use metrics::GraphicsQualityDiagnostics;
#[path = "graphics_quality_pipelines.rs"]
mod pipelines;

use bevy::light::{AtmosphereEnvironmentMapLight, DirectionalLightShadowMap, EnvironmentMapLight};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::render_resource::{DownlevelFlags, TextureFormat, TextureUsages};
use bevy::render::renderer::{RenderAdapter, RenderDevice};

/// Runtime visual preset. The default deliberately preserves the old renderer.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GraphicsQuality {
    #[default]
    Light,
    High,
    Ultra,
}

impl GraphicsQuality {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Light => Self::High,
            Self::High => Self::Ultra,
            Self::Ultra => Self::Light,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "LIGHT",
            Self::High => "HIGH",
            Self::Ultra => "ULTRA",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "high" => Some(Self::High),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }

    fn atmosphere(self, baseline: &AtmosphereSettings) -> AtmosphereSettings {
        let mut settings = baseline.clone();
        match self {
            Self::Light => {}
            Self::High => {
                settings.sky_view_lut_samples = 24;
                settings.aerial_view_lut_samples = 16;
            }
            Self::Ultra => {
                settings.sky_view_lut_samples = 32;
                settings.aerial_view_lut_samples = 24;
            }
        }
        settings
    }

    const fn environment_size(self) -> Option<u32> {
        match self {
            Self::Light => None,
            Self::High => Some(128),
            Self::Ultra => Some(256),
        }
    }
}

/// Explicit opt-in on the flight camera. The world-map camera is excluded.
#[derive(Component, Debug)]
pub struct GraphicsQualityCamera;

#[derive(Component, Clone)]
struct CameraBaseline {
    atmosphere: AtmosphereSettings,
    environment: Option<EnvironmentMapLight>,
}

/// Owns only generated lighting images, never visible geometry or physical state.
#[derive(Component)]
struct QualityEnvironment;

/// Capability gate matches Bevy 0.18.1's atmosphere and cubemap generator.
#[derive(Resource, Default)]
struct QualityCapabilities {
    environment_maps: bool,
    shadow_map_limit: u32,
}

#[derive(Resource, Default)]
struct QualityRuntime {
    applied: Option<GraphicsQuality>,
    helper: Option<Entity>,
    flight_renderable: bool,
    shadow_map_size: Option<usize>,
    environment_attached: bool,
}

#[derive(Debug)]
pub(crate) struct GraphicsQualityPlugin;

impl Plugin for GraphicsQualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsQuality>()
            .init_resource::<QualityCapabilities>()
            .init_resource::<QualityRuntime>()
            // Bevy's two generator setup systems queue commands in Update.
            // Its schedule boundary flushes them before any helper teardown.
            .add_systems(
                PostUpdate,
                apply_quality.after(bevy::camera::CameraUpdateSystems),
            );
        metrics::configure(app);
        pipelines::configure(app);
    }

    fn finish(&self, app: &mut App) {
        let capabilities =
            app.get_sub_app(RenderApp)
                .map_or_else(QualityCapabilities::default, |render_app| {
                    let device = render_app.world().resource::<RenderDevice>();
                    let adapter = render_app.world().resource::<RenderAdapter>();
                    let limits = device.limits();
                    QualityCapabilities {
                        environment_maps: limits.max_storage_textures_per_shader_stage >= 6
                            && limits.max_compute_workgroup_storage_size != 0
                            && limits.max_compute_workgroup_size_x != 0
                            && adapter
                                .get_downlevel_capabilities()
                                .flags
                                .contains(DownlevelFlags::COMPUTE_SHADERS)
                            && adapter
                                .get_texture_format_features(TextureFormat::Rgba16Float)
                                .allowed_usages
                                .contains(TextureUsages::STORAGE_BINDING),
                        shadow_map_limit: limits.max_texture_dimension_2d,
                    }
                });
        app.insert_resource(capabilities);
    }
}

fn camera_is_renderable(camera: &Camera) -> bool {
    // Match the public target/viewport requirements of Bevy camera extraction,
    // and also reject empty viewports. is_active alone is insufficient before
    // a target exists or when its window is minimized to zero physical pixels.
    camera.is_active
        && camera.physical_viewport_rect().is_some()
        && camera
            .physical_target_size()
            .is_some_and(|size| size.min_element() > 0)
        && camera
            .physical_viewport_size()
            .is_some_and(|size| size.min_element() > 0)
}

/// Exclusive so a switch removes old ownership and applies the new state in one
/// update. No render frame can observe two generators or partially rolled-back
/// camera settings. Bevy frees the helper's private render components on despawn.
fn apply_quality(world: &mut World) {
    let quality = *world.resource::<GraphicsQuality>();
    let mut runtime = world
        .remove_resource::<QualityRuntime>()
        .unwrap_or_default();
    let flight_renderable = world
        .query_filtered::<&Camera, With<GraphicsQualityCamera>>()
        .iter(world)
        .any(camera_is_renderable);
    let changed =
        runtime.applied != Some(quality) || runtime.flight_renderable != flight_renderable;
    if changed {
        // Release camera owners first, then all private generator components.
        let cameras: Vec<_> = world
            .query_filtered::<(Entity, &CameraBaseline), With<GraphicsQualityCamera>>()
            .iter(world)
            .map(|(entity, baseline)| (entity, baseline.environment.clone()))
            .collect();
        for (entity, environment) in cameras {
            let mut camera = world.entity_mut(entity);
            if let Some(environment) = environment {
                camera.insert(environment);
            } else {
                camera.remove::<EnvironmentMapLight>();
            }
        }
        if let Some(helper) = runtime.helper.take() {
            world.despawn(helper);
        }
        runtime.environment_attached = false;
        if world.contains_resource::<DirectionalLightShadowMap>() {
            let original = *runtime
                .shadow_map_size
                .get_or_insert_with(|| world.resource::<DirectionalLightShadowMap>().size);
            let limit = world.resource::<QualityCapabilities>().shadow_map_limit;
            world.resource_mut::<DirectionalLightShadowMap>().size =
                if quality == GraphicsQuality::Ultra && limit >= 4096 {
                    4096
                } else {
                    original
                };
            if quality == GraphicsQuality::Ultra && limit < 4096 {
                warn!("graphics ULTRA: 4096 shadow map unavailable; retaining {original}");
            }
        }
        if flight_renderable && let Some(size) = quality.environment_size() {
            if world.resource::<QualityCapabilities>().environment_maps {
                runtime.helper = Some(
                    world
                        .spawn((
                            AtmosphereEnvironmentMapLight {
                                size: UVec2::splat(size),
                                intensity: 0.35,
                                ..default()
                            },
                            QualityEnvironment,
                            Name::new("quality sky illumination"),
                        ))
                        .id(),
                );
            } else {
                warn!(
                    "graphics {}: atmosphere IBL unavailable on this adapter; retaining legacy ambient lighting",
                    quality.name()
                );
            }
        }
        info!(
            "graphics quality: {} (F4 cycle, Shift+F4 LIGHT)",
            quality.name()
        );
        runtime.applied = Some(quality);
        runtime.flight_renderable = flight_renderable;
    }

    let cameras: Vec<_> = world
        .query_filtered::<(
            Entity,
            &AtmosphereSettings,
            Option<&EnvironmentMapLight>,
            Option<&CameraBaseline>,
        ), With<GraphicsQualityCamera>>()
        .iter(world)
        .filter(|(_, _, _, baseline)| changed || baseline.is_none())
        .map(|(entity, atmosphere, environment, baseline)| {
            (
                entity,
                baseline.cloned().unwrap_or_else(|| CameraBaseline {
                    atmosphere: atmosphere.clone(),
                    environment: environment.cloned(),
                }),
            )
        })
        .collect();
    for (entity, baseline) in cameras {
        let mut camera = world.entity_mut(entity);
        camera.insert(quality.atmosphere(&baseline.atmosphere));
        if let Some(environment) = baseline.environment.clone() {
            camera.insert(environment);
        } else {
            camera.remove::<EnvironmentMapLight>();
        }
        camera.insert(baseline);
        runtime.environment_attached = false;
    }

    // The generator is intentionally on a disposable helper. Only its final,
    // public lighting component reaches the camera; its private cache never does.
    if !runtime.environment_attached
        && let Some(environment) = runtime
            .helper
            .and_then(|helper| world.get::<EnvironmentMapLight>(helper))
            .cloned()
    {
        let cameras: Vec<_> = world
            .query_filtered::<Entity, With<GraphicsQualityCamera>>()
            .iter(world)
            .collect();
        for camera in cameras {
            world.entity_mut(camera).insert(environment.clone());
        }
        // Keep authored ambient light. The render-world pipeline gate retains
        // baseline material draws until the exact upper variants are ready.
        runtime.environment_attached = true;
    }
    world.insert_resource(runtime);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::light::GeneratedEnvironmentMapLight;

    fn renderable_camera() -> Camera {
        Camera {
            computed: bevy::camera::ComputedCameraValues {
                target_info: Some(bevy::camera::RenderTargetInfo {
                    physical_size: UVec2::new(1280, 720),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        }
    }

    fn app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .insert_resource(DirectionalLightShadowMap::default())
            .add_plugins(GraphicsQualityPlugin);
        app.finish();
        app.cleanup();
        app.insert_resource(QualityCapabilities {
            environment_maps: true,
            shadow_map_limit: 8192,
        });
        let flight = app
            .world_mut()
            .spawn((
                renderable_camera(),
                AtmosphereSettings::default(),
                GraphicsQualityCamera,
                Transform::from_xyz(41.0, 23.0, -19.0),
            ))
            .id();
        let map = app
            .world_mut()
            .spawn((Camera::default(), AtmosphereSettings::default(), Msaa::Off))
            .id();
        app.update();
        (app, flight, map)
    }

    /// Emulate Bevy's three generated image owners; actual GPU rendering is a
    /// separate acceptance gate. This tests our ownership, never shader output.
    fn finish_generation(app: &mut App) {
        let helper = app.world().resource::<QualityRuntime>().helper.unwrap();
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let source = images.add(Image::default());
        let diffuse = images.add(Image::default());
        let specular = images.add(Image::default());
        app.world_mut().entity_mut(helper).insert((
            GeneratedEnvironmentMapLight {
                environment_map: source,
                ..default()
            },
            EnvironmentMapLight {
                diffuse_map: diffuse,
                specular_map: specular,
                intensity: 0.35,
                ..default()
            },
        ));
        app.update();
    }

    #[test]
    fn default_and_upper_sampling_preserve_legacy_texture_dimensions_and_range() {
        assert_eq!(GraphicsQuality::default(), GraphicsQuality::Light);
        let baseline = AtmosphereSettings::default();
        assert_eq!(baseline.sky_view_lut_samples, 16);
        assert_eq!(baseline.aerial_view_lut_samples, 10);
        for quality in [
            GraphicsQuality::Light,
            GraphicsQuality::High,
            GraphicsQuality::Ultra,
        ] {
            let settings = quality.atmosphere(&baseline);
            assert_eq!(
                settings.transmittance_lut_size,
                baseline.transmittance_lut_size
            );
            assert_eq!(
                settings.multiscattering_lut_size,
                baseline.multiscattering_lut_size
            );
            assert_eq!(settings.sky_view_lut_size, baseline.sky_view_lut_size);
            assert_eq!(settings.aerial_view_lut_size, baseline.aerial_view_lut_size);
            assert_eq!(
                settings.aerial_view_lut_max_distance.to_bits(),
                baseline.aerial_view_lut_max_distance.to_bits()
            );
            assert_eq!(
                settings.transmittance_lut_samples,
                baseline.transmittance_lut_samples
            );
            assert_eq!(
                settings.multiscattering_lut_samples,
                baseline.multiscattering_lut_samples
            );
            assert_eq!(
                settings.rendering_method as u32,
                baseline.rendering_method as u32
            );
        }
        assert_eq!(
            GraphicsQuality::Light.next().next().next(),
            GraphicsQuality::Light
        );
        assert!(GraphicsQuality::parse("HIGH").is_none());
    }

    #[test]
    fn twenty_cycles_restore_camera_and_release_owned_images_without_scene_growth() {
        let (mut app, flight, map) = app();
        for _ in 0..6 {
            app.update();
        }
        let base_entities = app.world().entities().count_spawned();
        let base_images = app.world().resource::<Assets<Image>>().len();
        let pose = *app.world().get::<Transform>(flight).unwrap();
        for _ in 0..20 {
            for quality in [GraphicsQuality::High, GraphicsQuality::Ultra] {
                app.insert_resource(quality);
                app.update();
                assert_eq!(app.world().entities().count_spawned(), base_entities + 1);
                let helper = app.world().resource::<QualityRuntime>().helper.unwrap();
                assert_eq!(
                    app.world()
                        .get::<AtmosphereEnvironmentMapLight>(helper)
                        .unwrap()
                        .size
                        .x,
                    quality.environment_size().unwrap()
                );
                finish_generation(&mut app);
                assert!(app.world().get::<EnvironmentMapLight>(flight).is_some());
                assert!(app.world().get::<EnvironmentMapLight>(map).is_none());
                assert_eq!(
                    app.world()
                        .get::<AtmosphereSettings>(map)
                        .unwrap()
                        .sky_view_lut_samples,
                    16
                );
                assert_eq!(*app.world().get::<Msaa>(map).unwrap(), Msaa::Off);
                assert_eq!(*app.world().get::<Transform>(flight).unwrap(), pose);
            }
            app.insert_resource(GraphicsQuality::Light);
            app.update();
            assert_eq!(app.world().entities().count_spawned(), base_entities);
            assert!(app.world().get::<EnvironmentMapLight>(flight).is_none());
            assert_eq!(
                app.world()
                    .get::<AtmosphereSettings>(flight)
                    .unwrap()
                    .sky_view_lut_samples,
                16
            );
            assert_eq!(
                app.world()
                    .get::<AtmosphereSettings>(flight)
                    .unwrap()
                    .aerial_view_lut_samples,
                10
            );
            assert_eq!(
                app.world().resource::<DirectionalLightShadowMap>().size,
                2048
            );
            // Asset handle tracking is deferred. No renderer is present here;
            // real extraction/cache settling is checked in the runtime probe.
            for _ in 0..4 {
                app.update();
            }
            assert_eq!(app.world().resource::<Assets<Image>>().len(), base_images);
        }
    }

    #[test]
    fn inactive_flight_camera_suspends_generation_and_resume_recreates_it() {
        let (mut app, flight, _) = app();
        app.world_mut().get_mut::<Camera>(flight).unwrap().is_active = false;
        app.insert_resource(GraphicsQuality::High);
        app.update();
        assert!(app.world().resource::<QualityRuntime>().helper.is_none());
        for _ in 0..3 {
            app.world_mut().get_mut::<Camera>(flight).unwrap().is_active = true;
            app.update();
            let helper = app.world().resource::<QualityRuntime>().helper.unwrap();
            finish_generation(&mut app);
            app.world_mut().get_mut::<Camera>(flight).unwrap().is_active = false;
            app.update();
            assert!(app.world().get_entity(helper).is_err());
            assert!(app.world().get::<EnvironmentMapLight>(flight).is_none());
            assert_eq!(
                *app.world().resource::<GraphicsQuality>(),
                GraphicsQuality::High
            );
        }
    }

    #[test]
    fn missing_or_empty_target_suspends_and_valid_target_resumes_the_selected_tier() {
        let (mut app, flight, _) = app();
        app.insert_resource(GraphicsQuality::High);
        for physical_size in [None, Some(UVec2::new(0, 720)), Some(UVec2::new(1280, 0))] {
            app.world_mut()
                .get_mut::<Camera>(flight)
                .unwrap()
                .computed
                .target_info = physical_size.map(|physical_size| bevy::camera::RenderTargetInfo {
                physical_size,
                scale_factor: 1.0,
            });
            app.update();
            assert!(app.world().resource::<QualityRuntime>().helper.is_none());
            assert!(app.world().get::<EnvironmentMapLight>(flight).is_none());
        }
        *app.world_mut().get_mut::<Camera>(flight).unwrap() = renderable_camera();
        for physical_size in [UVec2::new(0, 720), UVec2::new(1280, 0)] {
            app.world_mut().get_mut::<Camera>(flight).unwrap().viewport =
                Some(bevy::camera::Viewport {
                    physical_size,
                    ..default()
                });
            app.update();
            assert!(app.world().resource::<QualityRuntime>().helper.is_none());
        }
        app.world_mut().get_mut::<Camera>(flight).unwrap().viewport = None;
        app.update();
        let first = app.world().resource::<QualityRuntime>().helper.unwrap();
        finish_generation(&mut app);
        app.world_mut()
            .get_mut::<Camera>(flight)
            .unwrap()
            .computed
            .target_info = None;
        app.update();
        assert!(app.world().get_entity(first).is_err());
        assert!(app.world().get::<EnvironmentMapLight>(flight).is_none());
        *app.world_mut().get_mut::<Camera>(flight).unwrap() = renderable_camera();
        app.update();
        assert_ne!(
            app.world().resource::<QualityRuntime>().helper.unwrap(),
            first
        );
        finish_generation(&mut app);
        assert!(app.world().get::<EnvironmentMapLight>(flight).is_some());
        assert_eq!(
            *app.world().resource::<GraphicsQuality>(),
            GraphicsQuality::High
        );
    }

    #[test]
    fn authored_environment_and_custom_atmosphere_are_restored_exactly() {
        let (mut app, flight, _) = app();
        app.world_mut().despawn(flight);
        let diffuse = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let specular = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let environment = EnvironmentMapLight {
            diffuse_map: diffuse.clone(),
            specular_map: specular.clone(),
            intensity: 2.5,
            ..default()
        };
        let replacement = app
            .world_mut()
            .spawn((
                renderable_camera(),
                AtmosphereSettings {
                    sky_view_lut_samples: 19,
                    aerial_view_lut_samples: 13,
                    ..default()
                },
                environment,
                GraphicsQualityCamera,
            ))
            .id();
        app.update();
        app.insert_resource(GraphicsQuality::High);
        app.update();
        finish_generation(&mut app);
        assert_ne!(
            app.world()
                .get::<EnvironmentMapLight>(replacement)
                .unwrap()
                .diffuse_map
                .id(),
            diffuse.id()
        );
        app.insert_resource(GraphicsQuality::Light);
        app.update();
        let restored = app.world().get::<EnvironmentMapLight>(replacement).unwrap();
        assert_eq!(restored.diffuse_map.id(), diffuse.id());
        assert_eq!(restored.specular_map.id(), specular.id());
        assert_eq!(restored.intensity.to_bits(), 2.5_f32.to_bits());
        let atmosphere = app.world().get::<AtmosphereSettings>(replacement).unwrap();
        assert_eq!(atmosphere.sky_view_lut_samples, 19);
        assert_eq!(atmosphere.aerial_view_lut_samples, 13);
    }

    #[test]
    fn unsupported_generation_keeps_legacy_environment_and_shadow_fallback() {
        let (mut app, flight, _) = app();
        app.insert_resource(QualityCapabilities {
            environment_maps: false,
            shadow_map_limit: 2048,
        });
        app.insert_resource(GraphicsQuality::Ultra);
        app.update();
        assert!(app.world().resource::<QualityRuntime>().helper.is_none());
        assert!(app.world().get::<EnvironmentMapLight>(flight).is_none());
        assert_eq!(
            app.world().resource::<DirectionalLightShadowMap>().size,
            2048
        );
        assert_eq!(
            app.world()
                .get::<AtmosphereSettings>(flight)
                .unwrap()
                .sky_view_lut_samples,
            32
        );
    }
}
