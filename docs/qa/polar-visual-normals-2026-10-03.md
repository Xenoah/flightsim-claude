# Source-aware polar visual normals

Status: accepted render-only policy after matched South/North captures and the
combined-shadow factorial. Sparse lighting/shadow artifacts remain explicitly
below. Integrated runtime/gate evidence is recorded in
[surface rendering QA](surface-render-integration-2026-10-03.md).
No hardware-GPU or general FPS acceptance is implied.

## Fixed rendering policy

The app supplies its validated immutable global atlas together with the selector's
explicit Primary/Fallback identity. That identity follows the successful read and
same-ID replacement, never directory existence or numerical height resemblance.
Primary/regional DEMs bypass this filter.

For an affected fallback surface vertex, four source heights are sampled through
core LocalFrame/ECEF transforms, at +/-505 m north/east from its unchanged elevation.
Their cross product gives a filtered ECEF visual normal. There is no fifth centre
sample, second tangent-frame construction, cache, IO, source search or clock input.
Every exact-pole duplicate uses canonical longitude zero. The policy fully applies
inside 75% of each canonical polar source cap and smoothly fades over the outer
quarter. Surface normals are copied to corresponding skirts. Filtering precedes
boundary extraction, so bridges inherit normals without changing geometry.

There are no diagnostic runtime modes, CLI flags or hidden toggles. Slopes, palette
inputs, positions, origin, elevations, UVs, indices, source data/fingerprints,
physical contact and replay identity remain untouched. Existing preparation APIs
retain their legacy behavior when no global shading context is supplied.

## Why 505 m and what it means

The FSGT source packs H to metres and N to centimetres. Convex interpolation retains
at most 0.505 m height-packing uncertainty, but geographic longitude spacing contracts
near a pole. At the bundled south canonical row, node spacing is about 30.118 m;
340 of 2048 angular edges contain a one-metre H step. The radial-linear cap keeps
these angular slopes as radius contracts. More tessellation cannot recover smooth
source relief from those packed steps.

A 505 m half-stencil bounds the packing contribution to each central-difference
height-gradient component by approximately 0.001 in a local height graph. This is
not a bound on total normal-angle error, source accuracy, curved mesh error or
physical terrain slope. Filtering also suppresses retained source gradients:
the pinned pre-packing row still contains nontrivial angular relief. This is an
explicit visual approximation, not a claim to recovered surveyed information.

Transition-band normals can retain original per-tile differences. Only vertex
attributes outside the cap are guaranteed unchanged; coarse triangles touching a
modified pole can interpolate the changed shading farther out.

## Existing evidence and limits

- Exact NOAA source downloads matched the preparation script's complete pinned
  hashes, and reconstructed row samples round to every stored H/N channel
- Diagnostic pre-/post-packing angular-gradient RMS:0.006816 /0.013529;
  packing-added RMS 0.011569. Unquantized source maximum remains 0.029959
- Full selected-cut slope-only controls at both poles compared all 4,982,398
  surface/skirt and 659,068 bridge colours bit-for-bit, with zero differences
- Fresh matched South-Pole baseline and normals-only images both finished exit 0
  at 4094 live/displayed/desired surfaces, 8185 visible bridges, no pending work;
  the radial stripes disappear without visible geometry or aircraft regression
- The South pair uses one zero-duration Swift fixture, 350 m AGL, December 21 climate,
  cloud cover 0, identical camera and matched actual material settings. Their
  recorded `--no-surface-detail` switch is unknown and ignored: both ran the
  default detail-on setting. Preserve those original receipts and annotate this
  correction; future captures use valid `--surface-detail on` to match them
- The independent review agreed with the South comparison. This is one software-
  Vulkan scene, not a global visual or hardware-driver guarantee

The production cleanup preserves the diagnostic's normal math and removes slope
modes and their unused tangent-frame work. An optimized standalone comparison
against the pinned dd7dd95 diagnostic checked both complete 4094-tile cuts:
all 4,982,398 surface/skirt normal vectors per pole are bit-identical, all mesh
fields match, and all retained bridge inputs match. Normal-array FNV-1a64 values
are 76b20ae6d948d5fb (South) and bee1fa7fe294b718 (North); direct bit comparisons,
not hash equality alone, establish identity. Unchanged source slope/UV/elevation
inputs and unchanged palette functions preserve the prior complete colour proof.
The six focused production tests and provenance-supersession test pass in both
the standalone source-validated harness and fresh Cargo builds. Strict render/app
all-target clippy, strict docs, architecture, formatting and native build also
pass. The final combined source and diagnostic-phase correction subsequently
passed all 15 local integration gates, including the long globe suites.

The combined four-cascade, 2,000 m sun range preserves stripe removal but has a
sparse lighting/shadow tradeoff. In one fixed image mask excluding HUD/aircraft
rectangles, distance-only versus baseline darkens 69 pixels by at least 5/255
in any RGB channel; filtered plus extended reach versus filtered-only darkens
76. Only 17 locations overlap. The mask is not a semantic render-ID mask, and
RGB code-value deltas are not perceptual or linear-luminance scores. Filtering
is not necessary for these sparse flecks; their locations/response differ in
the combined image. That image also includes overlay-lifecycle changes, so equal
final cuts do not prove identical GPU submission ordering or a unique cause.
Bevy uses the interpolated visual normal for receiver-normal shadow bias, making
an interaction plausible. No specific acne, bridge or geometry cause is proven.
The production policy retains its broad improvement with this limitation; no
blanket bias adjustment or geometry workaround is included.

North combined has no broad seam, hue, horizon or aircraft regression. The nine
recorded bright specks remain unchanged. A private control changing only terrain
UNLIT, with the real palette/fog/geometry/normals retained, removes all nine into
their local colour (within 1/255 of a 5x5 median). This narrows the defect to the
lit PBR path or an interaction with its inputs, not an authored palette spot.
It does not identify a particular lighting term or establish a production fix.

## Performance and regression gates

The prior diagnostic Criterion actual 4094-tile whole-cut median was 0.837528 s
unfiltered versus 1.446674 s normals-only (+72.7%) in the documented dev profile.
It excludes palette work, bridge building, GPU upload/rendering and streaming;
it is neither free nor a worst-frame timing guarantee. The production benchmark
on clean ef971772 repeated the actual 4,094-tile South cut with one mesh live
at a time, after all other FlightSim builds, probes and renders finished:

| Path | Median whole-cut CPU preparation | 95% median interval |
| --- | ---: | ---: |
| Unfiltered | 0.852354 s | 0.845814–0.869193 s |
| Filtered 505 m | 1.468071 s | 1.443499–1.492867 s |

Each path used 10 samples/10 iterations in the documented dev profile. The added
median preparation cost was 0.615716 s (+72.2%) for this entire stress cut. This
includes atlas DEM generation and geometry, excludes palette/bridge preparation,
GPU work, uploads, streaming and frame cadence, and is not a frame-time or FPS
claim. Existing desktop applications were not a controlled system-wide isolation
guarantee. Cleanup did not make the normal filter free or establish a speedup.

Production benchmark, using one retained mesh at a time, 10 samples per path:

```
cargo bench --profile dev -j 2 -p flightsim-render --bench terrain_polar_normals
```

Compare unfiltered and filtered paths with the same atlas and actual 350 m AGL cut.
Do not benchmark during app captures or concurrent builds/probes. Preserve all
source/manifests' content hashes while forcing fresh mtimes before shared-target
Cargo use; another checkout's newer rlib must not stand in for current source.

Tests retain source-provenance supersession with numerically identical DEMs,
geometry/slopes/UV/elevation immutability, finite normalized normals, primary and
nonpolar identity, shared and mixed-L 9/L 10 vertices, unchanged bridge geometry,
canonical duplicate poles across LODs, cap endpoints, and render-origin rotation.
