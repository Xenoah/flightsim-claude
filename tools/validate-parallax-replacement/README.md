# Actual-module parallax import/binding validation

This standalone, CPU-only check composes the **modified vendored module** using
Naga-Oil 0.20.0 and validates the generated Naga 27.0.3 module. It reads the real
binding/helper modules, not substituted stubs. The synthetic fragment entrypoint
is a test fixture: this is not a full Bevy forward/deferred pipeline test.

## Verified result

All eight combinations pass:

- Ordinary and `BINDLESS`
- Without and with `RELIEF_MAPPING`
- Storage-buffer mesh and uniform-buffer mesh (`PER_OBJECT_BUFFER_BATCH_SIZE=64`)

The final harness passed formatting checks, Clippy with warnings denied, shell
syntax checks, and two complete runs whose JSON reports were byte-identical.

Every combination verifies:

1. The SHA-256 of every input before use and the exact independently reviewed
   appended replacement (hash below)
2. Naga-Oil preprocessing and genuine direct/transitive module import resolution
3. Naga validation with all validation flags, using the explicitly listed
   capabilities, and exactly one fragment entrypoint
4. The retained six-argument `parallaxed_uv` function and one or two loops as
   selected by the relief definition
5. The genuine `sample_depth_map` helper with exactly one explicit-LOD image
   sample instruction
6. Exact binding group/index sets, including the genuine material and mesh inputs
7. Emitted WGSL parsing and full validation again

The test mutates a helper call **in memory only** and requires composition to
reject the wrong argument count. It also requires rejection when
`MATERIAL_BIND_GROUP` is missing. Bindless variants must fail final validation
when nonuniform-indexing capabilities are removed but cube-array capability is
retained. A separate check confirms the genuine bindless module header cannot
be composed without cube-array capability. No input file is changed by these
negative controls.

## Exact sources and definitions

`inputs.json` is the enforced input manifest, ordered for dependency registration.
The full genuine closure is:

- `bevy_render::bindless`, from matching **bevy_render 0.18.1**
- `bevy_pbr::mesh_types`
- `bevy_pbr::mesh_bindings`, importing `mesh_types::Mesh`
- `bevy_pbr::pbr_types`
- `bevy_pbr::pbr_bindings`, importing `pbr_types::StandardMaterial`
- `bevy_pbr::parallax_mapping`, the actual locally modified vendored module

The latter five are read directly from `vendor/bevy_pbr/src/render/`. No original
registry parallax module, excluded routine, tutorial, or crate archive is read or
copied. Imported modules and the texture sampling helper are not replaced,
stripped, rewritten, or preprocessed by a custom parser. Naga-Oil does the actual
conditional processing, import resolution, and unused-symbol elimination.

The reviewed independent replacement begins at its SPDX comment in the modified
module. Its complete bytes must have SHA-256:

`8fa277d6bfefdda361d9cfe2274a0dc54d509fa603d35236aa7816f9cf095a58`

The complete modified module, including genuine imports and sampling helper,
must have SHA-256:

`65226a61e28f9f7b593e7a59d983a8b52e8c5ced952156e2977774dd5687baf1`

All variants define `MATERIAL_BIND_GROUP=UInt(3)`. The optional defines are
`BINDLESS=Bool(true)`, `RELIEF_MAPPING=Bool(true)`, and
`PER_OBJECT_BUFFER_BATCH_SIZE=UInt(64)`. Absent defines are omitted, not set false.
The value 64 is a valid representative test batch size, not a measured device
batch size. No application flags/features/defaults are changed.

The synthetic fragment receives varying UV/direction and a flat mesh index. It
reads the genuine mesh material slot, the genuine material fields
`parallax_depth_scale`, `max_parallax_layer_count`, and
`max_relief_mapping_search_steps`, and calls the real function with `-Vt` and the
material slot. In bindless mode, it reads the actual indices and material array.
This exercises parameter and resource types instead of replacing them with
constants or simplified declarations. It does not execute a full TBN transform,
material flag branch, vertex stage, or texture sampling on hardware.

### Shader definition and calling-convention evidence

The referenced files are identified in `reference-inputs.json`; those are
read-only supporting evidence, not part of the composed module list.

- `vendor/bevy_pbr/src/material.rs`: `MATERIAL_BIND_GROUP_INDEX` is 3;
  specialization adds `MATERIAL_BIND_GROUP` and optional `BINDLESS`
- `vendor/bevy_pbr/src/pbr_material.rs`: `ParallaxMappingMethod::Relief` selects
  the material key which adds `RELIEF_MAPPING`; the material shader fields are
  populated from the corresponding material settings
- `vendor/bevy_pbr/src/render/pbr_fragment.wgsl`: the actual caller uses the three
  material fields above, UV, negated tangent view direction, and material slot
- `vendor/bevy_pbr/src/render/mesh.rs`: `PER_OBJECT_BUFFER_BATCH_SIZE` is an optional
  pipeline-provided definition
- `bevy_shader-0.18.1/src/shader_cache.rs`: the Bevy shader cache maps sampled
  texture/storage-buffer nonuniform indexing and sampler nonuniform indexing
  from the adapter features; cube-array capability is likewise adapter-derived

### Capabilities and binding assertions

Ordinary variants validate with `Capabilities::empty()`.

Bindless variants explicitly enable only:

- `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`
- `SAMPLER_NON_UNIFORM_INDEXING`
- `CUBE_ARRAY_TEXTURES`

Cube-array support is necessary here because Naga-Oil validates the genuine
bindless module's full header, which also declares cube-array textures. The
final retained binding list does not include those cube-array textures. This
requirement was tested, not inferred. No `Capabilities::all()` shortcut is used.
These are validator assumptions, not measured adapter support.

Expected ordinary bindings `(group, binding)`:

- `(2, 0)`: mesh storage or uniform buffer
- `(3, 0)`: uniform `StandardMaterial`
- `(3, 11)`: depth texture
- `(3, 12)`: depth sampler

Expected bindless bindings:

- `(2, 0)`: mesh storage or uniform buffer
- `(3, 0)`: material index table
- `(3, 1)`: filtering sampler binding array
- `(3, 5)`: 2D texture binding array
- `(3, 10)`: material storage array

## Reproduce

Use the dependency versions pinned in `Cargo.lock`. The locked dependencies
declare Rust requirements up to 1.85; the observed and tested toolchain was
Rust/Cargo 1.93.0 on Linux. The lower MSRV was not independently tested. Supply an intact matching
`bevy_render-0.18.1` source directory; only `src/bindless.wgsl` is read from it.
Do not point the repository argument at an unmodified upstream checkout.

```sh
./validate.sh /absolute/path/to/flightsim-repository \
  /absolute/path/to/bevy_render-0.18.1 /absolute/path/to/output
```

`validate.sh` runs formatting and Clippy with warnings denied, then the executable
assertion suite. Dependencies must already be available in `CARGO_HOME`, since
it deliberately runs with `--locked --offline`. Normal authorized dependency
preparation may be done separately; it is not hidden in the script. Compiler and
Cargo versions plus pass lines go to `validation.log`. `validation.json` contains
the input and entrypoint hashes, all variants, capabilities, exact bindings,
composed-WGSL hashes, negative-control results, and limits. The composed shader
bytes are not exported. A failed run must be treated as failed even if an older
report remains in the chosen output directory; use a fresh output directory for
independent runs.

The standalone crate intentionally has its own workspace/lockfile and does not
modify the application's dependency graph. Its compiler/parser dependencies are
validation tooling, not shipped runtime dependency or source-license clearance
claims. Do not include `target/`, caches, binaries, or upstream crate archives in
source-only review exports. Copying the source harness alone does not establish
permission to redistribute external dependency source archives.

## Remaining verification

This passes genuine module import/parameter/binding integration under the listed
shader definitions. It does **not** establish GPU numerical/image behavior,
Windows/DX12/FXC behavior, a full application or full forward/deferred pipeline
pass, the active runtime selection of these variants, physical adapter support,
performance, or release/distribution authorization. CPU reference/numerical
verification of the independent routine is separate evidence.
