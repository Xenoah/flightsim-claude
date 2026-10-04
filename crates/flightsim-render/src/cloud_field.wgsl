#define_import_path flightsim::cloud_field

// Shared with cloud_field.rs: 64³ RG8, period16 base cells,8000m per base cell.
// R is precomputed fBm, G independent erosion detail. Exactly one trilinear
// texture lookup per field evaluation; no per-ray-step hash or octave loops.
@group(0) @binding(4) var cloud_field_texture: texture_3d<f32>;
@group(0) @binding(5) var cloud_field_sampler: sampler;

// Core transforms positions to ECEF; this is only radial texture projection.
fn cloud_field_coordinates(ecef_cells: vec3<f32>, drift_cells: vec3<f32>) -> vec3<f32> {
    let q = normalize(ecef_cells) * 796.376096426 + drift_cells;
    return q - floor(q / 16.0) * 16.0;
}
// Only mip0 red noise has the calibrated CDF. The renderer may replace green
// with threshold-aware filtered density for distant minification. Never apply
// the mip0 quantile to averaged red noise at a higher LOD.
fn cloud_field_rg_filtered(coords: vec3<f32>, lod: f32) -> vec2<f32> {
    return textureSampleLevel(cloud_field_texture, cloud_field_sampler,
        (coords * 4.0 + vec3<f32>(0.5)) / 64.0, lod).rg;
}
fn cloud_field_rg(coords: vec3<f32>) -> vec2<f32> {
    return cloud_field_rg_filtered(coords, 0.0);
}
fn cloud_field_noise(coords: vec3<f32>, seed: u32) -> f32 {
    // Seed already determined the uploaded texture. Texel centres represent
    // index/4 base cells, matching the CPU's eight-byte trilinear interpolation.
    return cloud_field_rg(coords).r;
}
fn cloud_field_erosion(coords: vec3<f32>) -> f32 {
    return cloud_field_rg(coords).g;
}
// threshold is the CPU's calibrated inverse-CDF uniform, never1-cover.
fn cloud_field_mask(noise: f32, threshold: f32) -> f32 {
    return smoothstep(threshold - 0.015, threshold + 0.015, noise);
}
fn cloud_field_density(noise: f32, threshold: f32) -> f32 {
    return smoothstep(threshold, threshold + 0.06, noise);
}
fn cloud_field_vertical_profile(height: f32, noise: f32) -> f32 {
    let n = clamp(noise, 0.0, 1.0);
    let bottom = 0.04 + 0.06 * (1.0 - n);
    let top = 0.58 + 0.38 * n;
    return smoothstep(bottom, bottom + 0.12, height)
        * (1.0 - smoothstep(top - 0.22, top, height));
}
