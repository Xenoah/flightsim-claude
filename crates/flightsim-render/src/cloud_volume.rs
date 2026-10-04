//! Bounded, render-only cloud shell. The two passes run after atmospheric sky
//! composition and before tonemapping. Current aircraft/cockpit are opaque;
//! arbitrary transparent/transmissive scene content is not depth-composited.
//!
//! The original Light draw path stays resident while an upper tier is requested.
//! A single render-world gate replaces its visible submissions and fog only after
//! the exact pipelines, uniform and targets for this frame are ready. There is
//! no asynchronous main-world visibility acknowledgement and no temporal history.

use crate::cloud_field::{
    CLOUD_FIELD_CELL_SIZE, CLOUD_FIELD_TEXTURE_SIZE, CloudField, cloud_cover_threshold,
    cloud_horizontal_density,
};
use crate::modeled_weather::{NonCloudWeatherFog, RenderWeather, ResolvedCloudLayer};
use crate::{CloudDeckSurface, CloudDistanceFog, RenderOrigin, SunDirection, TimeOfDay};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    pbr::{RenderCascadesVisibleEntities, RenderCubemapVisibleEntities, RenderVisibleMeshEntities},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{
                sampler, texture_2d, texture_3d, texture_depth_2d, texture_depth_2d_multisampled,
                uniform_buffer,
            },
            *,
        },
        renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, ViewDepthTexture, ViewTarget,
            prepare_view_targets,
        },
    },
};
use flightsim_core::{Seconds, geodetic::wgs84};
use flightsim_sim::weather::WeatherSelection;
use std::sync::{Arc, Mutex};

const SHADER: Handle<Shader> = uuid_handle!("69f03d26-74b2-4b09-a8b0-4f81cc7d3302");
const FIELD_SHADER: Handle<Shader> = uuid_handle!("afad6a60-8840-49dc-8a60-4c57d8076a41");

/// Independent of surface/lighting quality. Light preserves the original renderer.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CloudQuality {
    Off,
    #[default]
    Light,
    High,
    Ultra,
}
impl CloudQuality {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Light => "LIGHT",
            Self::High => "HIGH",
            Self::Ultra => "ULTRA",
        }
    }
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "light" => Some(Self::Light),
            "high" => Some(Self::High),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::Light,
            Self::Light => Self::High,
            Self::High => Self::Ultra,
            Self::Ultra => Self::Off,
        }
    }
    #[must_use]
    pub const fn is_volume(self) -> bool {
        matches!(self, Self::High | Self::Ultra)
    }
    #[must_use]
    pub const fn limits(self) -> (u32, u32, u32, u32) {
        match self {
            Self::High => (640, 360, 40, 4),
            Self::Ultra => (960, 540, 64, 6),
            _ => (0, 0, 0, 0),
        }
    }
}

/// Add to the one flight camera. Additional marked views retain Light.
#[derive(Component, Debug, Clone, Copy, Default, ExtractComponent)]
pub struct CloudVolumeCamera;

/// Last completed preparation, for observation only; never gates baseline draws.
#[derive(Resource, Debug, Clone)]
pub struct CloudVolumeDiagnostics {
    pub requested: CloudQuality,
    pub effective: CloudQuality,
    pub ready: bool,
    pub status: &'static str,
    pub target_size: UVec2,
    pub target_bytes: u64,
    pub noise_bytes: u64,
    pub uniform_bytes: u64,
    pub source_bytes: u64,
    pub noise_generation_count: u64,
    pub density_upload_count: u64,
    pub target_allocation_count: u64,
    pub last_upload_bytes: u64,
    pub view_samples: u32,
    pub sun_samples: u32,
}
impl Default for CloudVolumeDiagnostics {
    fn default() -> Self {
        Self {
            requested: CloudQuality::Light,
            effective: CloudQuality::Light,
            ready: false,
            status: "Light",
            target_size: UVec2::ZERO,
            target_bytes: 0,
            noise_bytes: 0,
            uniform_bytes: 0,
            source_bytes: 0,
            noise_generation_count: 0,
            density_upload_count: 0,
            target_allocation_count: 0,
            last_upload_bytes: 0,
            view_samples: 0,
            sun_samples: 0,
        }
    }
}
impl CloudVolumeDiagnostics {
    #[must_use]
    pub const fn effective_quality(&self) -> CloudQuality {
        self.effective
    }
}
#[derive(Resource, Clone, Default)]
struct CloudFeedback(Arc<Mutex<CloudVolumeDiagnostics>>);

#[derive(Default)]
pub(crate) struct CloudVolumePlugin;
impl Plugin for CloudVolumePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloudQuality>()
            .init_resource::<CloudVolumeDiagnostics>()
            .init_resource::<CloudFeedback>();
        // AssetPlugin alone does not register Assets<Shader>; RenderPlugin
        // owns that resource. Asset-enabled headless Apps must stay supported.
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_internal_asset!(app, SHADER, "cloud_volume.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, FIELD_SHADER, "cloud_field.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<CloudVolumeCamera>::default())
            .add_systems(Update, receive_diagnostics);
        let feedback = app.world().resource::<CloudFeedback>().clone();
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(feedback)
            .init_resource::<CloudGpuState>()
            .add_systems(ExtractSchedule, extract_cloud_inputs)
            .add_systems(
                Render,
                prepare_clouds
                    .in_set(RenderSystems::ManageViews)
                    .after(prepare_view_targets),
            )
            .add_render_graph_node::<ViewNodeRunner<CloudNode>>(Core3d, CloudNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::EndMainPass,
                    CloudNodeLabel,
                    Node3d::StartMainPassPostProcessing,
                ),
            );
    }
}
fn ready_camera_fog(remaining: Option<&NonCloudWeatherFog>) -> DistanceFog {
    remaining.map_or_else(crate::weather::inactive_cloud_distance_fog, |fog| {
        fog.0.clone()
    })
}

fn receive_diagnostics(
    quality: Res<CloudQuality>,
    feedback: Res<CloudFeedback>,
    mut diagnostics: ResMut<CloudVolumeDiagnostics>,
) {
    if let Ok(value) = feedback.0.lock() {
        *diagnostics = value.clone();
    }
    diagnostics.requested = *quality;
    if !quality.is_volume() {
        diagnostics.effective = *quality;
        diagnostics.ready = false;
    }
}

#[derive(Resource)]
struct CloudInputs {
    quality: CloudQuality,
    layer: ResolvedCloudLayer,
    weather: RenderWeather,
    origin: Option<RenderOrigin>,
    sun: SunDirection,
    clock: TimeOfDay,
    decks: Vec<MainEntity>,
    fog_cameras: Vec<(MainEntity, DistanceFog)>,
}
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Extracts bounded cloud inputs and each owned camera’s optional non-cloud fog"
)]
fn extract_cloud_inputs(
    mut commands: Commands,
    quality: Extract<Res<CloudQuality>>,
    layer: Extract<Res<ResolvedCloudLayer>>,
    weather: Extract<Res<RenderWeather>>,
    origin: Extract<Option<Res<RenderOrigin>>>,
    sun: Extract<Res<SunDirection>>,
    clock: Extract<Res<TimeOfDay>>,
    decks: Extract<Query<Entity, With<CloudDeckSurface>>>,
    fog: Extract<Query<(Entity, Option<&NonCloudWeatherFog>), With<CloudDistanceFog>>>,
) {
    let collect_owners = quality.is_volume() && !layer.is_clear();
    commands.insert_resource(CloudInputs {
        quality: **quality,
        layer: **layer,
        weather: **weather,
        origin: origin.as_ref().map(|v| **v),
        sun: **sun,
        clock: **clock,
        decks: if collect_owners {
            decks.iter().map(MainEntity::from).collect()
        } else {
            Vec::new()
        },
        fog_cameras: if collect_owners {
            fog.iter()
                .map(|(entity, remaining)| (MainEntity::from(entity), ready_camera_fog(remaining)))
                .collect()
        } else {
            Vec::new()
        },
    });
}

#[derive(Clone, Default, ShaderType)]
struct CloudUniform {
    view_from_clip: Mat4,
    world_from_view: Mat4,
    // ECEF axes in xyz map camera-relative rays. The otherwise unused w
    // components contain a CPU-f64-reduced, camera-relative3D detail phase.
    ecef_x: Vec4,
    ecef_y: Vec4,
    ecef_z: Vec4,
    camera_ecef_cells: Vec4,
    drift: Vec4,
    // Local spherical-shell up.xyz and camera radius.w; radius is CPU f64-derived.
    shell: Vec4,
    // camera altitude, layer base, thickness, extinction per metre.
    layer: Vec4,
    // Stable sphere intersection c values (earth/base/top), threshold.
    intersections: Vec4,
    sun: Vec4,
    // sunlight RGB radiance, ambient radiance.
    light: Vec4,
    // viewport xy origin and zw size.
    viewport: Vec4,
    // target width,height,view sample count,sun sample count.
    samples: Vec4,
    seed: UVec4,
}

struct CloudPipelines {
    ray_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    ray: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}
struct CloudTargets {
    size: UVec2,
    _color: Texture,
    _depth: Texture,
    color_view: TextureView,
    depth_view: TextureView,
}
struct PreparedCloud {
    entity: Entity,
    ray: RenderPipeline,
    composite: RenderPipeline,
    ray_layout: BindGroupLayout,
    composite_layout: BindGroupLayout,
    viewport: UVec4,
}
struct CloudNoise {
    seed: u64,
    source: Vec<u8>,
    cover_key: Option<u8>,
    _texture: Texture,
    view: TextureView,
    sampler: Sampler,
}
#[derive(Resource, Default)]
struct CloudGpuState {
    variants: [Option<CloudPipelines>; 2],
    targets: Option<CloudTargets>,
    prepared: Option<PreparedCloud>,
    uniform: Option<UniformBuffer<CloudUniform>>,
    noise: Option<CloudNoise>,
    noise_generation_count: u64,
    density_upload_count: u64,
    target_allocation_count: u64,
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Branch selects the limiting cap; resulting dimensions are at most960by540"
)]
fn bounded_target(viewport: UVec2, quality: CloudQuality) -> UVec2 {
    let (w, h, _, _) = quality.limits();
    if viewport.min_element() == 0 || w == 0 {
        return UVec2::ZERO;
    }
    // Integer arithmetic rounds down and never exceeds either hard dimension.
    let numerator = u64::from(w).saturating_mul(u64::from(viewport.y));
    let denominator = u64::from(h).saturating_mul(u64::from(viewport.x));
    if numerator <= denominator {
        UVec2::new(
            w.min(viewport.x),
            ((u64::from(viewport.y) * u64::from(w.min(viewport.x))) / u64::from(viewport.x)).max(1)
                as u32,
        )
    } else {
        UVec2::new(
            ((u64::from(viewport.x) * u64::from(h.min(viewport.y))) / u64::from(viewport.y)).max(1)
                as u32,
            h.min(viewport.y),
        )
    }
}

fn create_pipelines(
    cache: &PipelineCache,
    fullscreen: &FullscreenShader,
    multisampled: bool,
) -> CloudPipelines {
    let depth_entry = if multisampled {
        texture_depth_2d_multisampled()
    } else {
        texture_depth_2d()
    };
    let ray_layout = BindGroupLayoutDescriptor::new(
        "cloud ray layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (0, uniform_buffer::<CloudUniform>(false)),
                (1, depth_entry),
                (4, texture_3d(TextureSampleType::Float { filterable: true })),
                (5, sampler(SamplerBindingType::Filtering)),
            ),
        ),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "cloud composite layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<CloudUniform>(false),
                depth_entry,
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    let defs = if multisampled {
        vec!["MULTISAMPLED".into()]
    } else {
        vec![]
    };
    let ray = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("bounded cloud raymarch".into()),
        layout: vec![ray_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs: defs.clone(),
            entry_point: Some("raymarch".into()),
            targets: vec![
                Some(TextureFormat::Rgba16Float.into()),
                Some(TextureFormat::R32Float.into()),
            ],
        }),
        ..default()
    });
    let composite = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("depth aware cloud composite".into()),
        layout: vec![composite_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs: defs,
            entry_point: Some("composite".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrites::ALL,
            })],
        }),
        ..default()
    });
    CloudPipelines {
        ray_layout,
        composite_layout,
        ray,
        composite,
    }
}
fn create_noise(device: &RenderDevice, seed: u64) -> CloudNoise {
    let source = CloudField::new(seed).texture_bytes();
    let size = CLOUD_FIELD_TEXTURE_SIZE;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("original cloud noise and filtered density RG8"),
        size: Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: size,
        },
        mip_level_count: 7,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: TextureFormat::Rg8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("periodic filtered cloud field"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: FilterMode::Linear,
        ..default()
    });
    CloudNoise {
        seed,
        source,
        cover_key: None,
        _texture: texture,
        view,
        sampler,
    }
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Cover is validated in0..1; density quantization is bounded to8bits"
)]
fn density_mips(source: &[u8], cover_key: u8) -> Vec<Vec<u8>> {
    let threshold = cloud_cover_threshold(f32::from(cover_key) / 255.0);
    let mut base = source.to_vec();
    for texel in base.chunks_exact_mut(2) {
        let density = cloud_horizontal_density(f32::from(texel[0]) / 255.0, threshold);
        texel[1] = if density > 0.0 {
            (density * 255.0).round().max(1.0) as u8
        } else {
            0
        };
    }
    let mut result = vec![base];
    let mut size = CLOUD_FIELD_TEXTURE_SIZE as usize;
    while size > 1 {
        let next_size = size / 2;
        let previous = result.last().expect("base mip exists");
        let mut next = vec![0u8; next_size * next_size * next_size * 2];
        for z in 0..next_size {
            for y in 0..next_size {
                for x in 0..next_size {
                    for channel in 0..2 {
                        let mut sum = 0u16;
                        for dz in 0..2 {
                            for dy in 0..2 {
                                for dx in 0..2 {
                                    let index = (((z * 2 + dz) * size + (y * 2 + dy)) * size
                                        + (x * 2 + dx))
                                        * 2
                                        + channel;
                                    sum += u16::from(previous[index]);
                                }
                            }
                        }
                        next[((z * next_size + y) * next_size + x) * 2 + channel] =
                            ((sum + 4) / 8) as u8;
                    }
                }
            }
        }
        result.push(next);
        size = next_size;
    }
    result
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Validated cover uses nearest1/255 cache key, maximum error0.5/255"
)]
fn update_density_texture(noise: &mut CloudNoise, queue: &RenderQueue, cover: f32) -> bool {
    let key = (cover * 255.0).round() as u8;
    if noise.cover_key == Some(key) {
        return false;
    }
    for (mip, bytes) in density_mips(&noise.source, key).iter().enumerate() {
        let size = CLOUD_FIELD_TEXTURE_SIZE >> mip;
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &noise._texture,
                mip_level: mip as u32,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytes,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 2),
                rows_per_image: Some(size),
            },
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
        );
    }
    noise.cover_key = Some(key);
    true
}

fn create_targets(device: &RenderDevice, size: UVec2) -> CloudTargets {
    let create = |label, format| {
        device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let color = create("cloud radiance RGBA16F", TextureFormat::Rgba16Float);
    let depth = create("cloud scene guide R32F", TextureFormat::R32Float);
    let color_view = color.create_view(&TextureViewDescriptor::default());
    let depth_view = depth.create_view(&TextureViewDescriptor::default());
    CloudTargets {
        size,
        _color: color,
        _depth: depth,
        color_view,
        depth_view,
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "One same-frame render gate owns baseline suppression and GPU readiness"
)]
fn prepare_clouds(
    mut commands: Commands,
    inputs: Option<Res<CloudInputs>>,
    mut state: ResMut<CloudGpuState>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    adapter: Res<RenderAdapter>,
    cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    feedback: Res<CloudFeedback>,
    mut views: Query<
        (
            Entity,
            &ExtractedView,
            &ExtractedCamera,
            &ViewTarget,
            &Msaa,
            &mut Camera3d,
        ),
        With<CloudVolumeCamera>,
    >,
    mut visible: Query<&mut RenderVisibleEntities>,
    mut cascades: Query<&mut RenderCascadesVisibleEntities>,
    mut cubemaps: Query<&mut RenderCubemapVisibleEntities>,
    mut spots: Query<&mut RenderVisibleMeshEntities>,
) {
    state.prepared = None;
    let Some(inputs) = inputs else {
        state.targets = None;
        return;
    };
    let mut report = CloudVolumeDiagnostics {
        requested: inputs.quality,
        effective: if inputs.quality == CloudQuality::Off {
            CloudQuality::Off
        } else {
            CloudQuality::Light
        },
        status: "Light fallback",
        ..default()
    };
    let mut prepare = || {
        if !inputs.quality.is_volume() || inputs.layer.is_clear() {
            state.targets = None;
            state.uniform = None;
            state.noise = None;
            report.status = if inputs.layer.is_clear() {
                "Clear"
            } else {
                inputs.quality.name()
            };
            return;
        }
        let Some(origin) = inputs.origin else {
            report.status = "Waiting for origin";
            return;
        };
        let Some((entity, view, camera, target, msaa, mut camera3d)) = views.iter_mut().next()
        else {
            state.targets = None;
            report.status = "Waiting for flight camera";
            return;
        };
        let required = TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        if device.limits().max_texture_dimension_3d < CLOUD_FIELD_TEXTURE_SIZE
            || !view.hdr
            || target.main_texture_format() != TextureFormat::Rgba16Float
            || [TextureFormat::Rgba16Float, TextureFormat::R32Float]
                .into_iter()
                .any(|format| {
                    !adapter
                        .get_texture_format_features(format)
                        .allowed_usages
                        .contains(required)
                })
        {
            state.targets = None;
            report.status = "Unsupported HDR targets";
            return;
        }
        let viewport = UVec2::new(view.viewport.z, view.viewport.w);
        let size = bounded_target(viewport, inputs.quality);
        if size.min_element() == 0 || size.max_element() > device.limits().max_texture_dimension_2d
        {
            state.targets = None;
            report.status = "Empty or unsupported viewport";
            return;
        }
        let variant_index = usize::from(msaa.samples() > 1);
        if state.variants[variant_index].is_none() {
            state.variants[variant_index] =
                Some(create_pipelines(&cache, &fullscreen, variant_index != 0));
        }
        let pipelines = state.variants[variant_index]
            .as_ref()
            .expect("created above");
        let (Some(ray), Some(composite)) = (
            cache.get_render_pipeline(pipelines.ray),
            cache.get_render_pipeline(pipelines.composite),
        ) else {
            report.status = if matches!(
                cache.get_render_pipeline_state(pipelines.ray),
                CachedPipelineState::Err(_)
            ) || matches!(
                cache.get_render_pipeline_state(pipelines.composite),
                CachedPipelineState::Err(_)
            ) {
                "Shader error; Light fallback"
            } else {
                "Compiling; Light fallback"
            };
            return;
        };
        let prepared_ray = ray.clone();
        let prepared_composite = composite.clone();
        let ray_layout = cache.get_bind_group_layout(&pipelines.ray_layout).clone();
        let composite_layout = cache
            .get_bind_group_layout(&pipelines.composite_layout)
            .clone();
        let Some(uniform) = cloud_uniform(&inputs, &origin, view, camera.exposure, size) else {
            report.status = "Invalid cloud inputs";
            return;
        };
        if state
            .targets
            .as_ref()
            .is_none_or(|targets| targets.size != size)
        {
            // Drop our old owner before constructing its replacement. Submitted
            // command buffers may still retain GPU resources until completion.
            state.targets = None;
            state.targets = Some(create_targets(&device, size));
            state.target_allocation_count += 1;
        }
        if state
            .noise
            .as_ref()
            .is_none_or(|noise| noise.seed != inputs.layer.seed)
        {
            state.noise = None;
            state.noise = Some(create_noise(&device, inputs.layer.seed));
            state.noise_generation_count += 1;
        }
        if update_density_texture(
            state.noise.as_mut().expect("created noise"),
            &queue,
            inputs.layer.cover,
        ) {
            state.density_upload_count += 1;
            report.last_upload_bytes = 599_186;
        }
        let buffer = state.uniform.get_or_insert_with(UniformBuffer::default);
        buffer.set(uniform);
        buffer.set_label(Some("cloud per-view uniform"));
        buffer.write_buffer(&device, &queue);
        // Core3d prepares its actual sampled depth later in PrepareResources.
        camera3d.depth_texture_usages.0 |= TextureUsages::TEXTURE_BINDING.bits();
        state.prepared = Some(PreparedCloud {
            entity,
            ray: prepared_ray,
            composite: prepared_composite,
            ray_layout,
            composite_layout,
            viewport: view.viewport,
        });
        // Extraction restores both next frame. No main-world visibility/fog is
        // changed, so a canceled request or failed shader returns to Light.
        // Keep DistanceFog present: Bevy specializes every PBR material on its
        // presence. Removing it can make the scene disappear while cold mesh
        // pipelines compile when a cloud tier changes.
        if let Some((_, remaining)) = inputs
            .fog_cameras
            .iter()
            .find(|(main, _)| *main == view.retained_view_entity.main_entity)
        {
            commands.entity(entity).insert(remaining.clone());
        }
        if let Ok(mut entities) = visible.get_mut(entity) {
            for list in entities.entities.values_mut() {
                list.retain(|(_, main)| !inputs.decks.contains(main));
            }
        }
        // Legacy masked planes must not keep casting flat cloud shadows while
        // volume rendering is active. The volume itself casts no ground shadow.
        for mut light in &mut cascades {
            if let Some(views) = light.entities.get_mut(&entity) {
                for view in views {
                    view.entities
                        .retain(|(_, main)| !inputs.decks.contains(main));
                }
            }
        }
        for mut light in &mut cubemaps {
            for face in light.iter_mut() {
                face.entities
                    .retain(|(_, main)| !inputs.decks.contains(main));
            }
        }
        for mut light in &mut spots {
            light
                .entities
                .retain(|(_, main)| !inputs.decks.contains(main));
        }
        let (_, _, view_samples, sun_samples) = inputs.quality.limits();
        report.effective = inputs.quality;
        report.ready = true;
        report.status = "Ready";
        report.target_size = size;
        report.target_bytes = u64::from(size.x) * u64::from(size.y) * 12;
        report.noise_bytes = 599_186;
        report.uniform_bytes = CloudUniform::min_size().get();
        report.source_bytes = 524_288;
        report.view_samples = view_samples;
        report.sun_samples = sun_samples;
    };
    prepare();
    if !report.ready {
        state.targets = None;
        state.noise = None;
        state.uniform = None;
    }
    report.noise_generation_count = state.noise_generation_count;
    report.density_upload_count = state.density_upload_count;
    report.target_allocation_count = state.target_allocation_count;
    if let Ok(mut output) = feedback.0.lock() {
        *output = report;
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "Finite CPU f64 canonical values are narrowed only for bounded render uniforms"
)]
fn cloud_uniform(
    inputs: &CloudInputs,
    origin: &RenderOrigin,
    view: &ExtractedView,
    exposure: f32,
    size: UVec2,
) -> Option<CloudUniform> {
    if !inputs.layer.is_valid() || view.clip_from_view.w_axis.w.abs() > f32::EPSILON {
        return None;
    }
    let camera_render = view.world_from_view.translation();
    let world = origin.0.to_world(camera_render);
    let elapsed = match inputs.weather.selection {
        WeatherSelection::Legacy => Seconds(inputs.clock.utc.days_since_j2000() * 86400.0),
        WeatherSelection::Modeled(_) => inputs.weather.elapsed,
    };
    let drift = Vec3::from_array(CloudField::drift(elapsed)?);
    let camera_ecef_cells = (world.as_vec() / CLOUD_FIELD_CELL_SIZE.get()).as_vec3();
    // One kilometre detail cells use the same ECEF advection as the macro field.
    // Reduce canonical f64 BEFORE narrowing; never subtract giant ECEF floats.
    let detail_phase = ((world.as_vec() / 1_000.0 + drift.as_dvec3() * 8.0)
        .rem_euclid(glam::DVec3::splat(16.0)))
    .as_vec3();
    // An explicitly local spherical shell tangent to the WGS84 anchor. This is
    // a render approximation only; core owns all canonical ECEF conversion.
    let radius = wgs84::MEAN_RADIUS;
    let relative = camera_render.as_dvec3() + glam::DVec3::Y * radius;
    let camera_radius = relative.length();
    let camera_height = camera_radius - radius;
    let base = inputs.layer.base.get();
    let top = inputs.layer.top.get();
    let c = |h| ((camera_height - h) * (2.0 * radius + camera_height + h)) as f32;
    let to_sun = -crate::sun_light_direction(inputs.sun);
    let elevation = inputs.sun.elevation.get();
    let daylight = ((elevation + 0.08) / 0.18).clamp(0.0, 1.0) as f32;
    let daylight = daylight * daylight * (3.0 - 2.0 * daylight);
    // Single explicit sun attenuation for cloud lighting. Terrain/sky keep the
    // engine atmosphere. This artistically bounded approximation is not METAR.
    let direct = crate::direct_normal_illuminance(
        inputs.sun.elevation,
        crate::daylight::EXTRATERRESTRIAL_ILLUMINANCE,
    ) * exposure
        / std::f32::consts::PI;
    let warm = (1.0 - to_sun.y.max(0.0)).powi(4);
    let color = Vec3::new(1.0, 1.0 - 0.28 * warm, 1.0 - 0.55 * warm) * direct;
    let (_, _, steps, sun_steps) = inputs.quality.limits();
    if !camera_ecef_cells.is_finite()
        || !camera_radius.is_finite()
        || !exposure.is_finite()
        || !view.clip_from_view.is_finite()
    {
        return None;
    }
    let mut world_from_view = view.world_from_view.to_matrix();
    world_from_view.w_axis = Vec4::W;
    let uniform = CloudUniform {
        view_from_clip: view.clip_from_view.inverse(),
        world_from_view,
        ecef_x: origin
            .0
            .vector_to_render(glam::DVec3::X)
            .extend(detail_phase.x),
        ecef_y: origin
            .0
            .vector_to_render(glam::DVec3::Y)
            .extend(detail_phase.y),
        ecef_z: origin
            .0
            .vector_to_render(glam::DVec3::Z)
            .extend(detail_phase.z),
        camera_ecef_cells: camera_ecef_cells.extend((1.0 / CLOUD_FIELD_CELL_SIZE.get()) as f32),
        drift: drift.extend(0.0),
        shell: (relative / camera_radius)
            .as_vec3()
            .extend(camera_radius as f32),
        layer: Vec4::new(
            camera_height as f32,
            base as f32,
            (top - base) as f32,
            if inputs.layer.modeled {
                crate::modeled_weather::modeled_extinction(inputs.layer.visibility)
            } else {
                crate::weather::fog_extinction(inputs.layer.visibility)
            },
        ),
        intersections: Vec4::new(
            // The legacy sea-level blocker would hide authored sub-zero cloud
            // decks over below-sea-level terrain. Keep it below this layer;
            // actual opaque terrain remains the primary depth constraint.
            c(if inputs.layer.modeled {
                base.min(0.0) - 1.0
            } else {
                0.0
            }),
            c(base),
            c(top),
            cloud_cover_threshold(inputs.layer.cover),
        ),
        sun: to_sun.extend(daylight),
        light: color.extend(0.001 + 0.22 * daylight),
        viewport: view.viewport.as_vec4(),
        samples: Vec4::new(size.x as f32, size.y as f32, steps as f32, sun_steps as f32),
        seed: UVec4::new(
            CloudField::new(inputs.layer.seed).seed(),
            match inputs.weather.selection {
                WeatherSelection::Legacy => 0,
                WeatherSelection::Modeled(scenario) => scenario
                    .parameters()
                    .cloud
                    .map_or(0, |cloud| u32::from(cloud.morphology as u16)),
            },
            0,
            0,
        ),
    };
    uniform.is_valid().then_some(uniform)
}
impl CloudUniform {
    fn is_valid(&self) -> bool {
        self.view_from_clip.is_finite()
            && self.world_from_view.is_finite()
            && [
                self.ecef_x,
                self.ecef_y,
                self.ecef_z,
                self.camera_ecef_cells,
                self.drift,
                self.shell,
                self.layer,
                self.intersections,
                self.sun,
                self.light,
                self.viewport,
                self.samples,
            ]
            .into_iter()
            .all(|value| value.is_finite())
            && self.layer.z > 0.0
            && self.layer.w >= 0.0
            && self.shell.w > 0.0
            && self.shell.w < 1.0e9
            && self.viewport.z > 0.0
            && self.viewport.w > 0.0
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct CloudNodeLabel;
#[derive(Default)]
struct CloudNode;
impl ViewNode for CloudNode {
    type ViewQuery = (Entity, &'static ViewTarget, &'static ViewDepthTexture);
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (entity, target, depth): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let state = world.resource::<CloudGpuState>();
        let Some(prepared) = state.prepared.as_ref().filter(|p| p.entity == entity) else {
            return Ok(());
        };
        let targets = state
            .targets
            .as_ref()
            .expect("same-frame cloud gate owns targets");
        let uniform = state
            .uniform
            .as_ref()
            .expect("same-frame cloud gate owns uniform")
            .binding()
            .expect("same-frame cloud gate wrote uniform");
        let noise = state
            .noise
            .as_ref()
            .expect("same-frame cloud gate owns noise");
        let ray_bind = context.render_device().create_bind_group(
            "cloud ray binding",
            &prepared.ray_layout,
            &BindGroupEntries::with_indices((
                (0, uniform.clone()),
                (1, BindingResource::TextureView(depth.view())),
                (4, BindingResource::TextureView(&noise.view)),
                (5, BindingResource::Sampler(&noise.sampler)),
            )),
        );
        let composite_bind = context.render_device().create_bind_group(
            "cloud composite binding",
            &prepared.composite_layout,
            &BindGroupEntries::sequential((
                uniform,
                depth.view(),
                &targets.color_view,
                &targets.depth_view,
            )),
        );
        {
            let attachments = [
                Some(RenderPassColorAttachment {
                    view: &targets.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(LinearRgba::NONE.into()),
                        store: StoreOp::Store,
                    },
                }),
                Some(RenderPassColorAttachment {
                    view: &targets.depth_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(LinearRgba::BLACK.into()),
                        store: StoreOp::Store,
                    },
                }),
            ];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("bounded cloud raymarch"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_render_pipeline(&prepared.ray);
            pass.set_bind_group(0, &ray_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let attachments = [Some(RenderPassColorAttachment {
                view: target.main_texture_view(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("depth-aware premultiplied cloud composite"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let v = prepared.viewport;
            let vf = v.as_vec4();
            pass.set_viewport(vf.x, vf.y, vf.z, vf.w, 0.0, 1.0);
            pass.set_scissor_rect(v.x, v.y, v.z, v.w);
            pass.set_render_pipeline(&prepared.composite);
            pass.set_bind_group(0, &composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CloudLayer;
    #[test]
    fn readiness_suppresses_only_cloud_owned_extinction() {
        let remaining = NonCloudWeatherFog(DistanceFog {
            color: Color::srgb(0.5, 0.5, 0.5),
            falloff: FogFalloff::Exponential { density: 0.012 },
            ..default()
        });
        let fog = ready_camera_fog(Some(&remaining));
        assert_eq!(fog.color, remaining.0.color);
        assert!(
            matches!(fog.falloff,FogFalloff::Exponential { density } if density.to_bits()==0.012_f32.to_bits())
        );
        let legacy = ready_camera_fog(None);
        assert_eq!(legacy.color, Color::NONE);
    }

    #[test]
    fn inactive_fog_preserves_the_engine_mesh_specialization_key() {
        use bevy::pbr::{
            MeshPipelineKey, ViewKeyCache, ViewSpecializationTicks, check_views_need_specialization,
        };
        use bevy::render::view::RetainedViewEntity;

        let mut app = App::new();
        app.init_resource::<ViewKeyCache>()
            .init_resource::<ViewSpecializationTicks>()
            .add_systems(Update, check_views_need_specialization);
        let retained = RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0);
        let camera = app
            .world_mut()
            .spawn((
                ExtractedView {
                    retained_view_entity: retained,
                    clip_from_view: Mat4::IDENTITY,
                    world_from_view: GlobalTransform::IDENTITY,
                    clip_from_world: None,
                    hdr: true,
                    viewport: UVec4::new(0, 0, 1280, 720),
                    color_grading: default(),
                    invert_culling: false,
                },
                Msaa::Sample4,
                DistanceFog::default(),
            ))
            .id();
        app.update();
        let baseline = app.world().resource::<ViewKeyCache>()[&retained];
        assert!(baseline.contains(MeshPipelineKey::DISTANCE_FOG));

        // Upper rendering and Off both retain the inert component; extraction
        // can restore active Light fog without selecting any new mesh pipeline.
        for active in [false, true, false, true] {
            let fog = if active {
                DistanceFog::default()
            } else {
                let fog = crate::weather::inactive_cloud_distance_fog();
                assert_eq!(fog.color, Color::NONE);
                assert_eq!(fog.directional_light_color, Color::NONE);
                assert!(
                    matches!(fog.falloff, FogFalloff::Exponential { density } if density == 0.0)
                );
                fog
            };
            app.world_mut().entity_mut(camera).insert(fog);
            app.update();
            assert_eq!(app.world().resource::<ViewKeyCache>()[&retained], baseline);
        }

        // Establish that this test catches the original component-removal bug.
        app.world_mut().entity_mut(camera).remove::<DistanceFog>();
        app.update();
        assert_ne!(app.world().resource::<ViewKeyCache>()[&retained], baseline);
    }

    #[test]
    fn asset_enabled_headless_app_needs_no_shader_or_gpu_resources() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(CloudQuality::High)
            .add_plugins(CloudVolumePlugin);
        app.update();
        assert_eq!(*app.world().resource::<CloudQuality>(), CloudQuality::High);
        assert!(app.get_sub_app(RenderApp).is_none());
        assert!(!app.world().contains_resource::<Assets<Shader>>());
        let diagnostics = app.world().resource::<CloudVolumeDiagnostics>();
        assert!(!diagnostics.ready);
        assert_eq!(
            diagnostics.target_bytes + diagnostics.noise_bytes + diagnostics.uniform_bytes,
            0
        );
    }

    #[test]
    fn both_depth_variants_parse_and_validate_the_real_shader() {
        for multisampled in [false, true] {
            let mut source = String::from(
                "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, };\n",
            );
            let mut include = true;
            for line in include_str!("cloud_field.wgsl")
                .lines()
                .chain(include_str!("cloud_volume.wgsl").lines())
            {
                if line.starts_with("#define") || line.starts_with("#import") {
                    continue;
                }
                if line.starts_with("#ifdef MULTISAMPLED") {
                    include = multisampled;
                    continue;
                }
                if line.starts_with("#else") {
                    include = !include;
                    continue;
                }
                if line.starts_with("#endif") {
                    include = true;
                    continue;
                }
                if include {
                    source.push_str(line);
                    source.push('\n');
                }
            }
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        }
    }

    #[test]
    fn actual_shader_depth_policy_keeps_prefixes_and_rejects_hidden_segments() {
        // Evaluate the real WGSL helper's scalar expression tree, rather than
        // reimplementing its policy in Rust. Expected values describe geometric
        // visibility: an integrated prefix is valid; a segment behind an opaque
        // surface must not overwrite it, including sub-metre cockpit surfaces.
        let shader = include_str!("cloud_volume.wgsl");
        let start = shader
            .find("fn cloud_guide_weight(")
            .expect("shader depth policy");
        let end = shader[start..].find("\n}").expect("policy function end") + start + 2;
        let module =
            naga::front::wgsl::parse_str(&shader[start..end]).expect("standalone depth policy");
        let (_, function) = module.functions.iter().next().expect("policy function");
        let result = function
            .body
            .iter()
            .find_map(|statement| match statement {
                naga::Statement::Return { value } => *value,
                _ => None,
            })
            .expect("scalar policy result");
        fn evaluate(
            function: &naga::Function,
            expression: naga::Handle<naga::Expression>,
            inputs: [f32; 2],
        ) -> f32 {
            let sub = |value| evaluate(function, value, inputs);
            match function.expressions[expression] {
                naga::Expression::Literal(naga::Literal::F32(value)) => value,
                naga::Expression::FunctionArgument(index) => inputs[index as usize],
                naga::Expression::Binary { op, left, right } => match op {
                    naga::BinaryOperator::Subtract => sub(left) - sub(right),
                    naga::BinaryOperator::Multiply => sub(left) * sub(right),
                    _ => panic!("unsupported policy operator {op:?}"),
                },
                naga::Expression::Math {
                    fun,
                    arg,
                    arg1,
                    arg2,
                    ..
                } => match fun {
                    naga::MathFunction::Max => sub(arg).max(sub(arg1.expect("max rhs"))),
                    naga::MathFunction::SmoothStep => {
                        let edge0 = sub(arg);
                        let edge1 = sub(arg1.expect("edge1"));
                        let value = sub(arg2.expect("value"));
                        let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    }
                    _ => panic!("unsupported policy math {fun:?}"),
                },
                ref other => panic!("unsupported policy expression {other:?}"),
            }
        }
        for (guide, opaque, expected) in [
            (200_000.0, 700_000.0, 1.0),
            (500.0, 1000.0, 1.0),
            (1000.0, 1000.0, 1.0),
            (2000.0, 1000.0, 0.0),
            (10.0, 0.5, 0.0),
            (0.25, 0.5, 1.0),
            (1015.0, 1000.0, 0.5),
        ] {
            let weight = evaluate(function, result, [guide, opaque]);
            assert!(
                (weight - expected).abs() < 1e-6,
                "guide={guide} opaque={opaque}: {weight}"
            );
        }
    }

    #[test]
    fn density_mips_keep_clear_overcast_and_bounded_horizontal_density() {
        let size = CLOUD_FIELD_TEXTURE_SIZE as usize;
        let mut source = vec![0; size * size * size * 2];
        for (index, texel) in source.chunks_exact_mut(2).enumerate() {
            texel[0] = if index % 2 == 0 { 64 } else { 192 };
        }
        for (key, expected) in [(0, 0), (255, 255)] {
            let mips = density_mips(&source, key);
            assert_eq!(mips.len(), 7);
            assert_eq!(mips.iter().map(Vec::len).sum::<usize>(), 599_186);
            assert!(
                mips.iter()
                    .all(|mip| mip.chunks_exact(2).all(|texel| texel[1] == expected))
            );
        }
        let mips = density_mips(&source, 128);
        let base_mean = mips[0]
            .chunks_exact(2)
            .map(|texel| f64::from(texel[1]))
            .sum::<f64>()
            / f64::from(CLOUD_FIELD_TEXTURE_SIZE).powi(3);
        assert!((base_mean - f64::from(mips[6][1])).abs() <= 1.0);
    }

    fn uniform_fixture(layer: CloudLayer, projection: Mat4) -> Option<CloudUniform> {
        uniform_fixture_weather(layer, projection, RenderWeather::default())
    }
    fn uniform_fixture_weather(
        layer: CloudLayer,
        projection: Mat4,
        weather: RenderWeather,
    ) -> Option<CloudUniform> {
        let origin = RenderOrigin::new(flightsim_core::Geodetic::from_degrees(35.0, 139.0, 1500.0));
        let inputs = CloudInputs {
            quality: CloudQuality::High,
            layer: ResolvedCloudLayer {
                layer,
                modeled: matches!(weather.selection, WeatherSelection::Modeled(_)),
            },
            weather,
            origin: Some(origin),
            sun: SunDirection::default(),
            clock: TimeOfDay::default(),
            decks: vec![],
            fog_cameras: vec![],
        };
        let view = ExtractedView {
            retained_view_entity: bevy::render::view::RetainedViewEntity::new(
                Entity::PLACEHOLDER.into(),
                None,
                0,
            ),
            clip_from_view: projection,
            world_from_view: GlobalTransform::from_translation(Vec3::new(0.0, 1500.0, 0.0)),
            clip_from_world: None,
            hdr: true,
            viewport: UVec4::new(0, 0, 1280, 720),
            color_grading: default(),
            invert_culling: false,
        };
        cloud_uniform(&inputs, &origin, &view, 0.000025, UVec2::new(640, 360))
    }

    #[test]
    fn authored_negative_layers_are_not_hidden_by_the_synthetic_sea_level_blocker() {
        use flightsim_sim::weather::{WeatherPreset, WeatherScenario};
        let mut p = WeatherScenario::from_preset(
            WeatherPreset::Rain,
            flightsim_core::Geodetic::from_degrees(35.0, 139.0, 0.0),
            7,
        )
        .unwrap()
        .parameters();
        p.preset = WeatherPreset::Custom;
        let cloud = p.cloud.as_mut().unwrap();
        cloud.base = flightsim_core::Meters(-900.0);
        cloud.top = flightsim_core::Meters(-500.0);
        let layer = CloudLayer {
            cover: 1.0,
            base: cloud.base,
            top: cloud.top,
            visibility: cloud.visibility,
            seed: p.seed,
        };
        let scenario = WeatherScenario::try_from(p).unwrap();
        let uniform = uniform_fixture_weather(
            layer,
            Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1),
            RenderWeather {
                selection: WeatherSelection::Modeled(scenario),
                elapsed: Seconds::ZERO,
            },
        )
        .unwrap();
        assert!(
            uniform.intersections.x > uniform.intersections.y,
            "earth blocker must lie below cloud base"
        );
        assert_eq!(uniform.seed.y, 1);
        assert!(
            (uniform.layer.w - crate::modeled_weather::modeled_extinction(layer.visibility)).abs()
                < 1e-7
        );
    }

    #[test]
    fn impossible_gpu_ranges_fall_back_before_baseline_suppression() {
        let projection = Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1);
        let layer = CloudLayer::try_new(
            0.6,
            flightsim_core::Meters(1000.0),
            flightsim_core::Meters(2000.0),
            flightsim_core::Meters(300.0),
            1,
        )
        .expect("valid weather");
        assert!(uniform_fixture(layer, projection).is_some());
        assert!(
            uniform_fixture(
                CloudLayer {
                    top: flightsim_core::Meters(f64::MAX),
                    ..layer
                },
                projection
            )
            .is_none()
        );
        assert!(
            uniform_fixture(
                CloudLayer {
                    visibility: flightsim_core::Meters(f64::MIN_POSITIVE),
                    ..layer
                },
                projection
            )
            .is_none()
        );
        assert!(uniform_fixture(layer, Mat4::ZERO).is_none());
        assert!(uniform_fixture(layer, Mat4::IDENTITY).is_none());
    }

    #[test]
    fn target_limits_hold_for_wide_tall_and_high_dpi_views() {
        for quality in [CloudQuality::High, CloudQuality::Ultra] {
            let (w, h, _, _) = quality.limits();
            for viewport in [
                UVec2::new(1, 1),
                UVec2::new(3840, 2160),
                UVec2::new(7680, 100),
                UVec2::new(100, 7680),
                UVec2::new(2732, 2048),
            ] {
                let size = bounded_target(viewport, quality);
                assert!(
                    size.x <= w
                        && size.y <= h
                        && size.x <= viewport.x
                        && size.y <= viewport.y
                        && size.min_element() > 0
                );
            }
        }
        assert_eq!(
            bounded_target(UVec2::new(800, 600), CloudQuality::Light),
            UVec2::ZERO
        );
        assert_eq!(
            bounded_target(UVec2::new(0, 600), CloudQuality::Ultra),
            UVec2::ZERO
        );
    }
}
