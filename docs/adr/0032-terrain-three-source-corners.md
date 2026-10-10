# ADR-0032: Preserve triangular source corners without a redundant subdivision

- Status: implemented candidate; full application and driver qualification remain separate
- Date: 2026-10-09
- Related: [terrain centroid interpolation](0031-terrain-centroid-interpolation.md)

## Evidence

Centroid interpolation substantially reduces the original Balzers stitch-color
artifacts but does not remove every residual. Two recorded source junctions have
three vertices: two about 0.983 mm apart and a third about 916 m away. The source
triangle has about 0.3561 square metres of area. It is genuine geometry, not an
exactly zero-area face to discard.

The previous builder subdivides this triangle around an independently computed
mean. Reencoding the mean and endpoints into local f32 coordinates can move the
mean off the encoded source plane. The added mean also carries a separately
normalized normal and a palette color evaluated from its averaged elevation and
slope. Those attributes are not generally affine combinations of the source
vertices' final attributes. The distorted subdivision therefore combines extreme
aspect ratios with steep transverse attribute gradients.

A bounded standalone wgpu27/Vulkan llvmpipe diagnostic reproduced extreme
perspective-centroid fragment inputs at high-motion frame200, pixel(862,281), and
night frame257, pixel(1086,218). The reconstructed captured-order winners were
mesh83/triangle135 and mesh202/triangle135. Their pre-lighting RGB values were
approximately (0.675,-0.745,-0.739) and (2.048,-2.973,-1.649), despite ordinary
source vertex colors. Actual GPU vertex outputs were saved. Reversing mesh draw
order and removing vertex-capture writes did not change these target results or
full-scene depth.

The diagnostic matches every recorded native terrain depth bit outside one
nearer aircraft-shaped component in each image; that aircraft is intentionally
absent from the terrain-only probe. All 648 samples in the two 9-by-9 target
neighborhoods match. This establishes strong fidelity for these terrain states,
not identity of the complete production Bevy shader, draw scheduling, or final
PBR/tonemapped color.

## Decision

A corner may use its direct source triangle only when all of these hold:

1. It contains exactly three source vertices
2. Its canonical source links are exactly [0,1], [0,2], [1,2], a closed cycle
3. The encoded direct triangle passes the existing exact nonzero cross-product
   test used by MeshBuilder; there is no positive-area threshold

Emit both windings of the complete triangle in every incident ribbon. Repetition
is retained: sharing only one piece per ribbon previously opened an approximately
3 cm spoke gap when different local origins rounded the pieces differently.
The complete f64 three-source polygon is already a triangle; its mean subdivision
is not an additional source boundary.

If the direct triangle encodes to exactly zero area, retain the previous fan.
Some source triples collapse in all incident frames while their independently
rounded mean still produces nonzero faces. Two passing views would not justify
removing that existing finite-precision coverage guard.

Open three-source paths, four-source corners, and polar fans retain their previous
construction. In particular, an L-shaped incomplete cut must not acquire an
unrequested closing edge merely because it has three sources.

Keep the stored anchor and the existing vertex push order. Vertex positions,
normal/UV/elevation/slope storage, bounds, planner identities, and palette
construction remain on their existing paths; only selected index lists change.
The now-unused anchor is intentional, retaining layout and avoiding an unrelated
packing/budget change. Removing its contribution from emitted faces changes
interpolated shading on those faces; this is not a full-frame color-parity claim.
No DEM, physical terrain, LOD selection, replay state, MSAA setting, or broader
material contract changes.

## Bounded coverage result and limits

Both captured scenes were checked with every closed-three-cycle replacement,
including exact-degenerate fallback: high has 114 such cap copies in 81 meshes;
night has 174 copies in 129 meshes. The fallback retains 46 and 49 degenerate
copies respectively. All 96 high and 154 night baseline seam position/index
arrays were first matched bitwise from the retained source-mesh data.

Across all 7,372,800 full-screen before/after MSAA samples, the guarded control
has no newly uncovered or newly covered samples. Only the known target sample3
depth changes in each scene; ordinary backing ribbon triangles then supply
ordinary color and normal interpolants. The independent CPU reference also
preserves full-scene coverage under f64 and staged-f32 transforms, with and
without nearest1/256-pixel snapping.

This is exact-state bounded evidence. Other viewpoints can expose changes in
encoded surface shape and interpolated attributes. Production lighting, the complete night/high-motion replay windows, and the
separately retained baseline partial-sample depth exceptions remain unverified
by this two-state experiment. It does not characterize other drivers or Windows;
existing project release checks remain separate. This introduces no new physical-
GPU requirement. The centroid-only candidate remains a distinct partial
improvement. Neither it nor this diagnostic closes issue6's existing acceptance
criteria.

## Rejected alternatives

- An area/altitude cutoff would remove real source coverage and has no justified
  threshold
- Clamping colors would conceal the interpolation mechanism and alter materials
- Averaging only the mean's palette color is a useful read-only causal control,
  but leaves the extreme interpolated normal. Averaging raw normals is undone by
  the existing per-vertex normalization in the production vertex path
- Applying the direct triangle to every three-source path would invent a closing
  edge in incomplete cuts
- Dropping all encoded-degenerate direct corners would remove surviving rounded
  fan guards outside the two inspected views
- Removing repeated caps or changing four-source/polar triangulation broadens
  the change beyond the diagnosed redundant triangular subdivision

## Regression contract

The T-junction integration fixture requires the full direct source triangle in
each incident ribbon, with opposite windings and no mean-anchor faces. Historical
spoke-gap rays and the dateline variant remain intact. An actual L-shaped cut
checks open-three-source exclusion through both synchronous and budgeted planners.
Recorded numerical unit fixtures cover the submillimetre positive source polygon
and an exactly collapsed direct triangle with a surviving nonzero fan. Existing
four-way, polar, arbitrary-LOD, boundary-reencoding, and planner-budget regressions
remain required.

Runtime and geometry-test files participate in explicit source admission. A
separate migration must preserve historical source witnesses and identify these
new bytes; passing local geometry tests does not authorize silently rewriting
historical source pins or publishing a release.
