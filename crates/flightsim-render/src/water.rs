//! Optional procedural water shading on the existing curved geographic terrain.
//!
//! Light is the original material/mesh path, with no mask, proxy or water material.
//! Upper tiers share one 3 MiB geographic mask and the original mesh handles. A
//! render-world gate selects exactly one draw for each baseline/proxy pair after
//! the actual current-view pipeline is ready; cold/error pipelines retain land.
//! This is a shading slice: no displacement, surf, SSR, bathymetry or forecast.

#[path = "water_mask.rs"]
mod mask;
pub use mask::{
    WATER_MASK_BYTES, WATER_MASK_SIDE, WATER_MASK_TEXELS_PER_UPDATE, WaterMaskBuilder,
    geographic_water_channels,
};
#[path = "water_pipelines.rs"]
mod pipelines;

use crate::{
    CloudLayer, RenderOrigin, SunDirection, TimeOfDay,
    terrain_detail::{SurfaceDetailSettings, TerrainDetailUniform, TerrainMaterial},
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    light::NotShadowCaster,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::{
        RenderApp,
        render_resource::{AsBindGroup, ShaderType},
    },
    shader::ShaderRef,
};
use flightsim_core::{RenderFrame, Seconds};
use flightsim_world::global::GlobalTerrain;
use std::collections::{HashMap, HashSet};

const SHADER: Handle<Shader> = uuid_handle!("be5241e7-59f4-43d0-84a9-5f84fbb7f2db");
const WAVELENGTHS: [f64; 8] = [160.0, 71.0, 33.0, 13.0, 4.8, 1.7, 0.7, 0.25];
const DIRECTIONS: [[f64; 3]; 8] = [
    [0.8, 0.3, 0.5],
    [-0.2, 0.9, 0.3],
    [0.6, -0.4, 0.7],
    [0.4, 0.8, -0.3],
    [-0.7, 0.3, 0.6],
    [0.3, -0.8, 0.5],
    [0.9, 0.1, -0.4],
    [-0.5, -0.6, 0.7],
];

/// Independent from graphics/cloud quality and the physical simulation.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WaterQuality {
    #[default]
    Light,
    High,
    Ultra,
}
impl WaterQuality {
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
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "light" => Some(Self::Light),
            "high" => Some(Self::High),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }
    const fn waves(self) -> f32 {
        match self {
            Self::Light => 0.0,
            Self::High => 6.0,
            Self::Ultra => 8.0,
        }
    }
}

/// App supplies its validated global atlas only when global terrain is enabled.
/// Clone is a shared immutable Arc, never a new dataset or alternate geography.
#[derive(Resource, Debug, Clone)]
pub struct WaterAtlas(pub GlobalTerrain);

/// Logical owned resources, excluding Bevy's shared/cached pipelines and meshes.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct WaterDiagnostics {
    /// True while bounded CPU mask generation is still in progress.
    pub preparing_mask: bool,
    pub mask_bytes: usize,
    pub mask_completed_texels: usize,
    pub proxy_count: usize,
    pub material_count: usize,
    /// These values arrive from the last completed extraction/render gate.
    pub ready_pairs: usize,
    pub fallback_pairs: usize,
    pub failed_pipelines: bool,
}

/// Register after Bevy rendering and TerrainDetailPlugin. No Light assets are built.
#[derive(Debug, Default)]
pub struct WaterSurfacePlugin;
impl Plugin for WaterSurfacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WaterQuality>()
            .init_resource::<WaterDiagnostics>()
            .init_resource::<WaterRuntime>();
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_internal_asset!(app, SHADER, "water.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .add_systems(
                PostUpdate,
                sync_water.before(bevy::transform::TransformSystems::Propagate),
            );
        pipelines::configure(app);
    }
}

/// Upper material retains the existing StandardMaterial land settings.
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterSurface>;
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterSurface {
    #[uniform(100)]
    detail: TerrainDetailUniform,
    #[uniform(101)]
    uniform: WaterUniform,
    #[texture(102, dimension = "cube")]
    #[sampler(103)]
    mask: Handle<Image>,
}
impl MaterialExtension for WaterSurface {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Reflect, ShaderType)]
struct WaterUniform {
    ecef_x: Vec4,
    ecef_y: Vec4,
    ecef_z: Vec4,
    // ECEF anchor divided by the core Earth scale; used ONLY for mask direction.
    // Wave phases below are independently reduced in f64, not derived from this.
    origin: Vec4,
    waves: [Vec4; 8],
    sun: Vec4,
    settings: Vec4,
}
impl WaterUniform {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite normalized vectors and independently reduced wave phases are bounded before f32 conversion"
    )]
    fn for_frame(
        frame: &RenderFrame,
        elapsed: Seconds,
        sun: SunDirection,
        cover: f32,
        quality: WaterQuality,
    ) -> Self {
        let origin = frame.anchor().to_ecef().as_vec();
        let scale = flightsim_core::geodetic::wgs84::MEAN_RADIUS;
        if !origin.is_finite()
            || !elapsed.is_finite()
            || !sun.azimuth.is_finite()
            || !sun.elevation.is_finite()
            || !cover.is_finite()
        {
            return Self::default();
        }
        let waves = std::array::from_fn(|i| {
            let direction = glam::DVec3::from_array(DIRECTIONS[i]).normalize();
            let k = std::f64::consts::TAU / WAVELENGTHS[i];
            let omega = (9.81 * k).sqrt();
            // Reduce time separately before addition; centuries of civil time
            // never enter a large f32 sin() argument or seed the visual field.
            let time_phase = (elapsed.get() % (std::f64::consts::TAU / omega)) * omega;
            let phase = (origin.dot(direction) * k - time_phase).rem_euclid(std::f64::consts::TAU);
            (direction * k).as_vec3().extend(phase as f32)
        });
        let to_sun = -crate::sun_light_direction(sun);
        Self {
            ecef_x: frame.vector_to_render(glam::DVec3::X).extend(0.0),
            ecef_y: frame.vector_to_render(glam::DVec3::Y).extend(0.0),
            ecef_z: frame.vector_to_render(glam::DVec3::Z).extend(0.0),
            origin: (origin / scale).as_vec3().extend((1.0 / scale) as f32),
            waves,
            sun: to_sun.extend(sun.elevation.get().sin() as f32),
            settings: Vec4::new(quality.waves(), cover.clamp(0.0, 1.0), 0.0, 0.0),
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct WaterProxy {
    pub baseline: Entity,
}
#[derive(Resource, Default)]
struct WaterRuntime {
    builder: Option<WaterMaskBuilder>,
    image: Option<Handle<Image>>,
    materials: HashMap<AssetId<TerrainMaterial>, Handle<WaterMaterial>>,
    proxies: HashMap<Entity, Entity>,
}

fn release_water(world: &mut World, runtime: &mut WaterRuntime) {
    for (_, entity) in runtime.proxies.drain() {
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
    for (_, handle) in runtime.materials.drain() {
        world
            .resource_mut::<Assets<WaterMaterial>>()
            .remove(handle.id());
    }
    if let Some(image) = runtime.image.take() {
        world.resource_mut::<Assets<Image>>().remove(image.id());
    }
    // Drop retained map capacities as well as their asset/entity owners.
    *runtime = WaterRuntime::default();
}

fn sync_water(world: &mut World) {
    let quality = *world.resource::<WaterQuality>();
    // The common/default path must not query terrain, generate an atlas, allocate
    // proxy vectors, or even mark the existing material assets as changed.
    if quality == WaterQuality::Light
        && world.resource::<WaterRuntime>().image.is_none()
        && world.resource::<WaterRuntime>().builder.is_none()
    {
        return;
    }
    let mut runtime = world.remove_resource::<WaterRuntime>().unwrap_or_default();
    if quality == WaterQuality::Light
        || !world.contains_resource::<WaterAtlas>()
        || !world.contains_resource::<RenderOrigin>()
    {
        release_water(world, &mut runtime);
        *world.resource_mut::<WaterDiagnostics>() = WaterDiagnostics::default();
        world.insert_resource(runtime);
        return;
    }
    if runtime.image.is_none() {
        if runtime.builder.is_none() {
            runtime.builder = Some(WaterMaskBuilder::new(
                world.resource::<WaterAtlas>().0.clone(),
            ));
        }
        let builder = runtime.builder.as_mut().expect("builder created");
        let ready = builder.advance();
        let diagnostics = &mut *world.resource_mut::<WaterDiagnostics>();
        diagnostics.preparing_mask = !ready;
        diagnostics.mask_bytes = WATER_MASK_BYTES;
        diagnostics.mask_completed_texels = builder.completed_texels();
        if !ready {
            world.insert_resource(runtime);
            return;
        }
        let image = runtime
            .builder
            .take()
            .expect("complete builder")
            .into_image()
            .expect("all six faces complete");
        runtime.image = Some(world.resource_mut::<Assets<Image>>().add(image));
    }
    let frame = &world.resource::<RenderOrigin>().0;
    let detail_enabled = world
        .get_resource::<SurfaceDetailSettings>()
        .is_none_or(|settings| settings.enabled);
    let detail = TerrainDetailUniform::for_frame(frame, detail_enabled);
    let sun = world
        .get_resource::<SunDirection>()
        .copied()
        .unwrap_or_default();
    let clock = world
        .get_resource::<TimeOfDay>()
        .copied()
        .unwrap_or_default();
    let cover = world
        .get_resource::<CloudLayer>()
        .map_or(0.0, |layer| layer.cover);
    let uniform = WaterUniform::for_frame(
        frame,
        Seconds(clock.utc.days_since_j2000() * 86_400.0),
        sun,
        cover,
        quality,
    );
    // Only the displayed cohort; pending replacement terrain never gains proxies.
    let visible: Vec<_> = world
        .query_filtered::<(
            Entity,
            &Mesh3d,
            &MeshMaterial3d<TerrainMaterial>,
            &Visibility,
        ), Or<(
            With<crate::terrain::TerrainTile>,
            With<crate::terrain_stitching::TerrainBridge>,
        )>>()
        .iter(world)
        .filter(|(_, _, _, visibility)| **visibility != Visibility::Hidden)
        .map(|(entity, mesh, material, _)| (entity, mesh.0.clone(), material.0.clone()))
        .collect();
    let active: HashSet<_> = visible.iter().map(|(entity, _, _)| *entity).collect();
    runtime.proxies.retain(|baseline, proxy| {
        if active.contains(baseline) && world.get_entity(*proxy).is_ok() {
            true
        } else {
            if world.get_entity(*proxy).is_ok() {
                world.despawn(*proxy);
            }
            false
        }
    });
    let active_materials: HashSet<_> = visible
        .iter()
        .map(|(_, _, material)| material.id())
        .collect();
    runtime.materials.retain(|id, handle| {
        if active_materials.contains(id) {
            true
        } else {
            world
                .resource_mut::<Assets<WaterMaterial>>()
                .remove(handle.id());
            false
        }
    });
    for (baseline, mesh, material) in visible {
        let Some(base) = world
            .resource::<Assets<TerrainMaterial>>()
            .get(material.id())
            .map(|material| material.base.clone())
        else {
            continue;
        };
        let water_material = runtime
            .materials
            .entry(material.id())
            .or_insert_with(|| {
                world
                    .resource_mut::<Assets<WaterMaterial>>()
                    .add(WaterMaterial {
                        base,
                        extension: WaterSurface {
                            detail,
                            uniform,
                            mask: runtime.image.as_ref().expect("uploaded mask").clone(),
                        },
                    })
            })
            .clone();
        if let Some(proxy) = runtime.proxies.get(&baseline) {
            // Source replacement can reuse an entity; retain only the current mesh.
            if world
                .get::<Mesh3d>(*proxy)
                .is_none_or(|existing| existing.0 != mesh)
            {
                world.entity_mut(*proxy).insert(Mesh3d(mesh));
            }
            if world
                .get::<MeshMaterial3d<WaterMaterial>>(*proxy)
                .is_none_or(|existing| existing.0 != water_material)
            {
                world
                    .entity_mut(*proxy)
                    .insert(MeshMaterial3d(water_material));
            }
        } else {
            let layers = world
                .get::<bevy::camera::visibility::RenderLayers>(baseline)
                .cloned();
            let mut entity = world.spawn((
                WaterProxy { baseline },
                Mesh3d(mesh),
                MeshMaterial3d(water_material),
                Transform::IDENTITY,
                Visibility::Inherited,
                ChildOf(baseline),
                NotShadowCaster,
                Name::new("upper water terrain material"),
            ));
            if let Some(layers) = layers {
                entity.insert(layers);
            }
            runtime.proxies.insert(baseline, entity.id());
        }
    }
    for handle in runtime.materials.values() {
        if let Some(material) = world
            .resource_mut::<Assets<WaterMaterial>>()
            .get_mut(handle.id())
        {
            material.extension.detail = detail;
            material.extension.uniform = uniform;
        }
    }
    let diagnostics = &mut *world.resource_mut::<WaterDiagnostics>();
    diagnostics.proxy_count = runtime.proxies.len();
    diagnostics.material_count = runtime.materials.len();
    world.insert_resource(runtime);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Ecef, Geodetic, LocalFrame, Ned};

    #[test]
    fn waves_preserve_ecef_phase_at_dateline_poles_and_rebase() {
        let sun = SunDirection::default();
        for (lat, lon) in [(35.0, 139.0), (0.0, 179.99), (89.99, 65.0), (-89.99, -95.0)] {
            let anchor = Geodetic::from_degrees(lat, lon, 0.0);
            let local = LocalFrame::new(anchor);
            let first = RenderFrame::new(anchor);
            let second = RenderFrame::new(
                local
                    .ned_to_ecef_position(Ned::new(6000.0, -3000.0, 0.0))
                    .to_geodetic(),
            );
            let p = local.ned_to_ecef_position(Ned::new(100.0, 100.0, 0.0));
            let a = WaterUniform::for_frame(
                &first,
                Seconds(812_345_678.9),
                sun,
                0.0,
                WaterQuality::Ultra,
            );
            let b = WaterUniform::for_frame(
                &second,
                Seconds(812_345_678.9),
                sun,
                0.0,
                WaterQuality::Ultra,
            );
            for (i, wavelength) in WAVELENGTHS.iter().enumerate() {
                let phase = |u: WaterUniform, frame: &RenderFrame| {
                    let p = frame.to_render(p);
                    let ecef = Vec3::new(
                        u.ecef_x.truncate().dot(p),
                        u.ecef_y.truncate().dot(p),
                        u.ecef_z.truncate().dot(p),
                    );
                    f64::from(ecef.dot(u.waves[i].truncate()) + u.waves[i].w)
                };
                let delta = (phase(a, &first) - phase(b, &second) + std::f64::consts::PI)
                    .rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                let error_m = delta.abs() * wavelength / std::f64::consts::TAU;
                assert!(error_m < 0.003, "wave {i} at {lat},{lon}: {error_m} m");
            }
        }
        for (a, b) in [
            ((0.0, 180.0), (0.0, -180.0)),
            ((90.0, 0.0), (90.0, 137.0)),
            ((-90.0, 0.0), (-90.0, -92.0)),
        ] {
            let fa = RenderFrame::new(Geodetic::from_degrees(a.0, a.1, 0.0));
            let fb = RenderFrame::new(Geodetic::from_degrees(b.0, b.1, 0.0));
            let p = Ecef(fa.anchor().to_ecef().as_vec() + glam::DVec3::new(7.0, 9.0, 5.0));
            assert!(
                (fa.to_world(fa.to_render(p)).as_vec() - fb.to_world(fb.to_render(p)).as_vec())
                    .length()
                    < 1e-4
            );
            let ua = WaterUniform::for_frame(&fa, Seconds(0.0), sun, 0.0, WaterQuality::High);
            let ub = WaterUniform::for_frame(&fb, Seconds(0.0), sun, 0.0, WaterQuality::High);
            for i in 0..8 {
                assert!((ua.waves[i] - ub.waves[i]).length() < 1e-4);
            }
        }
    }

    #[test]
    fn invalid_uniforms_disable_water_without_nan() {
        let u = WaterUniform::for_frame(
            &RenderFrame::new(Geodetic::from_degrees(0.0, 0.0, 0.0)),
            Seconds(f64::NAN),
            SunDirection::default(),
            0.0,
            WaterQuality::High,
        );
        assert_eq!(u, WaterUniform::default());
    }

    #[test]
    fn light_allocates_no_water_image_material_proxy_or_builder() {
        let mut app = App::new();
        app.init_resource::<WaterQuality>()
            .init_resource::<WaterRuntime>()
            .init_resource::<WaterDiagnostics>()
            .add_systems(Update, sync_water);
        app.update();
        let runtime = app.world().resource::<WaterRuntime>();
        assert!(
            runtime.image.is_none()
                && runtime.builder.is_none()
                && runtime.proxies.is_empty()
                && runtime.materials.is_empty()
        );
        assert_eq!(app.world().resource::<WaterDiagnostics>().mask_bytes, 0);
    }

    #[test]
    fn actual_water_optics_wgsl_helpers_validate_without_a_gpu() {
        let source = include_str!("water.wgsl");
        let helpers =
            &source[source.find("const SLOPES:").unwrap()..source.find("@fragment").unwrap()];
        let module = naga::front::wgsl::parse_str(helpers).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn upper_cancel_during_mask_build_returns_to_zero_owned_resources() {
        let mut app = App::new();
        app.init_resource::<WaterRuntime>()
            .init_resource::<WaterDiagnostics>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<WaterMaterial>>()
            .insert_resource(WaterQuality::High)
            .insert_resource(WaterAtlas(GlobalTerrain::bundled().unwrap()))
            .insert_resource(RenderOrigin(RenderFrame::new(Geodetic::from_degrees(
                0.0, -140.0, 0.0,
            ))))
            .add_systems(Update, sync_water);
        app.update();
        assert_eq!(
            app.world()
                .resource::<WaterRuntime>()
                .builder
                .as_ref()
                .unwrap()
                .completed_texels(),
            WATER_MASK_TEXELS_PER_UPDATE
        );
        *app.world_mut().resource_mut::<WaterQuality>() = WaterQuality::Light;
        app.update();
        let runtime = app.world().resource::<WaterRuntime>();
        assert!(
            runtime.builder.is_none()
                && runtime.image.is_none()
                && runtime.proxies.is_empty()
                && runtime.materials.is_empty()
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert_eq!(app.world().resource::<WaterDiagnostics>().mask_bytes, 0);
    }

    #[test]
    fn upper_proxy_shares_current_mesh_and_cancel_releases_every_owner() {
        let mut app = App::new();
        app.init_resource::<WaterRuntime>()
            .init_resource::<WaterDiagnostics>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<WaterMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .insert_resource(WaterQuality::High)
            .insert_resource(WaterAtlas(GlobalTerrain::bundled().unwrap()))
            .insert_resource(RenderOrigin(RenderFrame::new(Geodetic::from_degrees(
                0.0, -140.0, 0.0,
            ))))
            .add_systems(Update, sync_water);
        // Bypass expensive geography generation for lifecycle-only coverage.
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut().resource_mut::<WaterRuntime>().image = Some(image);
        let material = app
            .world_mut()
            .resource_mut::<Assets<TerrainMaterial>>()
            .add(crate::terrain_detail::default_surface_detail_material());
        let mesh = Handle::<Mesh>::default();
        let baseline = app
            .world_mut()
            .spawn((
                crate::terrain::TerrainTile(flightsim_world::TileId::new(0, 0, 0)),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Visibility::Visible,
                Transform::IDENTITY,
            ))
            .id();
        let hidden = app
            .world_mut()
            .spawn((
                crate::terrain::TerrainTile(flightsim_world::TileId::new(0, 1, 0)),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Visibility::Hidden,
                Transform::IDENTITY,
            ))
            .id();
        app.update();
        let runtime = app.world().resource::<WaterRuntime>();
        assert_eq!(runtime.proxies.len(), 1);
        assert!(!runtime.proxies.contains_key(&hidden));
        let proxy = runtime.proxies[&baseline];
        assert_eq!(app.world().get::<Mesh3d>(proxy).unwrap().0, mesh);
        assert_eq!(
            app.world().get::<ChildOf>(proxy).unwrap().parent(),
            baseline
        );
        assert!(app.world().get::<NotShadowCaster>(proxy).is_some());
        *app.world_mut().resource_mut::<WaterQuality>() = WaterQuality::Ultra;
        app.update();
        assert_eq!(
            app.world().resource::<WaterRuntime>().proxies[&baseline],
            proxy
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        *app.world_mut().resource_mut::<WaterQuality>() = WaterQuality::Light;
        app.update();
        assert!(app.world().get_entity(proxy).is_err());
        assert!(app.world().get_entity(baseline).is_ok());
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert_eq!(app.world().resource::<Assets<WaterMaterial>>().len(), 0);
    }
    #[test]
    fn huge_finite_visual_time_keeps_phases_bounded() {
        let frame = RenderFrame::new(Geodetic::from_degrees(89.999, 179.999, 0.0));
        for time in [f64::MAX, -f64::MAX, 1.0e30, -1.0e30] {
            let uniform = WaterUniform::for_frame(
                &frame,
                Seconds(time),
                SunDirection::default(),
                0.8,
                WaterQuality::Ultra,
            );
            assert!((uniform.settings.x - 8.0).abs() < f32::EPSILON);
            assert!(
                uniform.waves.iter().all(|wave| wave.is_finite()
                    && wave.w >= 0.0
                    && wave.w <= std::f32::consts::TAU)
            );
        }
    }
}
