# Exact displayed-facet overlay contract

This is an opt-in render API. It does not modify DEM samples, `GroundSampler`,
runway definitions, FDM contact or replay state. App integration and final actual
scene acceptance must be verified separately from these CPU tests.

## Why vertex draping was insufficient

A bilinear height field and its two rendered triangles are different surfaces.
The regression saddle has a 50 m discrepancy at the cell centre. Resampling only
a pavement's corners cannot preserve clearance where a rendered crease crosses
its interior. Arbitrarily raising all pavement is not the solution.

`TerrainOverlay` retains an immutable footprint and authored layer offsets.
Surface triangles are intersected with the actual post-f32 terrain surface and
bridge facets in a local f64 tangent frame, then re-triangulated at every crease.
Ground support has an explicit 60-degree slope limit. Near-vertical seam walls
still render as terrain but are omitted from individual overlay intersections;
they cannot force the rest of a multi-kilometre road/landcover batch invisible.
The diagnostic `omitted_support_facets` counts steep candidate occurrences,
including repeated candidates in separate overlays. No ground is invented in
unsupported portions, and this is not a complete overlay of cliffs/seam walls.
Those walls still exist and can occlude adjacent overlay/lamp views; supporting
floor depiction is distinct from universal clearance against every triangle.
Paint colours and ordered small offsets are preserved. Airport builders supply
`ATTRIBUTE_OVERLAY_LIFT` so coarse source-node interpolation is never mistaken
for an intended above-ground offset. Legacy meshes may supply the original
physical elevation callback at registration. Fixture blocks retain their solid
shape and are supported over the maximum height across intersecting facets,
including an interior crease, rather than just the four corners.

This makes the depiction follow the displayed terrain. It does **not** make
coarse visual terrain agree with physical contact height. That independent
near-ground consistency limit remains, as do unsurveyed/coarse source data.
`overlay_usage().maximum_authored_displacement` reports the largest absolute
shift from the original authored mesh's interpolated ground surface. It is a
useful warning metric for real-airport QA, not an exact resampling of physical
contact at every fragment.

## Transaction and bounds

The existing terrain cut is retained while immutable source discovery and
clipping run on the compute pool. Discovery uses at most 40,960 bounded metadata
records (8,192 surfaces plus their existing 4N seam bound), without deep mesh
copies. At most two relevant terrain meshes are copied per update, additionally
charged against the existing mesh-preparation budget. The source generation is
frozen until airport/scenery mesh contents and terrain visibility commit together.
A same-ID source replacement invalidates the cache; distant unchanged airport
facets reuse their previous overlay geometry. Cancellation is checked during
source discovery, facet conversion, source triangles and fixture blocks.

Hard limits:

- 64 total registrations; 24 optional scenery registrations
- 65,536 aggregate source triangles; 40,000 optional source triangles
- 524,288 aggregate source vertices and 524,288 aggregate output vertices
- 512 nearby source-mesh records; 262,144 copied terrain vertices
- 32,768 candidate facets for one overlay
- 4,000,000 aggregate facet-conversion/intersection work units per transaction

A BVH bounds candidate queries; source-tile footprints reject unrelated facets
before conversion. Essential airports receive metadata/copy/work capacity first
and see only required terrain copies. Optional registry admission is atomic, but
its prepared batches fail independently in caller-supplied priority order (the
app supplies nearest first). A failed optional batch is hidden without erasing
completed airport or optional outputs. Later smaller batches can use the
remaining aggregate output capacity. Failed work stays charged; work exhaustion
stops remaining optional attempts while retaining earlier successes. Optional
snapshot failure still disables the cohort because no trustworthy frozen source
is available. `omitted_optional` counts omitted batches for the committed cut,
separately from the rebase-dependent precision gate. Essential failure hides that
whole required group for the cut and allows terrain streaming to proceed rather
than deadlocking. New terrain and whichever complete batches were admitted still
switch together; no partially clipped individual mesh is displayed.
The bounds are not a 60 fps or real-time throughput guarantee.

## f32 transform precision is a separate gate

Canonical source triangles are not identical to their separately transformed
GPU positions. Coarse roots can have million-metre local coordinates. Independent
CPU emulation of the real `Affine3A` quaternion/translation path found that even
an 0.08 m layer can be buried on a level-zero parent. That is not repaired by
f64 clipping.

Every terrain update, including origin rebases with unchanged tile signatures,
checks a conservative bound from the actual f32 quaternion-matrix discrepancy,
reference-basis rounding, affine evaluation, translation and output encoding.
Only sources actually intersected by the overlay enter this gate. The minimum
clipped-facet support cosine converts the authored vertical lift to its smaller
normal-direction clearance on a slope. A mesh is hidden when the bound consumes
at least half that clearance, and can reappear when the current frame has enough
headroom. The renderer never substitutes an arbitrary large lift. A real-affine
regression covers Tokyo, Alps, Quito, dateline and both polar regions at levels
0/4/8/11/13, including rebases; admitted layers remain above their corresponding
facets in the CPU checks. This is deliberately conservative and may omit coarse
or far detail. Nearby non-clipped facets, GPU rasterization and depth-buffer
precision remain separate acceptance limits.

## Ownership and dynamic scenery

`TerrainTiles::register_overlay` manages an essential mesh already owned by the
caller. New entities should start hidden. Draping replaces `Mesh` contents under
the original handle; it does not create a second scenery/airport handle.

`replace_optional_overlays` admits a complete hidden scenery cohort and returns
`TerrainOverlaySwap { revision, retired }`. Rejection leaves the old registry
unchanged. Only one optional swap may be outstanding. Keep returned old targets
visible until `overlay_usage().committed_revision >= revision`, then remove
those handles/despawn their entities in a system after terrain update. Retained
old targets continue receiving precision checks until the matching commit.

On explicit relocation/reset, call `clear_optional_overlays` first. It cancels
pending work and returns both staged-new and retained-old optional targets for
caller cleanup exactly once. Essential airport registrations are retained.
`unregister_overlay` also handles retained optional targets and must precede an
individual caller-owned despawn. `TerrainTiles::drain_all` owns only terrain,
retired terrain and seam assets: it cancels overlay work but does not return or
despawn caller-owned airport/scenery meshes. Avoid duplicate cleanup against the
caller's own ground-mesh lists. Solid scenery remains outside this API.

## Verification scope

Regression tests cover whole-fragment saddle clearance, paint separation,
fixture crest support, both windings of a bridge, dateline/high-latitude encoding,
deterministic output, source replacement atomics, bounded copying, zero budget,
dynamic dirty generations, unregister/reset cancellation, optional admission and
failure isolation, repeated asset replacement, and frame-dependent precision
hiding/recovery. Software Vulkan/native cockpit, shallow approach, overhead,
day/night, distant lighting and regional-scene acceptance remain integration
requirements. No Windows/vendor-GPU performance or universal clearance claim is
made by these tests.
