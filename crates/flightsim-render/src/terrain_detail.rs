//! Original, offline, world-anchored surface shading. This is illustrative
//! material variation, not imagery, land-cover data, vegetation or geometry.
//!
//! Canonical positions remain f64 ECEF in `core`/`world`. The CPU reduces the
//! origin independently into each periodic noise lattice before conversion to
//! f32. The shader rotates the existing relative render position into ECEF
//! axes. Neither tile UVs, LOD IDs nor the current origin seed the pattern.

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use flightsim_core::RenderFrame;

use crate::{RenderOrigin, RenderSet};

const SHADER_HANDLE: Handle<Shader> = uuid_handle!("74d47bf6-a823-48c8-aa7c-09ae7ba2f027");
const LATTICE_PERIOD: f64 = 256.0;
const CELL_METERS: [f64; 4] = [2048.0, 256.0, 32.0, 4.0];

/// The terrain-only material. Airports and other solid surfaces can continue
/// to use [`StandardMaterial`] without this shader.
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainDetail>;

/// Render-only control; never recorded into or applied to flight dynamics.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceDetailSettings {
    pub enabled: bool,
}

impl Default for SurfaceDetailSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Register once after Bevy's default rendering plugins. Shader source is
/// embedded in the executable, so offline packaging needs no loose texture or
/// shader files. Uniforms update in the same frame as an origin rebase.
#[derive(Debug, Default)]
pub struct TerrainDetailPlugin;

impl Plugin for TerrainDetailPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER_HANDLE, "terrain_detail.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .init_resource::<SurfaceDetailSettings>()
            .add_systems(
                Update,
                sync_terrain_detail
                    .after(RenderSet::Rebase)
                    .before(RenderSet::Terrain),
            );
    }
}

/// Material extension with bounded, aligned per-frame coordinate uniforms.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainDetail {
    #[uniform(100)]
    uniform: TerrainDetailUniform,
}

impl MaterialExtension for TerrainDetail {
    fn fragment_shader() -> ShaderRef {
        SHADER_HANDLE.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER_HANDLE.into()
    }
}

// vec4 fields deliberately keep the CPU/WGSL alignment explicit. Basis xyz
// columns are ECEF axes expressed in render space, so dot(column, relative)
// recovers the ECEF displacement without a second coordinate implementation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Reflect, ShaderType)]
pub(crate) struct TerrainDetailUniform {
    ecef_x: Vec4,
    ecef_y: Vec4,
    ecef_z: Vec4,
    macro_phase: Vec4,
    patch_phase: Vec4,
    grain_phase: Vec4,
    fine_phase: Vec4,
    settings: Vec4,
}

impl TerrainDetailUniform {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "reciprocal of four fixed power-of-two metre scales is exactly representable"
    )]
    pub(crate) fn for_frame(frame: &RenderFrame, enabled: bool) -> Self {
        let origin = frame.anchor().to_ecef().as_vec();
        let ecef_x = frame.vector_to_render(glam::DVec3::X).extend(0.0);
        let ecef_y = frame.vector_to_render(glam::DVec3::Y).extend(0.0);
        let ecef_z = frame.vector_to_render(glam::DVec3::Z).extend(0.0);
        if !origin.is_finite() || !ecef_x.is_finite() || !ecef_y.is_finite() || !ecef_z.is_finite()
        {
            // Invalid app state must not upload NaN uniforms. This render-only
            // fallback disables detail; the app remains responsible for rejecting
            // invalid canonical positions before geometry/physics processing.
            return Self::default();
        }
        let phase = |cell_meters: f64| {
            // Reduce in f64 BEFORE narrowing. A giant ECEF f32 offset loses the
            // very near-ground precision this material is intended to show.
            let periodic = (origin / cell_meters).rem_euclid(glam::DVec3::splat(LATTICE_PERIOD));
            periodic.as_vec3().extend((1.0 / cell_meters) as f32)
        };
        Self {
            ecef_x,
            ecef_y,
            ecef_z,
            macro_phase: phase(CELL_METERS[0]),
            patch_phase: phase(CELL_METERS[1]),
            grain_phase: phase(CELL_METERS[2]),
            fine_phase: phase(CELL_METERS[3]),
            settings: Vec4::new(f32::from(enabled), 0.0, 0.0, 0.0),
        }
    }
}

/// Keep the existing linear vertex palette as the base; detail is added only
/// by the fragment shader. Geometry, normals and all contact data are untouched.
#[must_use]
pub fn default_surface_detail_material() -> TerrainMaterial {
    TerrainMaterial {
        base: crate::default_terrain_material(),
        extension: TerrainDetail::default(),
    }
}

fn sync_terrain_detail(
    origin: Res<RenderOrigin>,
    settings: Res<SurfaceDetailSettings>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let uniform = TerrainDetailUniform::for_frame(&origin.0, settings.enabled);
    // Also initialize a material added after startup without requiring a rebase.
    // Avoid changing Assets on every frame when its uniforms already match.
    let changed: Vec<_> = materials
        .iter()
        .filter_map(|(id, material)| (material.extension.uniform != uniform).then_some(id))
        .collect();
    for id in changed {
        if let Some(material) = materials.get_mut(id) {
            material.extension.uniform = uniform;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flightsim_core::{Ecef, Geodetic, LocalFrame, Ned};

    const SHADER_SOURCE: &str = include_str!("terrain_detail.wgsl");

    fn phases(uniform: TerrainDetailUniform) -> [Vec4; 4] {
        [
            uniform.macro_phase,
            uniform.patch_phase,
            uniform.grain_phase,
            uniform.fine_phase,
        ]
    }

    fn shader_lattice(uniform: TerrainDetailUniform, position: Vec3, band: usize) -> Vec3 {
        let relative = Vec3::new(
            uniform.ecef_x.truncate().dot(position),
            uniform.ecef_y.truncate().dot(position),
            uniform.ecef_z.truncate().dot(position),
        );
        let phase = phases(uniform)[band];
        relative * phase.w + phase.truncate()
    }

    fn periodic_error(a: glam::DVec3, b: glam::DVec3) -> f64 {
        let error = (a - b).rem_euclid(glam::DVec3::splat(LATTICE_PERIOD));
        error
            .min(glam::DVec3::splat(LATTICE_PERIOD) - error)
            .max_element()
    }

    fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
        let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    // CPU reference for the explicitly documented WGSL filter contract. Actual
    // shader compilation and visual aliasing remain runtime acceptance gates.
    fn band_weight(footprint: f32) -> f32 {
        1.0 - smoothstep(0.12, 0.48, footprint)
    }

    fn palette_strength(base: [f32; 4]) -> f32 {
        smoothstep(0.0, 0.035, base[1] - base[2])
            * (1.0
                - smoothstep(
                    0.40,
                    0.72,
                    base[0] * 0.2126 + base[1] * 0.7152 + base[2] * 0.0722,
                ))
    }

    // Reference implementation of the fixed integer/hash interpolation contract.
    // The production shader, not this helper, performs rendering.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "test coordinates are bounded lattice values; hash output is masked to 16 bits"
    )]
    fn reference_noise(position: Vec3) -> f32 {
        let cell = position.floor().as_ivec3();
        let f = position - position.floor();
        let t = f * f * f * (f * (f * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));
        let hash = |offset: IVec3| {
            let wrapped = (cell + offset).as_uvec3() & UVec3::splat(255);
            let mut h = wrapped
                .x
                .wrapping_mul(1_597_334_677)
                .wrapping_add(wrapped.y.wrapping_mul(3_812_015_801))
                .wrapping_add(wrapped.z.wrapping_mul(2_798_796_415));
            h = (h ^ (h >> 16)).wrapping_mul(2_246_822_519);
            h = (h ^ (h >> 13)).wrapping_mul(3_266_489_917);
            h ^= h >> 16;
            f32::from((h & 65_535) as u16) * (2.0 / 65_535.0) - 1.0
        };
        let mix = |a: f32, b: f32, amount: f32| a + (b - a) * amount;
        let x00 = mix(hash(IVec3::ZERO), hash(IVec3::X), t.x);
        let x10 = mix(hash(IVec3::Y), hash(IVec3::new(1, 1, 0)), t.x);
        let x01 = mix(hash(IVec3::Z), hash(IVec3::new(1, 0, 1)), t.x);
        let x11 = mix(hash(IVec3::new(0, 1, 1)), hash(IVec3::ONE), t.x);
        mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z)
    }

    #[test]
    fn noise_contract_is_deterministic_bounded_and_continuous_across_periods() {
        for position in [
            Vec3::ZERO,
            Vec3::new(1.25, -71.75, 253.25),
            Vec3::new(-255.875, 500.5, -700.75),
        ] {
            let a = reference_noise(position);
            assert!((-1.0..=1.0).contains(&a));
            assert_eq!(a.to_bits(), reference_noise(position).to_bits());
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                let repeated = reference_noise(position + axis * 256.0);
                assert!((a - repeated).abs() < 1.0e-6);
            }
        }
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            for center in [Vec3::ZERO, Vec3::splat(256.0)] {
                let below = reference_noise(center - axis * 0.001);
                let above = reference_noise(center + axis * 0.001);
                assert!((below - above).abs() < 1.0e-5);
            }
        }
        // Independently calculated fixed integer lattice fixture (zero hash).
        assert!((reference_noise(Vec3::ZERO) + 1.0).abs() < f32::EPSILON);
        for multiplier in [
            "1597334677u",
            "3812015801u",
            "2798796415u",
            "2246822519u",
            "3266489917u",
        ] {
            assert!(SHADER_SOURCE.contains(multiplier));
        }
    }

    #[test]
    fn actual_wgsl_noise_helpers_parse_and_validate_without_a_gpu() {
        let start = SHADER_SOURCE.find("fn lattice_value(").unwrap();
        let end = SHADER_SOURCE.find("@fragment").unwrap();
        let helpers = &SHADER_SOURCE[start..end];
        let module = naga::front::wgsl::parse_str(helpers).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(module.functions.len(), 3);
    }

    #[test]
    fn invalid_origin_disables_detail_instead_of_uploading_nan_uniforms() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let frame = RenderFrame::new(Geodetic::from_degrees(value, value, 0.0));
            assert_eq!(
                TerrainDetailUniform::for_frame(&frame, true),
                TerrainDetailUniform::default()
            );
        }
    }

    #[test]
    fn uniform_is_finite_and_bounded_at_dateline_and_poles() {
        assert_eq!(TerrainDetailUniform::min_size().get(), 128);
        for latitude in [-90.0, -89.9999, -45.0, 0.0, 35.5, 89.9999, 90.0] {
            for longitude in [-180.0, -179.9999, 0.0, 139.8, 179.9999, 180.0] {
                let frame = RenderFrame::new(Geodetic::from_degrees(latitude, longitude, 1234.0));
                let uniform = TerrainDetailUniform::for_frame(&frame, true);
                let basis = Mat3::from_cols(
                    uniform.ecef_x.truncate(),
                    uniform.ecef_y.truncate(),
                    uniform.ecef_z.truncate(),
                );
                assert!(basis.is_finite());
                assert!((basis.determinant() - 1.0).abs() < 1.0e-6);
                for phase in phases(uniform) {
                    assert!(phase.is_finite());
                    assert!(phase.truncate().min_element() >= 0.0);
                    assert!(phase.truncate().max_element() <= 256.0);
                    assert!((1.0 / 2048.0..=0.25).contains(&phase.w));
                }
            }
        }
    }

    #[test]
    fn shader_coordinates_reconstruct_independent_f64_ecef_reference() {
        for (latitude, longitude) in [(35.5, 139.8), (0.0, 180.0), (90.0, 75.0), (-90.0, -95.0)] {
            let anchor = Geodetic::from_degrees(latitude, longitude, 0.0);
            let frame = RenderFrame::new(anchor);
            let uniform = TerrainDetailUniform::for_frame(&frame, true);
            let local = LocalFrame::new(anchor);
            for offset in [Ned::new(0.0, 0.0, 0.0), Ned::new(3990.0, -2500.0, -1700.0)] {
                let world = local.ned_to_ecef_position(offset);
                for (band, cell_meters) in CELL_METERS.iter().enumerate() {
                    let actual = shader_lattice(uniform, frame.to_render(world), band).as_dvec3();
                    let expected = world.as_vec() / *cell_meters;
                    let error_meters = periodic_error(actual, expected) * cell_meters;
                    assert!(error_meters < 0.022, "band {band}: {error_meters} m");
                }
            }
        }
    }

    #[test]
    fn rebase_preserves_texture_location_without_f32_world_truth() {
        for (latitude, longitude) in [
            (35.5, 139.8),
            (0.0, 179.999),
            (89.99, 65.0),
            (-89.99, -95.0),
        ] {
            let anchor = Geodetic::from_degrees(latitude, longitude, 0.0);
            let local = LocalFrame::new(anchor);
            let first = RenderFrame::new(anchor);
            let moved = local.ned_to_ecef_position(Ned::new(6000.0, -3000.0, 0.0));
            let mut second = first;
            assert!(second.rebase_if_needed(moved.to_geodetic()));
            let before = TerrainDetailUniform::for_frame(&first, true);
            let after = TerrainDetailUniform::for_frame(&second, true);
            for offset in [Ned::new(0.0, 0.0, 0.0), Ned::new(3000.0, -2500.0, -500.0)] {
                let point = local.ned_to_ecef_position(offset);
                for (band, cell_meters) in CELL_METERS.iter().enumerate() {
                    let a = shader_lattice(before, first.to_render(point), band).as_dvec3();
                    let b = shader_lattice(after, second.to_render(point), band).as_dvec3();
                    let error_meters = periodic_error(a, b) * cell_meters;
                    // The largest band deliberately stores a coarse fractional
                    // phase: 2048 m * one f32 ULP at 256 = 3.125 cm. Fine bands
                    // do not inherit this phase quantization.
                    assert!(error_meters < 0.040, "band {band}: {error_meters} m");
                    if band == 3 {
                        assert!(error_meters < 0.0025, "fine band moved {error_meters} m");
                    }
                }
            }
        }
    }

    #[test]
    fn equivalent_dateline_and_polar_frames_share_the_same_field() {
        for (a, b) in [
            ((0.0, 180.0), (0.0, -180.0)),
            ((90.0, 0.0), (90.0, 137.0)),
            ((-90.0, 0.0), (-90.0, -92.0)),
        ] {
            let first = RenderFrame::new(Geodetic::from_degrees(a.0, a.1, 0.0));
            let second = RenderFrame::new(Geodetic::from_degrees(b.0, b.1, 0.0));
            let point =
                Ecef(first.anchor().to_ecef().as_vec() + glam::DVec3::new(251.3, -42.7, 91.5));
            for band in 0..CELL_METERS.len() {
                let x = shader_lattice(
                    TerrainDetailUniform::for_frame(&first, true),
                    first.to_render(point),
                    band,
                );
                let y = shader_lattice(
                    TerrainDetailUniform::for_frame(&second, true),
                    second.to_render(point),
                    band,
                );
                assert!(periodic_error(x.as_dvec3(), y.as_dvec3()) < 0.0001);
            }
        }
    }

    #[test]
    fn filter_fades_before_subpixel_aliasing_and_never_inverts() {
        let mut previous = 1.0;
        for hundredths in 0_u16..=200 {
            let weight = band_weight(f32::from(hundredths) / 100.0);
            assert!((0.0..=1.0).contains(&weight));
            assert!(weight <= previous);
            previous = weight;
        }
        assert!(band_weight(0.1) > 0.999);
        assert!(band_weight(0.48) < f32::EPSILON);
        assert!(band_weight(1000.0) < f32::EPSILON);
        assert!(SHADER_SOURCE.contains("smoothstep(0.12, 0.48, footprint)"));
        assert!(SHADER_SOURCE.contains("sqrt(dot(dx, dx) + dot(dy, dy))"));
    }

    #[test]
    fn authored_water_and_snow_are_suppressed_but_below_sea_level_land_is_not() {
        use crate::biome::{SurfaceAppearance, linear_surface_color};
        use flightsim_core::{Kelvin, Meters, MetersPerSecond, Radians};
        let land = SurfaceAppearance {
            land_fraction: 1.0,
            elevation_msl: Meters(-400.0),
            slope: Radians::ZERO,
            temperature: Kelvin(298.15),
            precipitation: MetersPerSecond(0.003 / 86400.0),
            snow_fraction: 0.0,
        };
        assert!(palette_strength(linear_surface_color(land)) > 0.95);
        assert!(
            palette_strength(linear_surface_color(SurfaceAppearance {
                land_fraction: 0.0,
                ..land
            })) < f32::EPSILON
        );
        assert!(
            palette_strength(linear_surface_color(SurfaceAppearance {
                snow_fraction: 1.0,
                ..land
            })) < f32::EPSILON
        );
    }

    #[test]
    fn fixed_embedded_shader_has_bounded_cost_and_changes_no_geometry() {
        assert_eq!(SHADER_SOURCE.matches("* value_noise(").count(), 4);
        assert!(!SHADER_SOURCE.contains("textureSample"));
        assert!(!SHADER_SOURCE.contains("@vertex"));
        assert!(!SHADER_SOURCE.contains("pbr_input.N ="));
        assert!(!SHADER_SOURCE.contains("sin("));
        assert!(!SHADER_SOURCE.contains("loop {"));
        assert!(SHADER_SOURCE.contains("clamp(pattern, -0.25, 0.25)"));
        assert!(SHADER_SOURCE.contains("if strength > 0.0"));
        let frame = RenderFrame::new(Geodetic::from_degrees(35.5, 139.8, 0.0));
        let disabled = TerrainDetailUniform::for_frame(&frame, false);
        assert!(disabled.settings.x.abs() < f32::EPSILON);
        assert_eq!(disabled, TerrainDetailUniform::for_frame(&frame, false));
        let material = default_surface_detail_material();
        assert_eq!(material.base.base_color, Color::WHITE);
        assert!((material.base.perceptual_roughness - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn actual_surface_and_bridge_transforms_bound_rebase_and_attachment_variation() {
        use flightsim_core::Meters;
        use flightsim_world::{
            DemTile, HeightGrid, TerrainBoundary, TileId, build_mesh, plan_seams,
        };
        use std::collections::BTreeMap;

        let pattern = |uniform: TerrainDetailUniform, rendered: Vec3| {
            [0.26, 0.18, 0.10, 0.035]
                .into_iter()
                .enumerate()
                .map(|(band, weight)| {
                    weight * reference_noise(shader_lattice(uniform, rendered, band))
                })
                .sum::<f32>()
                .clamp(-0.25, 0.25)
        };
        let fixtures = [
            (
                TileId::new(3, 1, 4),
                TileId::new(7, 32, 68),
                -5.9765625,
                -133.5,
            ),
            (TileId::new(3, 0, 4), TileId::new(7, 255, 68), -5.9, 179.999),
            (TileId::new(3, 0, 0), TileId::new(7, 16, 0), 89.99, -156.0),
            (
                TileId::new(3, 0, 7),
                TileId::new(7, 16, 127),
                -89.99,
                -156.0,
            ),
            (TileId::new(0, 0, 0), TileId::new(4, 16, 8), -1.0, 0.01),
        ];
        for (first_id, second_id, latitude, longitude) in fixtures {
            let surfaces: Vec<_> = [first_id, second_id]
                .into_iter()
                .map(|id| {
                    let dem = DemTile::new(id.bounds(), HeightGrid::flat(33, 33, Meters(100.0)));
                    (id, build_mesh(id, &dem, &crate::mesh_options_for(id.level)))
                })
                .collect();
            let boundaries: BTreeMap<_, _> = surfaces
                .iter()
                .map(|(id, mesh)| (*id, TerrainBoundary::from_mesh(*id, mesh)))
                .collect();
            let bridges: Vec<_> = plan_seams(&boundaries, &boundaries.keys().copied().collect())
                .into_iter()
                .map(|seam| seam.build_mesh(&boundaries))
                .collect();
            assert!(!bridges.is_empty());
            let anchor = Geodetic::from_degrees(latitude, longitude, 1000.0);
            let before = RenderFrame::new(anchor);
            let after = RenderFrame::new(
                LocalFrame::new(anchor)
                    .ned_to_ecef_position(Ned::new(6000.0, -3000.0, 0.0))
                    .to_geodetic(),
            );
            let first_uniform = TerrainDetailUniform::for_frame(&before, true);
            let next_uniform = TerrainDetailUniform::for_frame(&after, true);
            let transformed =
                |mesh: &flightsim_world::TerrainMesh, frame: &RenderFrame, vertex: [f32; 3]| {
                    // Mirrors Transform -> GlobalTransform -> GPU world matrix,
                    // including the tile-relative f32 vertex and f32 origin/rotation.
                    GlobalTransform::from(Transform {
                        translation: frame.to_render(mesh.origin),
                        rotation: frame.rotation_to_render(glam::DQuat::IDENTITY),
                        ..default()
                    })
                    .affine()
                    .transform_point3(Vec3::from_array(vertex))
                };
            let mut max_rebase_variation = 0.0_f32;
            for mesh in surfaces.iter().map(|(_, mesh)| mesh).chain(&bridges) {
                for &vertex in &mesh.positions {
                    let a = pattern(first_uniform, transformed(mesh, &before, vertex));
                    let b = pattern(next_uniform, transformed(mesh, &after, vertex));
                    assert!(a.is_finite() && b.is_finite());
                    max_rebase_variation = max_rebase_variation.max((a - b).abs());
                }
            }
            let source_points: Vec<_> = surfaces
                .iter()
                .flat_map(|(_, mesh)| {
                    mesh.positions[..mesh.surface_vertex_count]
                        .iter()
                        .map(|&vertex| {
                            (
                                mesh.origin.as_vec() + Vec3::from_array(vertex).as_dvec3(),
                                pattern(first_uniform, transformed(mesh, &before, vertex)),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let mut matched = 0;
            let mut max_attachment_variation = 0.0_f32;
            for bridge in &bridges {
                for &vertex in &bridge.positions {
                    let world = bridge.origin.as_vec() + Vec3::from_array(vertex).as_dvec3();
                    let (source, source_pattern) = source_points
                        .iter()
                        .min_by(|a, b| {
                            a.0.distance_squared(world)
                                .total_cmp(&b.0.distance_squared(world))
                        })
                        .unwrap();
                    // Only actual shared source knots; intermediate ribbon and
                    // corner vertices need not match a surface vertex.
                    if source.distance(world) < 0.8 {
                        let bridge_pattern =
                            pattern(first_uniform, transformed(bridge, &before, vertex));
                        max_attachment_variation =
                            max_attachment_variation.max((bridge_pattern - source_pattern).abs());
                        matched += 1;
                    }
                }
            }
            assert!(matched >= 2);
            // This deliberately leaves ALL bands enabled, even on far-side
            // root vertices that derivative filtering normally makes featureless.
            // Bound the actual albedo multiplier, not fictitious exact welding.
            let limit = if first_id.level == 0 { 0.10 } else { 0.04 };
            assert!(
                max_rebase_variation < limit,
                "{first_id:?}: rebase {max_rebase_variation}"
            );
            assert!(
                max_attachment_variation < limit,
                "{first_id:?}: seam {max_attachment_variation}"
            );
            eprintln!(
                "{first_id:?}/{second_id:?}: unfiltered albedo-multiplier delta rebase={max_rebase_variation:.6}, source/bridge={max_attachment_variation:.6}, matched_knots={matched}"
            );
        }
    }

    #[test]
    fn surfaces_and_stitched_bridges_use_the_same_extended_material_type() {
        use crate::terrain::{TerrainTile, prepare_tile};
        use crate::terrain_stitching::{
            TerrainBridge, advance_stitched_update, apply_stitched_update,
        };
        use crate::{TerrainTiles, TerrainUpdate};
        use flightsim_core::Meters;
        use flightsim_world::{DemTile, HeightGrid, TileId};

        fn step(
            mut commands: Commands,
            mut meshes: ResMut<Assets<Mesh>>,
            mut tiles: ResMut<TerrainTiles>,
            mut started: Local<bool>,
        ) {
            let frame = RenderFrame::new(Geodetic::from_degrees(0.0, 0.0, 0.0));
            if *started {
                if tiles.is_stitching() {
                    advance_stitched_update::<TerrainMaterial>(
                        &mut commands,
                        &mut meshes,
                        &mut tiles,
                        Handle::default(),
                        &frame,
                        4,
                        None,
                    );
                }
                return;
            }
            *started = true;
            let ids = [TileId::new(4, 15, 8), TileId::new(4, 16, 8)];
            for id in ids {
                let dem = DemTile::new(id.bounds(), HeightGrid::flat(3, 3, Meters(20.0)));
                let prepared = prepare_tile::<TerrainMaterial>(
                    &mut commands,
                    &mut meshes,
                    Handle::default(),
                    &frame,
                    id,
                    &dem,
                    None,
                );
                tiles.insert_prepared(prepared);
            }
            apply_stitched_update::<TerrainMaterial>(
                &mut commands,
                &mut meshes,
                &mut tiles,
                Handle::default(),
                &frame,
                TerrainUpdate {
                    prepared: ids.to_vec(),
                    spawned: ids.to_vec(),
                    ..default()
                },
                4,
                None,
            );
        }

        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<TerrainTiles>()
            .add_systems(Update, step);
        for _ in 0..8 {
            app.update();
            if !app.world().resource::<TerrainTiles>().is_stitching() {
                break;
            }
        }
        assert!(!app.world().resource::<TerrainTiles>().is_stitching());
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<&MeshMaterial3d<TerrainMaterial>, With<TerrainTile>>()
                .iter(world)
                .count(),
            2
        );
        assert!(
            world
                .query_filtered::<&MeshMaterial3d<TerrainMaterial>, With<TerrainBridge>>()
                .iter(world)
                .count()
                >= 1
        );
        assert_eq!(
            world
                .query::<&MeshMaterial3d<StandardMaterial>>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn sync_initializes_late_materials_and_applies_settings_and_rebase() {
        let mut app = App::new();
        app.init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<SurfaceDetailSettings>()
            .insert_resource(RenderOrigin::new(Geodetic::from_degrees(35.5, 139.8, 0.0)))
            .add_systems(Update, sync_terrain_detail);
        app.update();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<TerrainMaterial>>()
            .add(default_surface_detail_material());
        app.update();
        let first = app
            .world()
            .resource::<Assets<TerrainMaterial>>()
            .get(&handle)
            .unwrap()
            .extension
            .uniform;
        assert!((first.settings.x - 1.0).abs() < f32::EPSILON);
        app.world_mut()
            .resource_mut::<SurfaceDetailSettings>()
            .enabled = false;
        app.world_mut()
            .insert_resource(RenderOrigin::new(Geodetic::from_degrees(-90.0, 60.0, 0.0)));
        app.update();
        let next = app
            .world()
            .resource::<Assets<TerrainMaterial>>()
            .get(&handle)
            .unwrap()
            .extension
            .uniform;
        assert!(next.settings.x.abs() < f32::EPSILON);
        assert_ne!(first.ecef_z, next.ecef_z);
        assert_eq!(app.world().resource::<Assets<TerrainMaterial>>().len(), 1);
    }
}
