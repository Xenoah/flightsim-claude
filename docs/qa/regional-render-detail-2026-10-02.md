# Scoped regional render detail: measured opt-in path

## Scope and APIs

`LodSelector::with_near_detail(Meters(radius), minimum_level)` requests a minimum
level for nearby geographic footprints, independent of AGL. It is **off by
default**, capped by the existing maximum level and leaf budget. Exhaustion still
retains a complete nonoverlapping globe and sets `truncated`. Calling
`without_near_detail()` restores the original SSE-only policy. The app must scope
this request to explicit immutable scenery coverage; it is not a global default.
It requests detail, not unavailable source data.

`prepare_tile_with_options` accepts bounded resolution 2..=65 and retains actual
boundaries/counts for stitching and overlay preparation. `prepare_tile` remains
an unchanged 33-point wrapper. An app may choose 65 for level 12+ DEMs whose source
grid has at least 65 samples per axis, using immutable source/pack facts. For the
current fixture/provider, regional grids are 65 and the bundled atlas grids are
33. Dimensions alone are a **format heuristic, not general primary provenance**.
A caller must not change a tile's options according to camera cadence while
keeping its source generation unchanged. No DEM, ground sampler, FDM or replay
data are altered.

## Regional fixture probe

A separate GLO-90-derived 65×65 tile fixture was evaluated at 47.127N, 9.529E
(Vaduz vicinity), with physical elevation 643.376 m ellipsoidal. This is a regional
terrain control, **not a surveyed runway**. The production selection /
primary-ancestor / global-fallback machinery was driven at its existing 8-item
budget to a stable cut. Independent local-vertical triangle-plane intersections
used post-f32 mesh positions; physical queries used the finest available primary
DEM. Bridge geometry/residency was also built and counted.

The final fixture has 765 real primary tiles: nine complete level-10 subtrees
through level 13. With a 5.5 km level-13 floor, selection converged in 24 updates to
116 surfaces and 264 bridges, with `matches_desired=true`, at 1.2, 350 and 1000 m
AGL. The cut has 12 level-12 and 48 level-13 surfaces. For all 11,289 probes inside
4.5 km (75 m spacing), the visible source DEM's bilinear height equals the physical
sampler's result: no source-selection mismatch remained in this sample.

With that same cut, upgrading only supplied 65-grid DEMs at level 12+ (60 surfaces)
produced these absolute visible-triangle versus physical-bilinear errors:

| Sample | 33-point mesh p95 / max | Scoped 65-point mesh p95 / max |
| --- | ---: | ---: |
| 4.5 km, 11,289 probes | 3.073 / 11.237 m | 0.320 / 1.428 m |
| 500 m, 1,961 probes at 20 m spacing | 3.055 / 12.331 m | 0.367 / 0.914 m |

The remaining maximum occurs at 47.1020389748N, 9.5665614863E on primary tile
13/8627/1952. Both paths use that same 65×65 DEM: rendered triangle height
1780.423156 m versus bilinear height 1781.851016 m. More tessellation removes much
of the undersampling but does **not** make triangles equal bilinear terrain on a
saddle. These metre-scale residuals must not be described as contact equality or
a surveyed-airport acceptance.

For the final cut:

- All-33 terrain/bridges: 161,314 vertices, 906,528 indices, 11,369,184 logical
  geometry bytes and 629,184 boundary bytes
- Scoped 65-point terrain/bridges: 365,922 vertices, 2,110,908 indices, 26,007,888
  logical geometry bytes and 936,384 boundary bytes
- The 4.5 km overlay footprint requires 93 surface/bridge records and 123,308
  copied vertices, below the unchanged 512-record and 262,144-vertex bounds
- Refining all supplied 65-grid DEMs, including low-level surfaces, raises geometry
  to 29,671,056 bytes with no additional error reduction in these near samples;
  that broader policy is not selected

### Source-completeness prerequisite

Earlier sparse versions with 187/281 tiles did not contain all child siblings
under real primary ancestors. The existing atomic-cut contract correctly retained
those real ancestors rather than mixing them with global children, even when
finer primary data existed at a queried point. A 173.429 m discrepancy at
47.0959673741N, 9.5665614863E was traced to displayed tile 10/1078/244 versus physical
tile 13/8627/1952. Completing real source subtrees resolved that source mismatch;
no synthetic child data or streamer provenance change was introduced. General
sparse packs retain this availability limitation, and callers must not promise a
near-detail floor when complete primary coverage is absent.

For context, the original 350 m-AGL SSE cut before the floor had 50 surfaces and
119 bridges; its near-500 m error p95/max was 10.201/24.199 m. At ground level,
upgrading just the existing seven level-12+ surfaces without a floor reduced the
near-500 m p95/max from 3.055/12.331 m to 0.367/0.914 m. These earlier controls
isolate the footprint and tessellation benefits but are not the final-cut costs.

## CPU mesh-generation measurement

A new `mesh_resolution` Criterion group compares 33 and 65 points without changing
the underlying DEM. An optional **absolute** `FLIGHTSIM_BENCH_DEM` path loads one
local validated tile outside the timed loop. The fixture/data itself is not in
Git. Example:

```sh
FLIGHTSIM_BENCH_DEM=/absolute/path/to/tile.fsdem \
CARGO_PROFILE_BENCH_LTO=false CARGO_PROFILE_BENCH_CODEGEN_UNITS=16 \
cargo bench --locked -p flightsim-world --bench terrain -- mesh_resolution --noplot
```

Linux shared-container preliminary Criterion results (20 samples, 1 s warmup,
2 s measurement; optimized benchmark profile, LTO off) were:

- Regional 65 DEM: 33 mesh 73.3–77.1 µs; 65 mesh 289.8–299.7 µs, about 3.95× midpoint
- Global 33 DEM: 33 mesh 75.7–77.4 µs; 65 mesh 288.6–300.6 µs

These timings cover CPU mesh construction/destruction only. They do not include
source IO, Bevy asset extraction/upload, draw calls, shaders, audio, or GPU work;
they are not a frame-rate or hardware-GPU guarantee. Existing read/preparation
budgets, sparse-source limitations, transient residency and actual-scene testing
remain authoritative. The separate final aggregate/native QA record owns runtime
acceptance and publication status.

## Shallow original-geometry embedment

At the separate forest control (47.1742644457N, 9.5925149085E), the settled scoped
65-point cut has 89 surfaces and 204 bridges. The same 4.5 km probe found no DEM
source mismatch, but visible triangles were as much as 1.709 m below physical
bilinear ground (near 500 m: 1.319 m below). Native imagery also showed detached
solid bases after the ground-overlay overflow was isolated.

Original procedural geometry therefore extends **only** building-wall and
trunk-bottom vertices 2 m below their existing sampled ground. Roofs, crowns,
upper walls, facade details, vertex counts and topology remain unchanged. This
shallow visual embedment accommodates the measured samples without moving whole
objects or modifying terrain/contact. It is not surveyed foundation geometry or
a general guarantee for arbitrary slopes, sparse DEMs or coarse fallback cuts.
Shadow-map bias is an independent source of detached-looking shadows and is not
changed by this patch. Close native verification remains required.
