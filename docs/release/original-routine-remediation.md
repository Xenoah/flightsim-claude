# Original routine remediation: reviewed source proposal

Base: `3b098bc1d20cc7cc99ae85896080d22b688ac6c3`. Prepared 2026-10-08 by
OpenAI assistants. This is an engineering source/licence assessment and technical
review, not a human legal opinion, title warranty, native attestation or release
approval. The authors and independent reviewers used functional specifications
and mathematical references without reading the prior implementations. Their
records describe that boundary honestly; no formal clean-room certification or
training-data provenance guarantee is asserted.

## Two original replacements

- `zune-jpeg 0.5.15`: replace only the eight-row AVX2 transpose, its three internal
  macros and obsolete private shuffle helper. Preserve all 32-bit lane payloads,
  the eight-reference API and JPEG features. Reviewed replacement SHA-256:
  `109392a2563197b153f4ad6d3a521a095b214b5ccbed710578cabc523a344592`.
  Upstream's original `MIT OR Apache-2.0 OR Zlib` declaration remains in
  `Cargo.toml.orig` and provenance. The modified package offers `MIT OR Apache-2.0`
  to match the new project-authored function/tests; it does not invent a Zlib
  grant for that replacement. Original licences for retained code stay present.
- `bevy_pbr 0.18.1`: replace only `parallaxed_uv` and its old origin-specific
  comments. Preserve imports, actual explicit-LOD sampling helper, bindings,
  registration, callers, signature and meaningful occlusion/relief algorithms.
  Replacement SHA-256:
  `8fa277d6bfefdda361d9cfe2274a0dc54d509fa603d35236aa7816f9cf095a58`.
  Complete module SHA-256:
  `65226a61e28f9f7b593e7a59d983a8b52e8c5ced952156e2977774dd5687baf1`.
  The package's `MIT OR Apache-2.0` expression is unchanged.

The two identified source-origin questions are resolved for these modified
implementations by replacement, not by inventing a prior author's grant. This
does not retrospectively clear the original registry implementations.

## Actual notices and identities

The documentation-only Stanford-dragon preview is omitted from the vendored
source; its rustdoc image line becomes an omission note. No runtime/include_bytes
reference exists and optional meshlet functionality is unchanged. The omission
and original image hash are retained in provenance; no image grant is invented.

All original root licences and retained-source notices are preserved. Both
packages add the existing project's licence texts and prominent modification
notices. zune includes the established Stanford individual-code public-domain,
stb MIT, libjpeg-turbo/IJG and pinned libultrahdr Apache notices, the IJG executable
acknowledgement and observed Rust adaptation note. The separate Adobe notice is
retained without claiming product-level patent clearance. Bevy retains
Bruneton/INRIA and raymarch-source notices. Its optional blue-noise asset matches
32 EA fastnoise PNG layers exactly; the primary EA BSD-3-Clause licence and
source/binary retention notice are added. NVIDIA STBN terms do not apply to this
identified EA texture. The unchanged collector captures all
25 named modified-package notice files with exact hashes. Generic licence text
is never treated as a new upstream grant.

`analytical-modified-source-provenance.json` binds every vendor file, original
archive checksum/revision, added-vendor patch and modified path. Original
`.cargo_vcs_info.json` files are omitted because upstream revisions identify
origins, not current modified trees. The six-field source-header manifest has
510 exact bindings: the obsolete full transpose excerpt is removed, retained
modified-file headers are rehashed, and two new SPDX spans are added.

The patches add complete modified vendor trees to a production base where those
paths do not exist. They contain no deleted original routine. Original crate
archives, caches, old routine text, native binaries, decoded pictures, full
third-party discussions and git history are excluded from remediation evidence.
Original source hashes and URLs preserve traceability.

The actual lock update changes only the two registry entries to path sources.
Linux-hosted Windows-filtered metadata has the same 448 available package
identities, all feature maps/dependency edges, and 13 workspace members. The
conservative normal/build collection remains 359 packages with no tone LUTs.
These observations are not native MSVC metadata.

## Executed checks

- Transpose: seven debug/release test groups, all 64 lanes and 2,048 individual
  payload bits, 20,000 deterministic random matrices, involution and scalar
  reference. Independent review added 14,114 matrices per build with scrambled
  distinct allocations/canaries
- Integrated zune: 14 debug/release tests, including retained upstream IDCT and
  progressive-JPEG cases. Twenty project JPEG fixtures produced exactly equal
  66,810,240 channel bytes in independently built scalar and x86-feature modes
- Parallax math: 22 debug/release tests and nine independent oracle tests with
  10,000 analytic cases
- Actual shader imports: six genuine modules; eight ordinary/bindless ×
  interpolation/relief × storage/uniform combinations; seven rejection controls;
  Naga-Oil composition, full Naga validation, binding checks and WGSL round-trips
- Modified analytical+commercial app: locked offline Rust 1.93/Linux cargo check
  passed. Source-archive policy has 15 passing adversarial/unit tests, including ZIP/TAR ambient-attribute override rejection

Reproduction tools are under `tools/validate-jpeg-replacement`,
`tools/validate-parallax-math` and `tools/validate-parallax-replacement`. The last
uses a synthetic fragment entrypoint and explicit capability assumptions, not a
full renderer or measured GPU capabilities. The shader deliberately bounds
finite-input grazing at 1e-4, coarse layers at 1,024 and refinements at 24. It finds
the first sampled bracket; sub-layer excursions can be missed. Secant exactness
requires an affine residual throughout the bracket. These are documented
semantics, not old/new pixel equivalence. No performance comparison against the
removed routine is claimed. Inherited upstream trailing whitespace is retained;
newly authored files are checked separately.

## Remaining qualification

The rest of the substantive dependency review, including CORE-MATH/LLVM notices
and accepted constgebra/hexf assessments, must be genuinely bound and integrated.
Old registry inventory/source admission cannot be relabelled as covering path
patches. A separately reviewed source-contract migration, actual fresh MSVC
capture and modified-tree projection, runtime/platform obligations, complete
inventory review, GPU/runtime/appearance acceptance, hosted ZIP/tar verification,
user choice of the reduced deliverable and publication receipt remain required.
The existing ordinary two-aircraft release gates are unchanged and still block.

A broader analytical app test build was attempted after the targeted checks.
Compilation reached the app link, but the linker terminated with SIGBUS while
the executor filesystem was full. The test binary never ran. That stage is an
infrastructure-blocked attempt, not a test pass; native/CI regression remains
required. Source and evidence were retained, and only the failed app-test's
regenerable temporary/cache outputs were reclaimed.
