# Incremental terrain seam planning — 2026-10-02

## Scope and design

The earlier seam bridge implementation budgets mesh builds but computes the
complete topology synchronously on each changed cut. Surviving measurements
identified about 17–18 ms for the default 4,094-tile polar cut and 38.6 ms for an
8,192-tile stress cut; those are historical context, not new-source measurements.

`TerrainSeamPlanner` is a deterministic, single-threaded state machine. The
renderer advances at most 1,024 work units per call, separately from the existing
shared surface/bridge mesh count. No boundary clone, worker thread, whole-plan
sort, output-vector growth, or unbudgeted normal-completion scratch drop hides
in setup or a phase transition. Emitted descriptors are inserted individually
into the pending ordered map and ordered bridge queue.

One work unit handles one of:

- One selected tile and its four indexed edges
- One interval comparison or unmatched edge removal, or an empty line transition
- One adjacent pair's endpoint samples and links (at most four samples)
- One ordinary corner (at most four source points and four incident links)
- One polar source vertex, with the original deterministic mean order
- One emitted seam, with at most four polar-cap assignments
- One scratch corner/cap removal, or a phase transition

Map operations cost O(log N). For a valid non-overlapping N-tile cut, total units
are bounded by 45N + P + 16, where P can conservatively include all retained
boundary vertices. More narrowly, only polar vertices are visited individually.
Scratch retains at most 4N indexed edges, 4N adjacent pairs, 8N corners, 2N polar
edge records and 2N assigned caps; an ordinary corner has at most four samples
and four links, and there are at most two polar means. No completed plan history
accumulates. These are logical record bounds, not allocator/graphics byte totals.

The current synchronous planner remains an independently executed reference.
Regression tests require exactly equal descriptors and order, including shared
anchor bits, fingerprints, polar-cap ownership and corner-patch contents. The
new path changes neither bridge mesh generation nor physical ground/FDM state.

## Transaction safety

Selection and source replacement remain paused from pending-cut creation through
planning, mesh preparation and atomic commit. The old displayed surfaces and
bridges remain visible. New descriptors still compare source generations and
third-neighbour corner patches. Mid-transaction rebases use existing world
transforms; meshes created later use the current frame. A reset drops the planner
and all emitted descriptors along with pending/visible/retired assets; no worker
or asynchronous result can survive it.

Planning progresses even when the remaining mesh budget is zero. Positive mesh
budgets then drain the bridge queue, so planning does not introduce starvation.
The source/memory bounds and original atomic old/new cut semantics are unchanged.
App terrain diagnostics distinguish active planning and its edge/pair/corner/
descriptor counts from an empty bridge queue.

## Explicit remaining synchronous costs

This is a topology-work bound, not a frame-time or 60 fps guarantee. Existing
cut assembly clones the displayed ID set and validates source presence. Atomic
visibility commit, outgoing asset release, pruning and explicit reset/cancel
cleanup are still linear synchronous operations. Cancellation may free O(N)
planning scratch immediately. Their timings must be reported separately from
incremental planner calls, not excluded and silently claimed solved.

More planning frames can increase time until a new cut becomes visible. It is
important to report both per-call latency and total work/staging frames. The
old complete visible cut remains stable while waiting.

## Verification and measurement

Implementation checks and quiet-window measurements are recorded below after
execution. Benchmark fixture creation (atlas/DEM/full surfaces) is outside the
world planner loops. Planner construction, same-cut target clone/validation,
each advance including descriptor queueing, completed output destruction, and
full-plan aggregate cost are reported explicitly. A separate ignored renderer
probe exercises real Commands/Assets paths and labels target assembly plus first
chunk, final commit plus its last bridge, and explicit all-assets cancellation.
The renderer fixture uses real default-resolution flat surfaces and no climate
colour callback; its source DEM fixture creation is outside the recorded surface
preparation time. Neither probe includes a GPU or proves actual frame rate.

Commands:

- `cargo test -p flightsim-world`
- `cargo test -p flightsim-render --test terrain_stitching_systems`
- `cargo bench -p flightsim-world --bench terrain_seam_planning --no-run`
- Quiet only: `FLIGHTSIM_PLANNING_PROBE=1 cargo bench -p flightsim-world --bench terrain_seam_planning -- --noplot`
- Quiet only: `cargo test -p flightsim-render --test terrain_stitching_systems measure_bounded_planning_transaction_costs -- --ignored --exact --nocapture`

Actual integrated render acceptance is a separate gate owned by the lead.

### Correctness gates at 06:00 UTC

- World release suite: 230 tests and one doctest passed
- Native unified render/input/ui/audio/app all-target suite: 682 passed, with the
  pre-existing manual render test and the new timing-only probe ignored
- Eight production stitching system tests passed, including both new multi-frame
  topology transaction cases; none of the previous geometry assertions changed
- Exact descriptor/order equivalence passed for all retained triangle fixtures,
  arbitrary L0–L24/source/resolution changes, both actual default 4,094-tile polar
  cuts at 33-point resolution, and the complete 8,192-tile cut
- Strict all-target world/native Clippy, strict world/render/app documentation,
  architecture checks, formatting and diff checks passed
- The five-fixture, ten-case Criterion benchmark smoke check passed; statistical
  measurements and the explicitly ignored renderer timing probe remain pending

The native gate used the established optimized development profile, debug
information disabled, incremental compilation disabled and `-D warnings`.
World timing uses release/thin-LTO/single-codegen-unit configuration. Results
from these different profiles must not be conflated.

### Quiet-window measurements at 06:02 UTC

Measured implementation: `0d7621350d4d680eb072f0401f74144c39ba5199`, with a clean
worktree. The subsequent source correction only clarifies the aggregate 4N
neighbour-pair wording; it changes no executable behavior. The lead and other
workers stopped builds and app processes for this run. The world process took
23.28 seconds and peaked at 115.0 MiB RSS; the renderer process took 4.26 seconds
and peaked at 1,262.7 MiB RSS. RSS is whole-process peak, not a geometry-byte
measurement or a new runtime memory allowance.

Release planner probe, 20 complete plans per fixture. Each timed advance includes
ordered descriptor/queue insertion and normal scratch cleanup. All calls and
outliers are retained; p95 is an empirical per-call percentile, not a confidence
interval or a hard elapsed-time bound.

| Cut | Tiles | Work units | Planning calls | Advance p50 | Advance p95 | Advance max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Pacific | 26 | 584 | 1 | 0.055 ms | 0.086 ms | 0.096 ms |
| Dateline | 80 | 1,304 | 2 | 0.136 ms | 0.180 ms | 1.544 ms |
| North pole | 4,094 | 88,917 | 87 | 0.044 ms | 0.765 ms | 1.361 ms |
| South pole | 4,094 | 88,917 | 87 | 0.053 ms | 0.949 ms | 2.031 ms |
| Complete L6 | 8,192 | 82,700 | 81 | 0.432 ms | 1.152 ms | 6.409 ms |

The 6.409 ms stress and 1.544 ms dateline maxima are not discarded or replaced
with average values. These measurements support the work-bounded improvement,
not a universal sub-millisecond promise. The polar cut needs more calls than
the larger uniform cut because it has more polar vertices to reduce.

Same-cut target clone plus source validation maxima were 0.317 ms north,
0.462 ms south and 0.803 ms for L6. Planner construction remained constant work
(20–31 ns recorded, near timer/optimizer resolution). Completed output/target
cleanup maxima were respectively 1.524, 2.041 and 3.446 ms; this is caller-owned
output destruction, separate from the already-budgeted planner scratch cleanup.

Criterion, 30 samples, 500 ms warmup and 1 second requested measurement (extended
by Criterion where needed), with both variants including descriptor queueing and
output destruction. These are full-plan CPU costs, not one frame:

| Cut | Synchronous reference estimate (95% CI) | Incremental estimate (95% CI) |
| --- | ---: | ---: |
| Pacific | 44.968 us (43.923–46.120) | 55.097 us (54.671–55.564) |
| Dateline | 147.64 us (145.85–149.35) | 196.12 us (191.43–202.33) |
| North pole | 18.512 ms (17.651–19.430) | 21.614 ms (20.623–22.736) |
| South pole | 20.190 ms (19.361–21.076) | 20.815 ms (20.555–21.092) |
| Complete L6 | 38.033 ms (36.761–39.913) | 43.975 ms (43.359–44.658) |

Thus the implementation trades higher total CPU and more staging frames for
bounded topology work per update. It is not an overall throughput speedup.
Repeated cut changes may defer newly requested detail; full worst-case streaming
convergence and GPU frame cadence are outside this probe. Existing budget-one
streaming convergence and complete old-cut preservation tests still pass.

Real Commands/Assets probe, optimized development profile, one complete run per
cut. Mesh budget is zero during planning, then 256 while building, then one for
the final portion to isolate commit plus its last bridge. The initial surface
preparation deliberately uses the entire cut as its mesh budget to expose the
worst configuration; that is not the application's normal per-frame budget.

| Operation | North pole, 4,094 tiles | Complete L6, 8,192 tiles |
| --- | ---: | ---: |
| Initial all-surface setup | 879.892 ms | 1,803.830 ms |
| Target assembly + first planning chunk | 1.773 ms | 2.586 ms |
| Largest later planning call | 3.328 ms | 3.174 ms |
| Largest planning full update, including Commands | 3.769 ms | 3.525 ms |
| Same-ID replacement target + first chunk | 1.158 ms | 1.620 ms |
| Atomic commit + final one bridge | 14.267 ms | 16.333 ms |
| Full commit update, including Commands | 15.856 ms | 19.056 ms |
| Explicit reset/cancel, all assets and active scratch | 63.652 ms | 84.518 ms |

Topology used 87/81 calls; total staging under that deliberately variable mesh
budget was 367/272 updates. Reset was measured after ordinary corner scratch had
accumulated (2,378/4,347 corners and 8,185/16,256 pairs), including live and retired
source assets. Both cases ended with zero terrain assets and zero scratch.

The remaining 19.056 ms full stress commit and 63–85 ms explicit reset are
significant synchronous costs. This patch does not solve them and does not claim
60 fps. Integrated rendered-scene acceptance remains a separate lead gate.
