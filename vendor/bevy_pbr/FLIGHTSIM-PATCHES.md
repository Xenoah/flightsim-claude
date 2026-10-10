# Local original parallax replacement

Upstream package: bevy_pbr 0.18.1. Source revision:
f667c282dad2c1419afb5836ded22a3ec263970e. Archive SHA-256:
a5ab6944ffc6fd71604c0fbca68cc3e2a3654edfcdbfd232f9d8b88e3d20fdc0.
The original and modified package expressions are both MIT OR Apache-2.0.
All original root and retained-source notices remain; additional project licence
copies identify the existing project terms for the new function.

The runtime change replaces the parallaxed_uv function and its origin-specific comments in
src/render/parallax_mapping.wgsl are replaced. The imports, explicit-LOD sampling
helper, ordinary/bindless texture accesses, public function signature, module
registration and call sites are retained. This is meaningful parallax occlusion
and relief functionality, not runtime disabling or a no-op.

The new function was authored from a functional ray equation and interface,
without the author reading the earlier implementation or tutorial. Its isolated
mathematical derivation, independent analytic tests, language validation and
honest provenance are described in FLIGHTSIM-PARALLAX-README.md. This is not formal
clean-room certification or a human legal opinion. The output is marked under
the project's existing MIT OR Apache-2.0 terms; no fictional human copyright
holder or alternative grant is invented.

Compatibility bounds are explicit: finite input domain, normalized view angle,
1e-4 grazing denominator, 1024-layer cap, 24 binary-refinement cap, first sampled
sign crossing and residual interpolation. max_steps=0 retains interpolation.
A coarse layer can miss a sub-layer excursion, as with sampled ray marching in
general. Affine exactness needs an affine residual throughout the bracket, not
an interval crossing a clamped-height kink. These are reviewed mathematical
semantics, not pixel equivalence with the removed implementation.

The original .cargo_vcs_info.json is omitted because its revision does not
identify the modified tree. FLIGHTSIM-UPSTREAM-SOURCE.json preserves exact
original archive/revision/member hashes as provenance only. The deleted routine
text and original .crate archive are absent from vendored source and evidence.
The documentation-only src/meshlet/meshlet_preview.png is omitted because its
specific redistribution grant was not established; its sole external-image
rustdoc line in src/meshlet/mod.rs becomes an omission note. No runtime or
include_bytes use exists, and optional meshlet functionality is unchanged.
All other original members were retained byte-for-byte at that migration; the
2026-10-09 terrain-only forward IO change is described below. The third-party-notices
directory preserves the Bruneton/INRIA and raymarch-source notices for retained
components. These additions do not claim actual final link inclusion.

Complete imported-module/bindless validation, native GPU/runtime appearance,
fresh target metadata and modified-tree binding, full notice integration and
all unchanged release gates remain separate qualification steps. This file is
not a publication receipt.

## Retained optional blue-noise texture

The exact stbn.ktx2 payload matches 32 Electronic Arts fastnoise source PNG RGB
layers, byte-for-byte in numeric order. EA offers its included textures under
BSD-3-Clause; the exact 2023 EA licence and factual provenance notice are supplied
in third-party-notices. This preserves the optional bluenoise_texture feature;
the project’s analytical feature graph does not enable it. NVIDIA STBN terms are
not the source grant for this exact file.

## Terrain-only centroid interpolation, 2026-10-09

`src/render/forward_io.wgsl` conditionally adds perspective-centroid interpolation
to world position, world normal and vertex color. The application-owned terrain
material supplies `FLIGHTSIM_TERRAIN_CENTROID` to both forward shader stages.
Without that define, all original IO declarations remain unchanged. UV, tangent,
flat instance fields and the separate prepass IO remain unchanged.

This keeps covered edge samples from using extrapolated terrain attributes at a
pixel center outside the triangle. It does not alter terrain topology, physics,
DEM values, MSAA settings, or material lighting. GPU/native appearance and any
performance effect require separate evidence; source validation does not supply
that evidence. Upstream origins in `FLIGHTSIM-UPSTREAM-SOURCE.json` remain exact
original-member provenance, not a claim that the modified forward IO is pristine.
The existing parallax replacement, its independent witnesses and its source
authorship limitations are unchanged.
