// SPDX-License-Identifier: MIT OR Apache-2.0
use naga::valid::{Capabilities, ValidationFlags, Validator};

const SOURCE: &str = include_str!("../shader/parallaxed_uv.wgsl");

// Deliberately supports only the single specified compile-time branch. This
// is not a replacement for the application's import/bindless preprocessor.
fn branch_source(relief: bool) -> String {
    let mut in_branch = false;
    let mut output = String::new();
    for line in SOURCE.lines() {
        match line.trim() {
            "#ifdef RELIEF_MAPPING" => {
                assert!(!in_branch);
                in_branch = true;
            }
            "#endif" => {
                assert!(in_branch);
                in_branch = false;
            }
            _ => {
                assert!(!line.trim().starts_with('#'), "unexpected directive");
                if !in_branch || relief {
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }
    }
    assert!(!in_branch);
    output
}

fn validate(source: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:#?}"));
}

#[test]
fn both_branches_parse_and_validate_with_analytic_stub() {
    for relief in [false, true] {
        let source = format!(
            "{}\n{}\n{}",
            r#"
fn sample_depth_map(uv: vec2<f32>, material_bind_group_slot: u32) -> f32 {
    return clamp(0.2 + 0.1 * uv.x + f32(material_bind_group_slot) * 0.001, 0.0, 1.0);
}"#,
            branch_source(relief),
            r#"
@fragment fn validate_entry(@location(0) uv: vec2<f32>, @location(1) view: vec3<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(parallaxed_uv(0.1, 32.0, 5u, uv, view, 2u), 0.0, 1.0);
}"#
        );
        validate(&source);
    }
}

#[test]
fn both_branches_validate_with_explicit_lod_texture_helper() {
    for relief in [false, true] {
        let source = format!(
            "{}\n{}\n{}",
            r#"
@group(0) @binding(0) var depth_texture: texture_2d<f32>;
@group(0) @binding(1) var depth_sampler: sampler;
fn sample_depth_map(uv: vec2<f32>, material_bind_group_slot: u32) -> f32 {
    return textureSampleLevel(depth_texture, depth_sampler, uv, 0.0).r;
}"#,
            branch_source(relief),
            r#"
@fragment fn validate_entry(@location(0) uv: vec2<f32>, @location(1) view: vec3<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(parallaxed_uv(0.1, 32.0, 5u, uv, view, 2u), 0.0, 1.0);
}"#
        );
        validate(&source);
    }
}
