# Availability-aware terrain parent fallback — 2026-10-01

## Scope and status

Local fix based on reviewed alpha.21 candidate `d104b8c`. This report covers the
renderer's missing-parent-fallback defect. It does **not** establish runway/light
visibility, final release acceptance, GPU performance, or publication.

SSE selection requested level 13 while the normalized Copernicus Haneda fixture
contains levels 8–12. The previous renderer drew only available selected leaves,
leaving near-camera holes even when their level-12 parents existed. The ground
sampler already had an independent parent fallback and is unchanged.

## Implementation contract

- Keep the SSE-selected target tree. Probe an unavailable desired leaf's ancestors
  until a cached DEM or existing rendered mesh is available
- Count every `TileSource::load` call against the frame budget, including missing
  and failed reads. `TerrainUpdate` exposes attempts, loaded, missing, and failed
- Fresh dependencies precede repeated attempts. Otherwise oldest attempt, then
  camera distance and tile ID determine order; hash iteration never selects work
- Retry missing/error results after 120 selection updates. They are not permanent
  ocean classifications. Prune attempt history to the desired tree and its
  ancestors every update, so travel does not retain all historical misses
- Resolve a complete, non-overlapping cut. If any requested child region is still
  uncovered, retain the ready parent and suppress **all** of its descendants
- Bound hidden mesh preparations independently by the same frame budget, including
  cache hits. Existing and prepared meshes remain valid after DEM cache eviction
- A parent is replaced only when a complete finer cut has prepared meshes. The app
  activates/hides/removes the complete cut together before rendering. New meshes
  start hidden and receive the current RenderFrame transform immediately, since
  terrain streaming runs after the normal transform pass
- Store mesh handles beside entities, so a mesh prepared and discarded in the same
  Commands batch is removed from Assets even before ECS queries can see its entity
- Retain existing visible descendants when their requested coarse replacement and
  ancestors are absent, including partial resident coverage. Do not discover new
  unseen descendants. Prune canceled hidden preparation when the desired tree changes
- Limit all resident meshes (visible and hidden) to 8,192. Under capacity pressure,
  farther retained descendants yield to nearer new work; active parent/replacement
  nodes stay protected. Report capacity pressure in TerrainUpdate
- Require the entire desired ancestor closure to fit the resident ceiling before
  any mutation. The default 4,096-leaf selector, including its truncated terminal
  path, fits. A custom selector exceeding that bound is rejected explicitly. This
  invariant prevents a deadlock with all slots protected by incomplete replacements
- Do not change the source, sampler, physics, airport surfaces, height/lift values,
  terrain mesh resolution, or tile format

Read and mesh-generation budgets are separate counters with the same configured
limit. A final visibility cut may activate many already prepared meshes together,
but activation itself performs no mesh generation: preparation and registration
stay within the frame budget. The 256-child regression
uses one source read and one mesh preparation per update, keeps its visible parent
until update 256, and then changes visibility together. It also uses a one-tile DEM
cache: prepared meshes do not require their DEMs to stay cached.

This is **ancestor fallback**, not discovery of available finer descendants below
an unavailable coarse selected tile. Areas with no data on the selected tile's
ancestor path can remain uncovered, including dataset edges, unless existing finer
visible coverage is retained. Distant retained coverage can be evicted at the finite
resident ceiling. An initially cold region can also remain uncovered until its
budgeted reads and preparations finish. These limits
must not be described as complete global terrain coverage.

## Synthetic regressions

`crates/flightsim-render/src/terrain_selection.rs` covers:

- All children absent; closest available parent retained
- Partial children and all 32 child/parent availability patterns
- A live parent retained until all four children are ready, including budget 1
- Child files appearing after an earlier miss and errors recovering after retry
- Corrupt `.fsdem` decoder errors distinguished from missing files
- Mixed-depth descendants suppressed when a coarser ancestor is selected
- A one-tile DEM cache preserving the parent and prepared children through refinement
- Fair progress for available leaves with a one-tile cache and no parent
- Zero budget retaining existing meshes without preparing even cached DEMs
- Full/partial resident finer coverage surviving unavailable coarsening, empty DEM
  cache, and subsequent parent arrival, without probing unseen child data
- A 256-leaf replacement with one read and one mesh preparation per update
- Canceled hidden work discarded when requested resolution changes
- Sufficient five-slot resident capacity completing a four-child replacement
- Invalid capacity rejected before state mutation; default full/truncated trees fit
- Retained history bounded at 96 test slots across 64 regions, preserving new nearby
  coverage and explicitly exercising eviction
- Retry metadata pruned while visiting 500 regions
- Deterministic output under reversed request order
- Date-line and polar edges, and a completely absent depth-24 ancestor path

Each synthetic update checks actual source-call and mesh-sink counts against the
budget, resident capacity, and every pair of visible IDs for ancestor/descendant
overlap. Three ECS/Assets tests in `terrain_streaming_systems.rs` additionally check
hidden preparation, initial transform ordering, atomic activation, and immediate
asset cleanup when preparation and destruction occur in one update.

## Real DEM footprint regression

The explicit ignored integration test
`crates/flightsim-render/tests/terrain_parent_fallback.rs` uses the production
renderer selection, `DiskTileSource`, and `TileCache`. It requires the external
normalized Haneda fixture, not distributed with this source. Fixture provenance
and limitations are documented in [data boundaries](data-boundaries-2026-10-01.md).
The validation fixture has 429 tiles, levels 8–12, 65×65 samples.

```bash
source /workspace/shared/flightsim-tools/env.sh
export CARGO_TARGET_DIR=/workspace/scratch/193080ebc0b3/flightsim-claude/target
FLIGHTSIM_REAL_DEM_TILES=/workspace/shared/flightsim-audit/copernicus-haneda-tiles \
  cargo test -j 2 -p flightsim-render --test terrain_parent_fallback \
  -- --ignored --nocapture
```

The synthetic runway footprint is sampled at 501 along-runway × 19 across-runway
points (9,519 total), matching the independent occlusion diagnosis. For each
camera, the test first reconstructs the previous available-selected-leaf behavior
and checks its exact missing count, then runs the fixed production selector for
240 updates with a source-read budget of eight. Coverage must stay complete after
it first becomes complete; duplicate coverage at any sampled point is rejected.

| Camera | Before missing | After missing | Before/after live tiles | Final footprint LOD | First complete update |
|---|---:|---:|---:|---:|---:|
| Ground 35.55, 139.78, 40 m | 6,774 / 9,519 | 0 / 9,519 | 11 / 13 | 12 | 1 |
| Approach 35.5331, 139.7533, 173 m | 2,474 / 9,519 | 0 / 9,519 | 15 / 16 | 12 | 2 |
| Drop start 35.55, 139.78, 3,022 m | 0 / 9,519 | 0 / 9,519 | 4 / 4 | 9 | 1 |
| Drop end 35.5501, 139.7802, 2,885 m | 0 / 9,519 | 0 / 9,519 | 4 / 4 | 9 | 1 |

Read totals over 240 updates, including retries and misses, were 95, 90, 70, and
70 respectively. These are workload counts, not frame-time or GPU measurements.

## Separate runway-visibility defect

Filling the holes can expose previously invisible occlusion by the existing
33×33 level-12 terrain triangles. The independent pre-fix diagnosis found terrain
up to approximately 2.57 m above the draped runway at this LOD, with 28.85% of full
footprint pavement samples buried. The level-9 drop case has a separate coarse
LOD displacement too. This change does not alter either surface or claim to fix
that burial. See [runway drape QA](runway-terrain-drape-2026-10-01.md).

Actual app recaptures against the preserved alpha.21 binary and independent review
are separate gates after the local numerical/test result. No captures or remote
publication are claimed by this report.

## Final local validation

On 2026-10-01, the corrected hidden-preparation implementation passed:

- Workspace clippy, all targets, `-D warnings`
- CI headless package group, all targets: 833 tests passed; benchmark test harnesses
  also completed successfully (no performance measurement is inferred)
- CI render/input/UI/audio/app package group, all targets: 597 tests passed,
  including 95 app unit tests, 195 render unit tests, and 3 terrain ECS tests
- Explicit external Haneda test: 1 passed, confirming the table above unchanged
  with bounded mesh preparation. It is intentionally ignored in the ordinary suite
- Headless package doc tests: 5 passed
- Workspace docs, private items included, `RUSTDOCFLAGS='-D warnings'`
- `cargo fmt --all --check`, architecture dependency rules, and `git diff --check`

Independent read-only review rechecked the 23 selection regressions and 3 ECS tests
and found no remaining code blocker. These are Linux local gates; remote/Windows
CI and actual app visual acceptance remain separate.
