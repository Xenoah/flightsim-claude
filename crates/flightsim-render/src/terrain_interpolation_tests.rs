//! Regression for the subpixel stitch-triangle MSAA color flashes. Exercise the
//! real material specialization and Bevy shader preprocessor, not a second WGSL
//! implementation. These CPU tests do not claim native-driver or pixel coverage.

use super::TerrainDetail;
use bevy::{
    asset::uuid_handle,
    mesh::MeshVertexBufferLayouts,
    pbr::{
        MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline, MeshLayouts,
        MeshPipeline, MeshPipelineKey, MeshPipelineViewLayout, MeshPipelineViewLayouts,
    },
    prelude::*,
    render::render_resource::{
        BufferBindingType, DownlevelFlags, FragmentState, MultisampleState,
        RenderPipelineDescriptor, VertexState,
    },
    shader::{ShaderCache, ShaderCacheSource, ShaderDefVal},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const DEFINE: &str = "FLIGHTSIM_TERRAIN_CENTROID";
const FORWARD_IO: &str = include_str!("../../../vendor/bevy_pbr/src/render/forward_io.wgsl");
const PREPASS_IO: &str = include_str!("../../../vendor/bevy_pbr/src/prepass/prepass_io.wgsl");

// Specialization needs no GPU objects: these unused layouts are descriptors,
// not fake handles or RenderDevices. Calling the actual trait method also catches
// an accidentally disconnected specialization helper.
fn specialize<E: MaterialExtension<Data = ()>>(descriptor: &mut RenderPipelineDescriptor) {
    let pipeline = MaterialExtensionPipeline {
        mesh_pipeline: MeshPipeline {
            view_layouts: MeshPipelineViewLayouts(Arc::new(std::array::from_fn(|_| {
                MeshPipelineViewLayout {
                    main_layout: default(),
                    binding_array_layout: default(),
                    empty_layout: default(),
                    #[cfg(debug_assertions)]
                    texture_count: 0,
                }
            }))),
            clustered_forward_buffer_binding_type: BufferBindingType::Uniform,
            mesh_layouts: MeshLayouts {
                model_only: default(),
                lightmapped: default(),
                skinned: default(),
                skinned_motion: default(),
                morphed: default(),
                morphed_motion: default(),
                morphed_skinned: default(),
                morphed_skinned_motion: default(),
            },
            shader: default(),
            per_object_buffer_batch_size: None,
            binding_arrays_are_usable: false,
            clustered_decals_are_usable: false,
            skins_use_uniform_buffers: false,
        },
    };
    let mesh = Mesh::from(Triangle3d::default());
    let layout = mesh.get_mesh_vertex_buffer_layout(&mut MeshVertexBufferLayouts::default());
    let mesh_key = MeshPipelineKey::from_msaa_samples(descriptor.multisample.count);
    E::specialize(
        &pipeline,
        descriptor,
        &layout,
        MaterialExtensionKey {
            mesh_key,
            bind_group_data: (),
        },
    )
    .unwrap();
}

fn descriptor(defs: &[&str], fragment: bool) -> RenderPipelineDescriptor {
    let defs: Vec<ShaderDefVal> = defs.iter().map(|name| (*name).into()).collect();
    RenderPipelineDescriptor {
        vertex: VertexState {
            shader_defs: defs.clone(),
            ..default()
        },
        fragment: fragment.then(|| FragmentState {
            shader_defs: defs,
            ..default()
        }),
        multisample: MultisampleState {
            count: 4,
            ..default()
        },
        ..default()
    }
}

fn module(io: &str, source: &str, defs: &[ShaderDefVal]) -> Arc<naga::Module> {
    let mut cache =
        ShaderCache::new(
            default(),
            DownlevelFlags::all(),
            |_: &(), source, _| match source {
                ShaderCacheSource::Naga(module) => Ok(module),
                _ => panic!("expected Bevy's preprocessed Naga module"),
            },
        );
    let io_handle: Handle<Shader> = uuid_handle!("02d27302-6842-451a-ac68-035fcdd8d97e");
    let main_handle: Handle<Shader> = uuid_handle!("0be007cb-4e14-463a-9406-cec9e56e0606");
    cache.set_shader(
        io_handle.id(),
        Shader::from_wgsl(io.to_owned(), "terrain-test-io.wgsl"),
    );
    cache.set_shader(
        main_handle.id(),
        Shader::from_wgsl(source.to_owned(), "terrain-test-entry.wgsl"),
    );
    let module = cache.get(&(), 0, main_handle.id(), defs).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    let expected_centroid_locations = if io == FORWARD_IO && defs.contains(&DEFINE.into()) {
        if defs.contains(&"VERTEX_COLORS".into()) {
            &[0, 1, 5][..]
        } else {
            &[0, 1][..]
        }
    } else {
        &[]
    };
    assert_backend_centroids(&module, &info, expected_centroid_locations);
    module
}

// Bevy already enables both writers through its DX12/Vulkan feature graph.
// This verifies emitted shader interfaces, not DXC, a driver or a GPU pipeline.
fn assert_backend_centroids(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    expected: &[u32],
) {
    let expected: BTreeSet<u32> = expected.iter().copied().collect();
    let mut hlsl = String::new();
    let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &default(), &default())
        .write(module, info, None)
        .unwrap();
    assert!(reflection.entry_point_names.iter().all(Result::is_ok));
    let hlsl_locations: BTreeSet<u32> = hlsl
        .lines()
        .filter(|line| line.split_whitespace().any(|word| word == "centroid"))
        .map(|line| {
            line.rsplit_once(" : LOC")
                .unwrap()
                .1
                .trim_end_matches(';')
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(hlsl_locations, expected, "HLSL centroid locations\n{hlsl}");

    let words = naga::back::spv::write_vec(module, info, &default(), None).unwrap();
    let mut locations = BTreeMap::new();
    let mut centroid_ids = BTreeSet::new();
    let mut cursor = 5; // SPIR-V's five-word module header.
    while cursor < words.len() {
        let length = usize::try_from(words[cursor] >> 16).unwrap();
        assert!(length > 0 && cursor + length <= words.len());
        // SPIR-V grammar: OpDecorate = 71, Centroid = 16, Location = 30.
        // Decode instructions rather than matching incidental raw word values.
        if words[cursor] & 0xffff == 71 {
            let instruction = &words[cursor..cursor + length];
            match instruction[2] {
                16 => {
                    assert_eq!(length, 3);
                    centroid_ids.insert(instruction[1]);
                }
                30 => {
                    assert_eq!(length, 4);
                    locations.insert(instruction[1], instruction[3]);
                }
                _ => {}
            }
        }
        cursor += length;
    }
    let spv_locations: BTreeSet<u32> = centroid_ids.iter().map(|id| locations[id]).collect();
    assert_eq!(spv_locations, expected, "SPIR-V centroid locations");
}

fn io_members(module: &naga::Module, stage: naga::ShaderStage) -> &[naga::StructMember] {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.stage == stage)
        .unwrap();
    let ty = if stage == naga::ShaderStage::Vertex {
        entry.function.result.as_ref().unwrap().ty
    } else {
        entry.function.arguments[0].ty
    };
    let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner else {
        panic!("expected imported VertexOutput");
    };
    members
}

fn assert_sampling(members: &[naga::StructMember], centroid: bool, color: bool) {
    let names = if color {
        &["world_position", "world_normal", "color"][..]
    } else {
        &["world_position", "world_normal"][..]
    };
    for &name in names {
        let member = members
            .iter()
            .find(|m| m.name.as_deref() == Some(name))
            .unwrap();
        let Some(naga::Binding::Location {
            interpolation,
            sampling,
            ..
        }) = member.binding
        else {
            panic!("missing varying binding for {name}");
        };
        assert_eq!(
            interpolation,
            Some(naga::Interpolation::Perspective),
            "{name}"
        );
        assert_eq!(
            sampling,
            Some(if centroid {
                naga::Sampling::Centroid
            } else {
                naga::Sampling::Center
            }),
            "{name}: subpixel MSAA coverage must not extrapolate terrain varyings at an uncovered center"
        );
    }
}

fn stages(io_module: &str) -> [String; 2] {
    [
        format!(
            "#import bevy_pbr::{io_module}::VertexOutput\n@vertex fn vertex() -> VertexOutput {{ var out: VertexOutput; out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0); return out; }}"
        ),
        format!(
            "#import bevy_pbr::{io_module}::VertexOutput\n@fragment fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {{\nvar color = vec4<f32>(0.0);\n#ifdef VERTEX_COLORS\ncolor = in.color;\n#endif\nreturn color + in.world_position + vec4<f32>(in.world_normal, 0.0);\n}}"
        ),
    ]
}

#[test]
fn terrain_specialization_changes_only_both_stage_defines() {
    for samples in [1, 4, 8] {
        for fragment in [false, true] {
            let mut actual = descriptor(&["VERTEX_COLORS", "EXISTING_DEFINE"], fragment);
            actual.multisample.count = samples;
            let mut expected = actual.clone();
            expected.vertex.shader_defs.push(DEFINE.into());
            if let Some(stage) = &mut expected.fragment {
                stage.shader_defs.push(DEFINE.into());
            }
            specialize::<TerrainDetail>(&mut actual);
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn terrain_forward_stages_use_centroid_and_other_materials_keep_center() {
    // Optional UV/tangent/flat instance fields must retain their exact bindings.
    let baseline = descriptor(
        &[
            "VERTEX_COLORS",
            "VERTEX_UVS_A",
            "VERTEX_UVS_B",
            "VERTEX_TANGENTS",
            "VERTEX_OUTPUT_INSTANCE_INDEX",
            "VISIBILITY_RANGE_DITHER",
        ],
        true,
    );
    let mut terrain = baseline.clone();
    specialize::<TerrainDetail>(&mut terrain);
    let mut water = baseline.clone();
    specialize::<crate::water::WaterSurface>(&mut water);
    assert_eq!(
        water, baseline,
        "water must not opt into terrain interpolation"
    );
    for (stage, source, terrain_defs, original_defs) in [
        (
            naga::ShaderStage::Vertex,
            &stages("forward_io")[0],
            &terrain.vertex.shader_defs,
            &baseline.vertex.shader_defs,
        ),
        (
            naga::ShaderStage::Fragment,
            &stages("forward_io")[1],
            &terrain.fragment.as_ref().unwrap().shader_defs,
            &baseline.fragment.as_ref().unwrap().shader_defs,
        ),
    ] {
        let actual_module = module(FORWARD_IO, source, terrain_defs);
        let baseline_module = module(FORWARD_IO, source, original_defs);
        let actual = io_members(&actual_module, stage);
        let baseline = io_members(&baseline_module, stage);
        assert_sampling(actual, true, true);
        assert_sampling(baseline, false, true);
        assert_eq!(actual.len(), baseline.len());
        for (a, b) in actual.iter().zip(baseline) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.offset, b.offset);
            assert_eq!(
                actual_module.types[a.ty].inner,
                baseline_module.types[b.ty].inner
            );
            let mut expected_binding = b.binding.clone();
            if matches!(
                a.name.as_deref(),
                Some("world_position" | "world_normal" | "color")
            ) {
                let Some(naga::Binding::Location { sampling, .. }) = &mut expected_binding else {
                    panic!("expected varying location");
                };
                *sampling = Some(naga::Sampling::Centroid);
            }
            assert_eq!(a.binding, expected_binding);
        }
    }
}

#[test]
fn uncolored_forward_materials_validate_with_no_optional_fields() {
    let baseline = descriptor(&[], true);
    let mut terrain = baseline.clone();
    specialize::<TerrainDetail>(&mut terrain);
    for (stage, source, terrain_defs, original_defs) in [
        (
            naga::ShaderStage::Vertex,
            &stages("forward_io")[0],
            &terrain.vertex.shader_defs,
            &baseline.vertex.shader_defs,
        ),
        (
            naga::ShaderStage::Fragment,
            &stages("forward_io")[1],
            &terrain.fragment.as_ref().unwrap().shader_defs,
            &baseline.fragment.as_ref().unwrap().shader_defs,
        ),
    ] {
        for (defs, centroid) in [(terrain_defs, true), (original_defs, false)] {
            let module = module(FORWARD_IO, source, defs);
            let members = io_members(&module, stage);
            assert_eq!(
                members.len(),
                3,
                "only clip position, world position and normal"
            );
            assert!(
                members
                    .iter()
                    .all(|member| member.name.as_deref() != Some("color"))
            );
            assert_sampling(members, centroid, false);
        }
    }
}

#[test]
fn terrain_define_preserves_prepass_and_deferred_stage_interfaces() {
    for deferred in [false, true] {
        let mut defs = vec![
            "PREPASS_PIPELINE",
            "VERTEX_COLORS",
            "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
            "NORMAL_PREPASS",
            "PREPASS_FRAGMENT",
            "VERTEX_UVS_A",
            "VERTEX_UVS_B",
            "VERTEX_TANGENTS",
            "MOTION_VECTOR_PREPASS",
            "UNCLIPPED_DEPTH_ORTHO_EMULATION",
            "VERTEX_OUTPUT_INSTANCE_INDEX",
            "VISIBILITY_RANGE_DITHER",
        ];
        if deferred {
            defs.push("DEFERRED_PREPASS");
        }
        let mut terrain = descriptor(&defs, true);
        let baseline = terrain.clone();
        specialize::<TerrainDetail>(&mut terrain);
        for (stage, source, defs, original_defs) in [
            (
                naga::ShaderStage::Vertex,
                &stages("prepass_io")[0],
                &terrain.vertex.shader_defs,
                &baseline.vertex.shader_defs,
            ),
            (
                naga::ShaderStage::Fragment,
                &stages("prepass_io")[1],
                &terrain.fragment.as_ref().unwrap().shader_defs,
                &baseline.fragment.as_ref().unwrap().shader_defs,
            ),
        ] {
            assert!(defs.contains(&DEFINE.into()));
            let actual = module(PREPASS_IO, source, defs);
            let original = module(PREPASS_IO, source, original_defs);
            assert_sampling(io_members(&actual, stage), false, true);
            assert_eq!(io_members(&actual, stage), io_members(&original, stage));
        }
    }
}
