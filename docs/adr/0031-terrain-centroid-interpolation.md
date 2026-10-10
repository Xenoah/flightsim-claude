# ADR-0031: Keep forward terrain interpolation inside MSAA coverage

- Status: implemented candidate; native qualification is separate
- Date: 2026-10-09

## Evidence and cause

The Balzers midnight westbound replay reproduced isolated one-pixel color
flashes on thin stitch triangles with production 4x MSAA. The projected area of
one offending triangle was about 0.00572 square pixels. A multisampled triangle
can cover a sample without covering the pixel center. Default center sampling
then extrapolates vertex color, normal and world position outside that triangle,
so valid mesh values can become extreme fragment inputs.

An 81-frame diagnostic A/B run on Vulkan llvmpipe, changing the three varyings
to perspective-centroid interpolation, removed all 19 smooth-depth boundary
color candidates. All 298,598,400 native depth samples were byte-identical, as
were recorded physics, camera and light metadata, and the 209 captured terrain
meshes' positions, indices, normals and colors. This diagnostic supports a
shading cause; it is not physical-GPU qualification or proof for other routes.

## Decision

`TerrainDetail::specialize` adds `FLIGHTSIM_TERRAIN_CENTROID` to both vertex and
fragment shader definitions. The existing vendored `forward_io.wgsl` uses that
definition to select `@interpolate(perspective, centroid)` only for world
position, world normal and vertex color. Both terrain tiles and their stitch
bridges already use `TerrainMaterial`, so their interfaces agree. Optional UVs,
tangents and flat instance fields retain their original interpolation.

Centroid interpolation selects a covered location when the pixel center is
uncovered. It preserves perspective correction and does not change triangle
coverage, clip-space positions, geometry, normals stored in meshes, depth,
MSAA settings, LOD decisions, physical ground or replay state. The two stages
must receive the same definition; changing only the fragment shader is not a
valid portable interface contract. These defaults and stage-matching rules are
specified by [WGSL interpolation](https://www.w3.org/TR/WGSL/#interpolation).

Other materials do not opt in. This includes StandardMaterial aircraft and
airport surfaces, and the separate optional WaterMaterial. The ordinary default
water quality uses TerrainMaterial. The opt-in is inert for prepass/deferred
shaders, whose separate `prepass_io.wgsl` stays unchanged. The current forward
MSAA defect does not justify changing those interfaces or lighting modes.

## Alternatives and costs

- A global centroid override was useful for diagnosis but would change every
  material. The material-scoped definition avoids that broader change
- Disabling MSAA would hide the sample/center discrepancy while changing image
  quality and coverage. It is not the selected fix
- Clamping fragment inputs would conceal extrapolation and change shading
  semantics. Modifying stitch geometry would disturb already passing coverage
  and mesh/physics separation

Centroid evaluation can change boundary shading and position derivatives. It
does not promise pixel parity at silhouettes, a performance improvement, or
elimination of every possible terrain artifact. Native moving-scene acceptance
and Windows driver validation remain required separately from CPU shader tests.
Separately observed single-MSAA-sample coverage outliers remain under
investigation; this shading correction does not close the broader terrain
continuity acceptance criteria.

## Regression contract

GPU-free tests call the actual material specialization with pipeline
descriptors, verify that only the intended stage definitions change (including
vertex-only pipelines and 1/4/8 samples), then use Bevy's actual ShaderCache
preprocessing and Naga validation to inspect both stage interfaces. They require
centroid sampling on all three opted-in forward varyings, center sampling with
the opt-in absent, and unchanged optional/flat fields and prepass/deferred IO.
Both colored and uncolored forward variants are covered. Naga's HLSL and
SPIR-V writers additionally must emit centroid exactly at the intended locations;
this is shader translation coverage, not DXC or driver execution.
The existing terrain system test verifies real tile and bridge material
components. Historical source, native and release evidence remain historical;
an explicit source migration binds the new implementation and tests.
