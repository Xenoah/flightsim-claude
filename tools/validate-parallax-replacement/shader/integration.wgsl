// SPDX-License-Identifier: MIT OR Apache-2.0
// Test entrypoint only. All imported modules are the genuine Bevy inputs.
#import bevy_pbr::parallax_mapping::parallaxed_uv
#import bevy_pbr::mesh_bindings::mesh
#ifdef BINDLESS
#import bevy_pbr::pbr_bindings::{material_indices, material_array}
#else
#import bevy_pbr::pbr_bindings::material
#endif

struct Input {
    @location(0) uv: vec2<f32>,
    @location(1) tangent_direction: vec3<f32>,
    @location(2) @interpolate(flat) mesh_index: u32,
}

@fragment
fn fragment(input: Input) -> @location(0) vec4<f32> {
    let slot = mesh[input.mesh_index].material_and_lightmap_bind_group_slot & 65535u;
#ifdef BINDLESS
    let parameters = material_array[material_indices[slot].material];
#else
    let parameters = material;
#endif
    let uv = parallaxed_uv(
        parameters.parallax_depth_scale,
        parameters.max_parallax_layer_count,
        parameters.max_relief_mapping_search_steps,
        input.uv,
        -input.tangent_direction,
        slot,
    );
    return vec4<f32>(uv, 0.0, 1.0);
}
