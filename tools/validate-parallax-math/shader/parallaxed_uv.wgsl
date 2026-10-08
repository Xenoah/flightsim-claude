// SPDX-License-Identifier: MIT OR Apache-2.0
// Original implementation prepared from the documented ray/height-field contract.
// Keep the containing module's imports and sample_depth_map binding helper.
fn parallaxed_uv(
    depth_scale: f32,
    max_layer_count: f32,
    max_steps: u32,
    original_uv: vec2<f32>,
    Vt: vec3<f32>,
    material_bind_group_slot: u32,
) -> vec2<f32> {
    if max_layer_count < 1.0 || depth_scale == 0.0 {
        return original_uv;
    }

    // Normalize in two stages so finite, large direction components do not
    // overflow a dot product. A zero direction cannot define a displaced ray.
    let direction_extent = max(max(abs(Vt.x), abs(Vt.y)), abs(Vt.z));
    if direction_extent == 0.0 || all(Vt.xy == vec2<f32>(0.0)) {
        return original_uv;
    }
    let scaled_direction = Vt / direction_extent;
    let direction = scaled_direction / length(scaled_direction);
    let incidence = abs(direction.z);

    // The exact tangent-plane projection is singular at grazing incidence.
    // These bounds keep useful finite inputs finite and bound per-fragment work.
    let projection_z = max(incidence, 0.0001);
    let bounded_scale = clamp(depth_scale, -1.0e20, 1.0e20);
    let ray_uv = bounded_scale * (vec2<f32>(direction.x, -direction.y) / projection_z);
    let maximum_layers = floor(min(max_layer_count, 1024.0));
    let layers = u32(ceil(1.0 + (maximum_layers - 1.0) * (1.0 - incidence)));

    // Residual F(t) = t - height(original_uv + t * ray_uv).
    // Visit samples in increasing depth and keep the first sampled sign bracket.
    let start_height = clamp(sample_depth_map(original_uv, material_bind_group_slot), 0.0, 1.0);
    if start_height == 0.0 {
        return original_uv;
    }
    var lower_depth = 0.0;
    var lower_residual = -start_height;
    var upper_depth = 1.0;
    var upper_residual = 0.0;
    for (var layer = 1u; layer <= layers; layer += 1u) {
        let depth = f32(layer) / f32(layers);
        let uv = original_uv + depth * ray_uv;
        let height = clamp(sample_depth_map(uv, material_bind_group_slot), 0.0, 1.0);
        let residual = depth - height;
        if residual >= 0.0 {
            if residual == 0.0 {
                return uv;
            }
            upper_depth = depth;
            upper_residual = residual;
            break;
        }
        lower_depth = depth;
        lower_residual = residual;
    }

#ifdef RELIEF_MAPPING
    // Each new sample halves the selected depth interval. Stop when f32 can
    // no longer represent a strictly interior midpoint, even if steps remain.
    for (var step = 0u; step < min(max_steps, 24u); step += 1u) {
        let middle_depth = lower_depth + 0.5 * (upper_depth - lower_depth);
        if middle_depth <= lower_depth || middle_depth >= upper_depth {
            break;
        }
        let uv = original_uv + middle_depth * ray_uv;
        let height = clamp(sample_depth_map(uv, material_bind_group_slot), 0.0, 1.0);
        let residual = middle_depth - height;
        if residual == 0.0 {
            return uv;
        }
        if residual > 0.0 {
            upper_depth = middle_depth;
            upper_residual = residual;
        } else {
            lower_depth = middle_depth;
            lower_residual = residual;
        }
    }
#endif

    // Interpolation is exact in real arithmetic when the sampled residual is
    // affine over this bracket; an active height clamp kink breaks that condition.
    // Relief uses the refined bracket; zero steps retain ordinary interpolation.
    let residual_span = upper_residual - lower_residual;
    var fraction = 0.5;
    if residual_span > 0.0 {
        fraction = clamp(-lower_residual / residual_span, 0.0, 1.0);
    }
    let hit_depth = lower_depth + fraction * (upper_depth - lower_depth);
    return original_uv + hit_depth * ray_uv;
}
