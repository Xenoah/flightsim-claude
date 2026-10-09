# Explicit Tony/Filmic source and build audit

This is a separate source identity, `bevy-0.18.1-tony-filmic-source-v1`.
It does not change what `ordinary` and `analytic` mean. Ordinary requires both
Tony McMapface and Blender Filmic payloads in the executable and excludes AgX.
Analytical requires all three original payloads to be absent. The independent
Swift analytical build recipe and its commercial-staging feature requirement
remain unchanged.

The historical default is named `bevy-0.18.1-upstream-three-lut-source-v1`.
Its ordinary control still requires all three source and executable payloads;
its analytical audit still requires all three original source references and
excludes all three executable payloads. Omitting the source-recipe argument
selects only that historical behavior. There is no implicit two-or-three-LUT
acceptance. Historical receipts do not attest the modified dependency.

## Source identity and private reference

`scripts/tonemapping-two-lut-source.json` pins the reviewed 55 regular files
under `vendor/bevy_core_pipeline`, including the modified module, unchanged
shaders, retained payloads, provenance, modification records and notices. The
auditor pins that manifest's SHA-256. Extra or missing files or directories,
symlinks, source mutations, mismatched supplemental notices, another Cargo
patch route, or a registry-style core lock entry fail. Other repository source
is covered by the enclosing source-admission contract, not this vendor manifest.

The excluded AgX reference comes only from the complete original private
`bevy_core_pipeline-0.18.1.crate`, SHA-256
`4d0810e85c2436e50c67448d48a83bf0bb1b5849899619ae2c7ea817221e9172`.
Its actual compressed size is 628451 bytes and its inventory has 49 files.
The archive must be outside the source checkout. The source CI runner also
requires it outside the evidence and build directories; the workflow uploads
only `tonemapping-evidence`. Never add the archive or extracted AgX bytes to the
source package, artifact bundle, evidence upload, or distribution.

The reader rejects noncanonical paths, symlinks, special files, incorrect hashes,
unsafe/duplicate archive members and inventory differences. Reads are bounded:
2 MiB compressed archive/member/source files, 4 MiB decompressed archive,
64 archive members, 128 vendor entries, 64 MiB graph/messages/dep-info/fingerprint
files and 2 GiB per executable or library. The archive is read in memory and is
never extracted. This is exact-payload exclusion evidence; it is not a detector
for arbitrary transformed or independently re-encoded content.

## Build evidence API

The existing Python API remains callable without additional arguments. New
source evidence must explicitly call:

```python
audit(graph_path, messages_path, mode, kind,
      recipe=None,  # Or the existing explicit Swift analytical build identity.
      source_recipe=TWO_LUT_SOURCE,
      source_root=absolute_checkout,
      upstream_archive=absolute_private_archive,
      target="x86_64-pc-windows-msvc")
```

`mode` is `ordinary` or `analytic`; `kind` is `app` or `sun-clock`. The supported
exact targets are `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`.
`validate_source(source_root, upstream_archive)` performs the bounded source-only
checks and returns an evidence dictionary. It does not invoke Cargo or execute
an application. `select_artifacts` additionally accepts the independent
`source_recipe` and `source_root` keyword arguments.

For command-line build auditing:

```sh
python3 scripts/check-tonemapping-build.py \
  --source-recipe bevy-0.18.1-tony-filmic-source-v1 \
  --source-root /absolute/checkout \
  --upstream-archive /absolute/private/bevy_core_pipeline-0.18.1.crate \
  --target x86_64-unknown-linux-gnu \
  --mode ordinary --kind app \
  --graph /absolute/evidence/graph.txt \
  --messages /absolute/evidence/messages.jsonl
```

The new audit requires one successful Cargo completion, one final executable,
one library artifact for each of the six dependency packages and the render
routing library, and exact mode features. Core, render and executable Cargo
path package identities must identify the admitted checkout's exact roots and
versions. Cargo's named and directory-implied fragments are both supported;
arbitrary path packages and registry disguises are not. The core graph source
route and target source path must identify the same vendor directory.

Each selected library must be in the executable's exact target/profile tree.
The target directory and ELF/PE x86-64 headers must agree with the requested
platform. Library and executable fingerprints must match graph features and
`-D warnings`; ambiguous executable fingerprints fail. Core dep-info must name
the admitted library/module source and exactly both local retained LUT inputs
in ordinary mode, or none in analytical mode. Checkout-relative Cargo paths are
resolved against the explicitly admitted source root; traversal, symlinks, AgX
and foreign source routes fail.
The result records source, graph, Cargo-message, executable, library,
fingerprint and dep-info hashes plus exact payload offsets. Source/build
reports always retain `release_admitted: false` and `native_qualified: false`.

The enclosing native collector remains responsible for authentic command
capture, fresh target creation, compiler/profile pinning, Cargo metadata
closure, artifact freezing and any broader platform requirements. The small
standalone audit does not retroactively prove a target's creation history.

## CI and validation limits

`ci-tonemapping.py` now requires `--upstream-archive`, validates the explicit
source identity before build work, and creates fresh, disjoint evidence and
build directories. Tests, checks and Clippy use QA scratch targets. The runner
records directory identities and discards only those newly created QA directories
after rejecting replacement directories, symlinks and junctions before compiling
into separate fresh final targets, keeping at most two Bevy target trees on disk
and preventing leftover executable fingerprints from obscuring artifact selection.
Both final app audits select the new source recipe and
exact Linux target. All existing analytical tests, decoder features, mixed-mode
rejections, neither-mode rejections and render-library-only check remain.
Compiler/rustflags overrides that could obscure the selected flags are rejected.

Synthetic tests cover both modes and both target headers, source/package-route
spoofing, missing and ambiguous artifacts, cross-target paths, payload mutation
and exclusion, dep-info changes, root/render fingerprint mismatch, exact source
membership and notices, private-reference corruption/bounds/unsafe members,
historical strict-three-LUT controls and CI's private-reference/upload split.
Synthetic binaries and fabricated Cargo records are negative-witness fixtures,
not real build evidence. The exact local source and original private archive
can be checked without compiling or running Bevy.

No new Cargo builds, native runs, rendered-image comparison, performance claim,
whole-target rights acceptance, release qualification or publication approval
is established by this tooling migration. Those gates require their own fresh,
authentic evidence for the final admitted source identity.
