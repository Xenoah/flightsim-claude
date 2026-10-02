# Mixed-LOD terrain bridge verification — 2026-10-02

## Defect and scope

The previous default 33×33 mesh skirts use max(20 m, 4×DEM height error).
That does not account for coarse spherical/ellipsoidal chords or arbitrary
neighbour LOD gaps. The actual default selector can put L3(1,4) beside
L7(32,68) at camera (-5.9765625°, -133.5°, 12,000 m), with the default
16 px / 1080 px / 60° / max-level 13 / root-error 20 km settings and a zero
local surface reference. A post-f32 triangle probe measured a 99.270165 m
aperture between the fine skirt bottom and coarse top, with no hit along a
200 m transverse segment through the aperture. A skirt-centroid control hits.

This repair joins the actual displayed edges. It does not change DEM values,
source priority, ground sampling, FDM/replay state, mesh surface vertices or
the separately unresolved runway-to-rendered-terrain draping mismatch.

## Geometry contract

`TerrainBoundary` retains the four post-f32 source edge polylines plus source
origin and attributes. Integer quadtree edge sweeps find adjacent intervals,
including distinct prime-meridian and dateline edges between root tiles.
`TerrainSeam` merges knots from both edges and triangulates their ribbon.
Source steps and crossings are handled by two-sided indices rather than an
assumed side of the terrain material. T/cross-junction endpoint fans share an
anchor inside the convex hull of incident source points. Each ordinary endpoint
also includes the complete at-most-four-point incident fan in one local frame;
its links follow actual neighbouring edge pairs, never row-major tile order.
Poles have a shared
anchor and once-per-tile polar fans, including post-f32 polar residuals.

Each bridge uses the finer tile origin. Its final local f32 reencoding has an
explicit conservative error bound; no claim of bit-identical welding after
independent floating-origin GPU transforms is made. Tests inspect actual final
triangles and quantify this representation error, rather than using DEM heights
as a proxy for mesh coverage. Original skirts remain available for small
representation differences. There is no fixed exaggerated skirt depth or lift.

## Streaming and ownership contract

- Surface preparation and bridge preparation share the existing frame mesh
  budget; source read attempts retain their separate existing budget
- A new visibility cut is staged while the prior cut and prior bridges remain
  displayed. While staging, selection pauses and the full mesh budget goes to
  the bridge queue; this prevents preparation starvation
- All new surfaces/bridges are hidden until one Commands batch commits the cut
- A same-ID global-to-primary replacement retains its old visible surface and
  old bridges until commit. Actual surface generation IDs and shared-corner
  descriptor equality invalidate affected bridges, including third neighbours
- DEM eviction does not discard edge data. Unchanged bridges are reused
- Existing WorldPosition/WorldOrientation transforms handle rebases; new meshes
  get the current RenderFrame immediately, including mid-transaction rebases
- Explicit new-flight relocation drains every tile, retained old replacement,
  hidden/visible bridge, descriptor, boundary, and pending transaction

Planning is O(N log N) time and O(N) space over at most 8,192 selected/resident
tile IDs; it is one topology plan per cut change, not an unbounded pair
scan. The incremental renderer path now advances that plan across updates with
a fixed 1,024-unit work budget; see the separate
[incremental planning evidence](terrain-seam-planning-2026-10-02.md) for its
scope, total latency and remaining synchronous assembly/commit/reset costs. A non-overlapping cut has at most 4N adjacent-pair bridges. Surface assets
may temporarily include one preparation batch of outgoing assets beyond the
selector's 8,192-ID residency ceiling (at most another 8,192 in the extreme
configuration). Old and new bridge sets coexist only until the pending cut
commits, so at most 8N bridge assets are resident. Each default boundary stores
132 vertices (four 33-point edges). A default bridge has a conservative bound
of 266 vertices / 1,536 indices including two windings and all possible polar
fans. These bounds are independent of the LOD difference and DEM cache size.
`TerrainTiles::resource_usage()` exposes tile/retired/bridge counts, retained
boundary counts/bytes, geometry vertex/index counts and logical mesh bytes.
The byte metric excludes graphics-driver copies and Bevy bookkeeping.

## Verification status

At 04:34 UTC the independent reviewer linked the new world rlib into its
existing standalone actual-triangle probe. All six original flat/bundled cases
(L3/L4, L4/L5 and the actual-cut L3/L7 pair) changed from zero aperture-segment
hits to two hits in both ray directions. For bundled L3/L7, the aperture target
was 0.000460096 m from the stitched triangles, inside the 0.013819776 m explicit
f32 reencoding bound. Bundled L3/L4 measured 0.009835974 m inside a 0.110047983 m
bound. Source/log are retained as `mixed_lod_stitched_probe.rs/.log` in the task
QA workspace. These are independent geometric checks, not screenshots.

A broader independent post-f32 probe at 04:40 UTC checked 34,002 interior
ribbon rays over Pacific, dateline, north-pole and south-pole default cuts:
zero misses. Polar cuts each had 4,094 leaves / 8,185 seams and were truncated
by the existing selector bound. Its largest measured residual was 0.258143 m,
inside the maximum 0.759141 m frame-encoding bound. Separations below 2 m were
intentionally skipped; this is not an exact sub-metre welding test.

The first targeted geometry suite exposed a separate approximately 3 cm gap
between ordinary T-junction cap spokes reencoded in different ribbon frames.
The unchanged failing spoke ray (weights `[0.1, 0.8, 0.1]`) is retained in the
regression test. Duplicating
the complete small incident fan in each ribbon closed that internal spoke gap;
all ten initial geometry tests then passed. Source-edge attachment and separate
polar-fan representation limits remain explicitly bounded, not called exact.

At 04:43 UTC, strict-warning package tests passed:
- `cargo test -p flightsim-render`: 241 passed, 1 pre-existing ignored
- `cargo test -p flightsim-app`: 111 passed, including actual map-start cleanup
  of a pending same-ID replacement, repeated at the dateline and south pole

The renderer tests include shared mesh budget, same-ID atomic replacement,
refinement across several preparation frames, rebase during staging, complete
cancellation cleanup, 20 source replacements without residency growth, reuse of
unchanged bridges, and the actual default mixed cut reaching completion with a
one-tile DEM cache and a one-mesh-per-frame budget. All had zero failures.

At 04:45 UTC, `cargo test -p flightsim-world` passed 227 tests plus one
doctest; all-target world Clippy passed with `-D warnings`. Eleven dedicated
geometry regressions cover the original aperture, arbitrary LOD0..13-to-24 and
18/31-vertex edge resolution differences, signed/crossing source heights,
T/cross/dateline junctions, both poles, content invalidation, and an 8,192-tile
cut. That full cut produced 16,256 neighbour ribbons (within 4N), retaining
65,536 edge vertices with the test's two-point edge resolution. A production
33-point boundary occupies 5,424 bytes including its allocated edge storage.

Additional measured attachment reencoding errors / conservative bounds, in m:
- L0: 0 / 1.315517142
- L3: 0.048424992 / 0.173192076
- L7: 0.002767372 / 0.009370285
- L13: 0.000043186 / 0.000155842
- L24: 0.000000085 / 0.000053654

Final exact-source package reruns at 04:49 UTC passed: world 227 tests plus
one doctest, renderer 241 tests (one pre-existing ignored), app 111 tests.
All-target Clippy for world/render/app passed with `-D warnings`, affected-package
rustdoc passed with `RUSTDOCFLAGS=-D warnings`, architecture checks passed,
`cargo fmt --all --check` and `git diff --check` passed. All six original
flat/bundled aperture cases are included in the permanent regression suite.

The lead's integrated native/offscreen screenshot gate remains pending because
its working branch contains additional startup fixes. No benchmark or
rendered-image success is implied by these numerical and Commands/Assets tests.
The new source is neither published nor evidence of a released binary.
