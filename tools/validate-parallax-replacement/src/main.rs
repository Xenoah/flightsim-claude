// SPDX-License-Identifier: MIT OR Apache-2.0
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env, fs, path::Path};

const ENTRY: &str = include_str!("../shader/integration.wgsl");
const EXPECTED_FUNCTION_HASH: &str =
    "8fa277d6bfefdda361d9cfe2274a0dc54d509fa603d35236aa7816f9cf095a58";

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn compose(
    sources: &[(String, String)],
    defs: HashMap<String, ShaderDefValue>,
    capabilities: Capabilities,
) -> Result<naga::Module, String> {
    let mut composer = Composer::default().with_capabilities(capabilities);
    for (path, source) in sources {
        if let Err(error) = composer.add_composable_module(ComposableModuleDescriptor {
            source,
            file_path: path,
            ..Default::default()
        }) {
            return Err(error.emit_to_string(&composer));
        }
    }
    composer
        .make_naga_module(NagaModuleDescriptor {
            source: ENTRY,
            file_path: "shader/integration.wgsl",
            shader_defs: defs,
            ..Default::default()
        })
        .map_err(|e| e.emit_to_string(&composer))
}

fn loops(block: &naga::Block) -> usize {
    block
        .iter()
        .map(|s| match s {
            naga::Statement::Loop {
                body, continuing, ..
            } => 1 + loops(body) + loops(continuing),
            naga::Statement::Block(b) => loops(b),
            naga::Statement::If { accept, reject, .. } => loops(accept) + loops(reject),
            naga::Statement::Switch { cases, .. } => cases.iter().map(|c| loops(&c.body)).sum(),
            _ => 0,
        })
        .sum()
}

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(
        args.len(),
        4,
        "usage: validator REPO BEVY_RENDER_0_18_1_DIR REPORT_JSON"
    );
    let manifest: Value = serde_json::from_str(include_str!("../inputs.json")).unwrap();
    let sources: Vec<_> = manifest
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let root = if entry["root"] == "repo" {
                &args[1]
            } else {
                &args[2]
            };
            let path = entry["path"].as_str().unwrap();
            let source = fs::read_to_string(Path::new(root).join(path)).unwrap();
            assert_eq!(
                hash(source.as_bytes()),
                entry["sha256"].as_str().unwrap(),
                "input identity changed: {path}"
            );
            (path.to_string(), source)
        })
        .collect();
    let parallax = &sources.last().unwrap().1;
    let independent_source = &parallax[parallax.find("// SPDX-License-Identifier:").unwrap()..];
    assert_eq!(
        hash(independent_source.as_bytes()),
        EXPECTED_FUNCTION_HASH,
        "the module must contain the exact reviewed standalone replacement"
    );

    let mut variants = vec![];
    for bindless in [false, true] {
        for relief in [false, true] {
            for uniform_mesh in [false, true] {
                let name = format!(
                    "{}-{}-{}",
                    if bindless { "bindless" } else { "ordinary" },
                    if relief { "relief" } else { "interpolation" },
                    if uniform_mesh {
                        "uniform-mesh"
                    } else {
                        "storage-mesh"
                    }
                );
                let mut defs =
                    HashMap::from([("MATERIAL_BIND_GROUP".to_string(), ShaderDefValue::UInt(3))]);
                if bindless {
                    defs.insert("BINDLESS".into(), ShaderDefValue::Bool(true));
                }
                if relief {
                    defs.insert("RELIEF_MAPPING".into(), ShaderDefValue::Bool(true));
                }
                if uniform_mesh {
                    defs.insert(
                        "PER_OBJECT_BUFFER_BATCH_SIZE".into(),
                        ShaderDefValue::UInt(64),
                    );
                }
                let capabilities = if bindless {
                    Capabilities::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
                        | Capabilities::SAMPLER_NON_UNIFORM_INDEXING
                        | Capabilities::CUBE_ARRAY_TEXTURES
                } else {
                    Capabilities::empty()
                };
                let module = compose(&sources, defs.clone(), capabilities)
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                let info = Validator::new(ValidationFlags::all(), capabilities)
                    .validate(&module)
                    .unwrap();
                assert_eq!(module.entry_points.len(), 1);
                assert_eq!(module.entry_points[0].stage, naga::ShaderStage::Fragment);
                let function = module
                    .functions
                    .iter()
                    .find(|(_, f)| {
                        f.name
                            .as_ref()
                            .is_some_and(|n| n.starts_with("parallaxed_uv"))
                    })
                    .expect("replacement function must be retained")
                    .1;
                assert_eq!(function.arguments.len(), 6);
                assert_eq!(loops(&function.body), if relief { 2 } else { 1 });
                let helper = module
                    .functions
                    .iter()
                    .find(|(_, f)| {
                        f.name
                            .as_ref()
                            .is_some_and(|n| n.starts_with("sample_depth_map"))
                    })
                    .expect("real depth helper must be retained")
                    .1;
                let mut image_samples = 0;
                for (_, expression) in helper.expressions.iter() {
                    if let naga::Expression::ImageSample { level, .. } = expression {
                        assert!(matches!(level, naga::SampleLevel::Exact(_)));
                        image_samples += 1;
                    }
                }
                assert_eq!(
                    image_samples, 1,
                    "real helper uses exactly one explicit-LOD sample"
                );
                let mut bindings: Vec<_> = module
                    .global_variables
                    .iter()
                    .filter_map(|(_, g)| g.binding.as_ref().map(|b| (b.group, b.binding)))
                    .collect();
                bindings.sort();
                let expected = if bindless {
                    vec![(2, 0), (3, 0), (3, 1), (3, 5), (3, 10)]
                } else {
                    vec![(2, 0), (3, 0), (3, 11), (3, 12)]
                };
                assert_eq!(
                    bindings, expected,
                    "material and depth binding ABI changed: {name}"
                );
                let mesh = module
                    .global_variables
                    .iter()
                    .find(|(_, g)| {
                        g.binding
                            .as_ref()
                            .is_some_and(|b| b.group == 2 && b.binding == 0)
                    })
                    .unwrap()
                    .1;
                assert_eq!(
                    mesh.space,
                    if uniform_mesh {
                        naga::AddressSpace::Uniform
                    } else {
                        naga::AddressSpace::Storage {
                            access: naga::StorageAccess::LOAD,
                        }
                    }
                );
                let emitted = naga::back::wgsl::write_string(
                    &module,
                    &info,
                    naga::back::wgsl::WriterFlags::EXPLICIT_TYPES,
                )
                .unwrap();
                let reparsed = naga::front::wgsl::parse_str(&emitted).unwrap();
                Validator::new(ValidationFlags::all(), capabilities)
                    .validate(&reparsed)
                    .unwrap();
                let capability_negative = if bindless {
                    assert!(
                        Validator::new(ValidationFlags::all(), Capabilities::CUBE_ARRAY_TEXTURES)
                            .validate(&module)
                            .is_err(),
                        "bindless nonuniform indexing should require adapter capabilities"
                    );
                    "rejected_without_nonuniform_indexing_capabilities_cube_array_kept"
                } else {
                    "not_applicable"
                };
                println!("PASS {name}: exact inputs, real import closure, validation, expected binding layout, explicit LOD, WGSL round-trip");
                let mut def_report: Vec<_> =
                    defs.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
                def_report.sort();
                variants.push(json!({"name":name,"status":"pass","shader_defs":def_report,"capabilities":format!("{capabilities:?}"),"bindings":bindings,"replacement_loop_count":loops(&function.body),"explicit_lod_sample_count":image_samples,"composed_wgsl_sha256":hash(emitted.as_bytes()),"capability_negative_control":capability_negative}));
            }
        }
    }
    let broken_sources: Vec<_> = sources
        .iter()
        .map(|(p, s)| {
            (
                p.clone(),
                if p.ends_with("parallax_mapping.wgsl") {
                    s.replace(
                        "sample_depth_map(uv, material_bind_group_slot)",
                        "sample_depth_map(uv)",
                    )
                } else {
                    s.clone()
                },
            )
        })
        .collect();
    let defs = HashMap::from([("MATERIAL_BIND_GROUP".into(), ShaderDefValue::UInt(3))]);
    assert!(
        compose(&broken_sources, defs, Capabilities::empty()).is_err(),
        "negative call-ABI control must fail"
    );
    assert!(
        compose(&sources, HashMap::new(), Capabilities::empty()).is_err(),
        "undefined MATERIAL_BIND_GROUP control must fail"
    );
    let bindless_defs = HashMap::from([
        ("MATERIAL_BIND_GROUP".into(), ShaderDefValue::UInt(3)),
        ("BINDLESS".into(), ShaderDefValue::Bool(true)),
    ]);
    let no_cube = compose(
        &sources,
        bindless_defs,
        Capabilities::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | Capabilities::SAMPLER_NON_UNIFORM_INDEXING,
    )
    .unwrap_err();
    assert!(
        no_cube.contains("CUBE_ARRAY_TEXTURES"),
        "the genuine bindless header requires cube-array capability"
    );
    println!("PASS negative controls: broken helper call ABI, absent MATERIAL_BIND_GROUP, and missing bindless-header cube-array capability rejected");
    let report = json!({
        "schema_version":1,
        "scope":"CPU-only Naga-Oil composition of the actual replacement module and complete genuine direct/transitive import closure, with synthetic fragment entrypoint using genuine StandardMaterial and mesh fields",
        "naga_version":"27.0.3", "naga_oil_version":"0.20.0",
        "function_sha256":EXPECTED_FUNCTION_HASH,
        "entrypoint_sha256":hash(ENTRY.as_bytes()),
        "inputs":manifest,
        "variants":variants,
        "negative_controls":{"broken_helper_call_abi":"rejected","missing_material_bind_group":"rejected","missing_bindless_header_cube_array_capability":"rejected"},
        "limits":["No GPU execution or image output","No Windows/DX12 compiler validation","No complete Bevy forward/deferred pipeline invocation","No application/runtime or release authorization claim","Bindless device capability support is not measured"],
    });
    fs::write(
        &args[3],
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
}
