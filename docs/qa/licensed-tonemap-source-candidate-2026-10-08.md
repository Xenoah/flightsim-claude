# Tony/Filmic source subset candidate

Status: independently reviewed local source candidate. No full Bevy compile,
native execution, release-recipe adoption or publication is established here.
Base: `d918943a70d01644b5a6bebb8a903a5bbc89139f`.

## Purpose and boundary

Ordinary cameras default to TonyMcMapface, but upstream Bevy 0.18.1 embeds Tony,
Blender Filmic and AgX together when `tonemapping_luts` is enabled. The exact
Tony MIT and scoped Filmic BSD notice routes have separate reviewed records;
AgX's specific LUT grant remains unresolved. The source candidate preserves
ordinary Tony and both retained LUT payloads, while excluding the AgX payload
and explicitly rejecting renderer selection of AgX.

The root patch selects `vendor/bevy_core_pipeline` as modified local 0.18.1.
The normalized package manifest and features are byte-identical to upstream;
the root lock entry correctly drops the pristine-registry source/checksum.
The full original 49-member inventory, exact archive hash, omissions, licence
texts and supplemental notices are preserved in the
[vendor provenance](../../vendor/bevy_core_pipeline/FLIGHTSIM-UPSTREAM-SOURCE.json)
and [modification record](../../vendor/bevy_core_pipeline/FLIGHTSIM-PATCHES.md).
Of those 49 members, 46 remain byte-identical, one module changes, and the AgX
payload plus stale registry VCS metadata are omitted. Shader source, retained
LUT bytes, decoding and sampling settings are unchanged.

AgX remains an enum/resource compatibility surface. Both HDR specialization
and shared LUT-binding selection deliberately panic with a diagnostic, even
with a caller-supplied handle. No substitute AgX image is rendered. Analytical
methods reuse the retained Tony handle without sampling its color, preserving
the unchanged node's ready-image cache path. In feature-off mode this is the
same upstream shared placeholder. Camera3d analytical remapping remains in the
unchanged app/render source; an explicit rendered Camera2d AgX is unsupported.

Rejected shortcuts: disabling the feature and injecting a manual Tony texture
does not satisfy the upstream HDR diagnostics or feature closure; silent AgX
fallbacks would misrepresent its behavior. A direct fallback-image binding for
non-LUT methods was corrected during independent review because the existing
node deliberately avoids caching that transient image.

## Bounded checks performed

- Exact archive SHA-256 matches the original root lock checksum; all 49 member
  hashes, retained payloads and asset notices checked against source
- Entire vendored tree checked for the full omitted AgX payload; no occurrence
- Six source mutations rejected: changed Tony bytes, shader, decoder feature,
  returned AgX file, missing retained node, changed Tony notice
- The exact HDR selection expression and full shared-binding function were
  extracted into a tiny Rust harness with GPU resource identity stand-ins.
  Both `tonemapping_luts` on/off passed with Rust 1.93.0 and `-Dwarnings`:
  seven supported mappings, default Tony, both AgX panics with supplied custom
  handle, feature-off Tony/Filmic diagnostic counts, and missing-image fallback
- `cargo tree --locked --offline`, Windows MSVC target, single-package ordinary
  and analytical configurations for both app and render, all resolved to this
  vendor path. All four passed the existing graph-only validator unchanged.
  Ordinary core feature: `tonemapping_luts`; analytical core feature: none.
  KTX2/zstd/zstd_rust decoding remains in both exact graphs
- Existing tonemapping-build tests: 10 passed; CI tonemapping tests: 6 passed
- Workspace `cargo fmt --all --check`, changed project/module whitespace
  checks, and modified upstream module rustfmt with its original 2021 style
  passed. The full staged whitespace check reports six pre-existing upstream
  findings in four unchanged shader/info files (trailing spaces or blank EOF
  lines); those exact upstream bytes are deliberately preserved

The extraction harness tests actual selection source, not Bevy type integration,
shader compilation, image decoding/upload or a native renderer. No screenshot,
pixel-equivalence or performance measurement is claimed. The cache correction
preserves a reuse opportunity; it is not an FPS claim.

Reproduce the source and small selection checks, with the verified upstream
archive outside the distribution:

```sh
python3 tools/validate-tonemap-subset.py \
  --upstream-archive /path/to/bevy_core_pipeline-0.18.1.crate \
  --rustc /path/to/rustc
```

The optional `--rustc` runs only the tiny extracted CPU selections. Omit it for
archive/source checks alone. Existing app/render feature definitions, camera
producers, analytical sanitizer, checker source, CI workflows, recipe/pin files,
historical rights records and release gates are unchanged.

## Independent review and remaining work

Independent review approved the final module at SHA-256
`e14d4564197bccdd994211c078dfe077b809315c211eb317ab25264622aacbfd`,
with no outstanding source findings. The exact archive comparison and extracted
selection report were rechecked at that hash. Review also corrected the text
about the unused `agx` compatibility handle: Bevy's default handle can resolve
to a generic default image; the selection guard still rejects AgX before binding.

Full integration checks remain unrun because the existing Rust target is owned
by separate active work and the environment has a limited disk budget. Once
available, reuse matching cached profile/flags for `cargo check --locked
--offline -j2 -p flightsim-app` in ordinary and explicit analytical modes first.
Actual render-plugin tests, compiled feature/fingerprint/dep-info evidence,
final executable/source-package payload inspection, native captures, fresh
whole-target source/notices/platform review and normal release gates remain.

The unchanged current acceptance checker requires all three original source
LUTs for its existing recipes. It must refuse this new source subset; passing
its graph-only validation does not make a changed recipe accepted. No checker
pin migration or package clearance is supplied here. Previous green ordinary
or analytical artifacts cannot attest this modified dependency.
