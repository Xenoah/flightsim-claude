// Original procedural material detail, authored for FlightSim. No sampled
// imagery or external texture assets. Does not alter vertices or normals.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    decal::clustered::apply_decals,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

struct TerrainDetailUniform {
    ecef_x: vec4<f32>,
    ecef_y: vec4<f32>,
    ecef_z: vec4<f32>,
    macro_phase: vec4<f32>,
    patch_phase: vec4<f32>,
    grain_phase: vec4<f32>,
    fine_phase: vec4<f32>,
    settings: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> detail: TerrainDetailUniform;

// Exact unsigned arithmetic avoids sin/fract hashes and unstable large angles.
// Each lattice is independently periodic in ECEF xyz; no longitude or pole
// singularity, tile UV boundary or floating-origin boundary enters the hash.
fn lattice_value(cell: vec3<i32>) -> f32 {
    let wrapped = vec3<u32>(cell & vec3<i32>(255));
    var h = wrapped.x * 1597334677u + wrapped.y * 3812015801u + wrapped.z * 2798796415u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    h = h ^ (h >> 16u);
    return f32(h & 65535u) * (2.0 / 65535.0) - 1.0;
}

fn value_noise(p: vec3<f32>) -> f32 {
    let cell = vec3<i32>(floor(p));
    let f = fract(p);
    // Quintic interpolation has zero first/second derivatives at each cell
    // boundary; wrapping the hash gives a continuous periodic volume.
    let t = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(lattice_value(cell), lattice_value(cell + vec3<i32>(1, 0, 0)), t.x);
    let x10 = mix(lattice_value(cell + vec3<i32>(0, 1, 0)), lattice_value(cell + vec3<i32>(1, 1, 0)), t.x);
    let x01 = mix(lattice_value(cell + vec3<i32>(0, 0, 1)), lattice_value(cell + vec3<i32>(1, 0, 1)), t.x);
    let x11 = mix(lattice_value(cell + vec3<i32>(0, 1, 1)), lattice_value(cell + vec3<i32>(1, 1, 1)), t.x);
    return mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z);
}

fn band_weight(footprint: f32) -> f32 {
    // Stop the band before a pixel spans half a cell. Fade in advance instead
    // of trying to preserve detail as sub-pixel stipple at altitude/horizon.
    return 1.0 - smoothstep(0.12, 0.48, footprint);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let relative = in.world_position.xyz;
    let ecef_relative = vec3<f32>(
        dot(detail.ecef_x.xyz, relative),
        dot(detail.ecef_y.xyz, relative),
        dot(detail.ecef_z.xyz, relative),
    );
    // Derivatives are evaluated unconditionally, outside potentially varying
    // control flow. An isotropic conservative footprint avoids grazing aliases.
    let dx = dpdx(ecef_relative);
    let dy = dpdy(ecef_relative);
    // Frobenius norm bounds the largest footprint direction, including a
    // sheared/diagonal pixel. max(length(dx), length(dy)) can underestimate it.
    let footprint_m = sqrt(dot(dx, dx) + dot(dy, dy));
    let macro_weight = band_weight(footprint_m * detail.macro_phase.w);
    let patch_weight = band_weight(footprint_m * detail.patch_phase.w);
    let grain_weight = band_weight(footprint_m * detail.grain_phase.w);
    let fine_weight = band_weight(footprint_m * detail.fine_phase.w);

    let base = pbr_input.material.base_color.rgb;
    // A conservative palette cue, NOT a geographic land mask. Current water
    // is blue-dominant; current snow is bright. Both fade smoothly toward the
    // unmodified base palette, including mixed coastline/snow vertices.
    let land_cue = smoothstep(0.0, 0.035, base.g - base.b);
    let snow_cue = 1.0 - smoothstep(0.40, 0.72, dot(base, vec3<f32>(0.2126, 0.7152, 0.0722)));
    let strength = detail.settings.x * land_cue * snow_cue;
    var pattern = 0.0;
    if strength > 0.0 {
        // Exactly four bands, no iteration count, clock, random seed, texture
        // allocation or per-tile state. Skip wholly unresolved bands.
        if macro_weight > 0.0 {
            pattern += 0.26 * macro_weight * value_noise(ecef_relative * detail.macro_phase.w + detail.macro_phase.xyz);
        }
        if patch_weight > 0.0 {
            pattern += 0.18 * patch_weight * value_noise(ecef_relative * detail.patch_phase.w + detail.patch_phase.xyz);
        }
        if grain_weight > 0.0 {
            pattern += 0.10 * grain_weight * value_noise(ecef_relative * detail.grain_phase.w + detail.grain_phase.xyz);
        }
        if fine_weight > 0.0 {
            pattern += 0.035 * fine_weight * value_noise(ecef_relative * detail.fine_phase.w + detail.fine_phase.xyz);
        }
    }
    let variation = clamp(pattern, -0.25, 0.25) * strength;
    if strength > 0.0 {
        pbr_input.material.base_color = vec4<f32>(clamp(base * (1.0 + variation), vec3<f32>(0.0), vec3<f32>(1.0)), pbr_input.material.base_color.a);
        pbr_input.material.perceptual_roughness = clamp(pbr_input.material.perceptual_roughness - variation * 0.32, 0.72, 1.0);
    }
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    apply_decals(&pbr_input);
#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}
