// Original procedural ocean material. Geographic RG8 mask comes from the bundled atlas.
// No displacement, external imagery, SSR, shore surf or physical wave coupling.
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


#import bevy_pbr::mesh_view_bindings::view

struct WaterUniform {
    ecef_x: vec4<f32>,
    ecef_y: vec4<f32>,
    ecef_z: vec4<f32>,
    origin: vec4<f32>,
    waves: array<vec4<f32>, 8>,
    sun: vec4<f32>,
    settings: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> water: WaterUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var water_mask: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var water_sampler: sampler;

const SLOPES: array<f32, 8> = array<f32, 8>(0.055, 0.080, 0.065, 0.090, 0.095, 0.105, 0.060, 0.045);
const PI: f32 = 3.14159265359;

fn water_band_weight(cycles_per_pixel: f32) -> f32 {
    return 1.0 - smoothstep(0.10, 0.45, cycles_per_pixel);
}

fn water_fresnel(cosine: f32) -> f32 {
    // Air / water IOR 1.333 gives F0 approximately 0.02037.
    return 0.02037 + (1.0 - 0.02037) * pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
}

fn analytic_sky(reflected: vec3<f32>, up: vec3<f32>, sun: vec3<f32>, cover: f32) -> vec3<f32> {
    let sun_height = dot(sun, up);
    let daylight = smoothstep(-0.10, 0.12, sun_height);
    let sunset = (1.0 - smoothstep(0.04, 0.40, sun_height)) * daylight;
    let elevation = clamp(dot(reflected, up), 0.0, 1.0);
    let horizon = pow(1.0 - elevation, 3.0);
    let sunward = pow(max(dot(reflected, sun), 0.0), 6.0);
    let zenith = vec3<f32>(0.17, 0.36, 0.68);
    let horizon_color = mix(vec3<f32>(0.65, 0.79, 0.90), vec3<f32>(1.0, 0.32, 0.10), sunset * sunward);
    let clear = mix(zenith, horizon_color, horizon);
    let overcast = vec3<f32>(0.55, 0.60, 0.65) * (0.70 + 0.30 * elevation);
    // Approximate sky radiance in cd/m², then the common camera exposure applies.
    // Cloud cover drives sky response even with Graphics Light and Clouds Off.
    return mix(clear * 6200.0, overcast * 3100.0, cover) * daylight + vec3<f32>(0.02, 0.025, 0.04);
}

fn sun_glitter(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, up: vec3<f32>, alpha2: f32, cover: f32) -> vec3<f32> {
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 0.001);
    let h_sum = v + l;
    let h = h_sum * inverseSqrt(max(dot(h_sum,h_sum), 0.000001));
    let nh = max(dot(n,h), 0.0);
    let vh = max(dot(v,h), 0.0);
    let d_denom = nh * nh * (alpha2 - 1.0) + 1.0;
    let distribution = alpha2 / max(PI * d_denom * d_denom, 0.0000001);
    let visibility_v = 2.0 * nv / max(nv + sqrt(alpha2 + (1.0-alpha2)*nv*nv), 0.001);
    let visibility_l = 2.0 * nl / max(nl + sqrt(alpha2 + (1.0-alpha2)*nl*nl), 0.001);
    let sun_height = dot(up,l);
    let above_horizon = smoothstep(-0.012, 0.018, sun_height);
    let atmosphere = exp(-0.21 / max(sun_height, 0.04));
    let clouds = pow(1.0-cover, 2.0);
    let warmth = 1.0 - smoothstep(0.06, 0.35, sun_height);
    let sun_color = mix(vec3<f32>(1.0,0.97,0.90), vec3<f32>(1.0,0.43,0.16), warmth);
    let brdf_times_nl = distribution * visibility_v * visibility_l * water_fresnel(vh) / max(4.0*nv, 0.004);
    return sun_color * 130000.0 * atmosphere * above_horizon * clouds * brdf_times_nl;
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

    let mask_position = water.origin.xyz + ecef_relative * water.origin.w;
    let mask_direction = normalize(select(vec3<f32>(1.0,0.0,0.0),mask_position,dot(mask_position,mask_position)>0.000001));
    let channels = textureSample(water_mask, water_sampler, mask_direction).rg;
    // Stay conservative at the coarse coastline: mixed dry-land pixels retain
    // the original palette. This mask does not promise surveyed shore geometry.
    let water_amount = smoothstep(0.65, 0.98, channels.r + channels.g) * select(0.0, 1.0, water.settings.x > 0.0);
    let lake = channels.g / max(channels.r + channels.g, 0.001);
    let up_world = normalize(pbr_input.world_normal);
    let up_ecef = normalize(vec3<f32>(dot(water.ecef_x.xyz,up_world),dot(water.ecef_y.xyz,up_world),dot(water.ecef_z.xyz,up_world)));
    let normal_dx = dpdx(up_world);
    let normal_dy = dpdy(up_world);
    let view_dx = dpdx(pbr_input.V);
    let view_dy = dpdy(pbr_input.V);
    let angular_variance = 0.25 * (dot(normal_dx,normal_dx)+dot(normal_dy,normal_dy)+dot(view_dx,view_dx)+dot(view_dy,view_dy));
    var slope = vec3<f32>(0.0);
    var unresolved_variance = 0.0;
    // Derivative-derived footprint evaluated above is unconditional. Unresolved
    // wave energy moves into slope variance rather than disappearing or aliasing.
    for (var i = 0u; i < 8u; i += 1u) {
        let wave = water.waves[i];
        let k = max(length(wave.xyz), 0.00001);
        let tangent = wave.xyz / k - up_ecef * dot(wave.xyz / k, up_ecef);
        let amplitude = SLOPES[i] * mix(1.0, 0.55, lake);
        let selected = select(0.0, 1.0, f32(i) < water.settings.x);
        let resolved = water_band_weight(footprint_m * k / (2.0 * PI)) * selected;
        if resolved > 0.0 {
            slope += tangent * amplitude * resolved * cos(dot(wave.xyz,ecef_relative)+wave.w);
        }
        // High omits the two shortest waves but retains their statistical energy.
        unresolved_variance += 0.5 * amplitude * amplitude * dot(tangent,tangent) * (1.0-resolved*resolved);
    }
    let n_ecef = normalize(up_ecef-slope);
    let water_normal = normalize(water.ecef_x.xyz*n_ecef.x+water.ecef_y.xyz*n_ecef.y+water.ecef_z.xyz*n_ecef.z);
    let alpha2 = clamp(0.0049 + unresolved_variance + angular_variance, 0.0049, 0.25);
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
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    if water_amount > 0.0 {
        let n = water_normal;
        let v = pbr_input.V;
        let l = water.sun.xyz;
        let reflect_direction = reflect(-v,n);
        let fresnel = water_fresnel(max(dot(n,v),0.0));
        // Broaden unresolved sky response with the same variance used for glitter.
        let sky = analytic_sky(normalize(mix(reflect_direction,up_world,alpha2*1.5)),up_world,l,water.settings.y);
        let daylight = smoothstep(-0.10,0.12,dot(up_world,l));
        let body_color = mix(vec3<f32>(0.006,0.039,0.055),vec3<f32>(0.014,0.049,0.038),lake);
        let body = body_color * daylight * mix(2300.0,1300.0,water.settings.y) * (1.0-fresnel);
        let glitter = sun_glitter(n,v,l,up_world,alpha2,water.settings.y);
        let radiance = body + fresnel*sky + glitter;
        out.color = vec4<f32>(mix(out.color.rgb,radiance*view.exposure,water_amount),out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input,out.color);
    return out;
}
