#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> plane: vec4<f32>;
@group(1) @binding(1) var<uniform> sky: vec4<f32>;
@group(1) @binding(2) var<uniform> ground: vec4<f32>;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // The quad never rotates or grows beyond the dial. Only this signed
    // sky/ground boundary moves, in normalized dial coordinates for DPI safety.
    let point = in.uv - vec2<f32>(0.5);
    let distance = dot(plane.xy, point) - plane.z;
    let edge = max(fwidth(distance), 0.000001);
    let fraction = smoothstep(-0.5 * edge, 0.5 * edge, distance);
    let color = mix(sky, ground, fraction);
    // Overflow::clip is rectangular. Mask the circular gauge explicitly in
    // fragment space, with a one-pixel inner antialias fringe.
    let radius = length(point);
    let rim_width = max(fwidth(radius), 0.000001);
    let coverage = 1.0 - smoothstep(0.5 - rim_width, 0.5, radius);
    return vec4<f32>(color.rgb, color.a * coverage);
}
