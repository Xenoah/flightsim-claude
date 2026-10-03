# Adaptive overlay snapshot pacing

Status: accepted after independent review and all 15 integration gates on
`7e51d7ad6f19eb71099ed6a279d6e7f2c226ae52`. The measured production change is
`5f7ec3e686581b81b47a29894cb2d84f9537d78f`, based on
`ef9717727f793945633c5567b353eea77e2a9ded`; the integration preserves those code
bytes and adds the reviewed evidence below. The measurements support a narrow
reset-readiness result, not a general frame-time or FPS improvement.

## Why investigate

Before this change, the implementation copied at most two relevant meshes per update,
regardless of size. This limit predates the atomic-upload work: it also appears
in f604dd9 and f693903. No isolated copy-CPU measurement establishes two as a
necessary threshold. Whole-app log intervals are not copy CPU durations.

A matched-input Balzers refresh in the current source copies 48 sources and
56,491 vertices in 24 updates. Fifteen updates copy 280 vertices each. The frozen
forest capture copies 66 sources and 79,436 vertices in 33 updates, with 22 of those
updates copying 280 vertices. These counts motivate an isolated scheduling
experiment. They do not establish a latency regression or FPS gain.

## Policy and accounting

- Consume only the mesh attempts left after terrain surfaces and bridges. Do not
  raise that allowance; production defaults to 8. Preserve required-first source
  order and the same frozen source generations and exact snapshots.
- Normal copied-position target: **8,450** vertices, two maximum 65×65 source
  grids. The renderer admits resolutions 2..=65 in
  `terrain::prepare_tile_with_global_shading`.
- Normal charged-index target: **52,224** entries. `flightsim-world/src/mesh.rs`
  emits 6*(65-1)^2 = 24,576 surface indices and its `append_skirt` emits
  4*(65-1)*6 = 1,536 skirt indices. Two complete buffers total 52,224.
  Snapshot copying retains only surface positions/triangles but must inspect
  skirt indices too; using only copied triangle counts would undercharge work.
- One indivisible first source can exceed either normal target, but existing
  source and transaction validation still applies. Stop before a second copy
  after that exception. A maximum-size first source cannot starve indefinitely.
- Per-source index ceiling stays 6×262,144 = 1,572,864 entries. Per-update charged
  index ceiling explicitly preserves the prior two-source worst case:
  **3,145,728**. The conservative normal policy ordinarily stops far below it.
- Existing 512-source and 262,144 aggregate copied-vertex transaction bounds,
  524,288 output cap, upload pacing, failure grouping and atomic commit remain
  unchanged. This candidate does not alter clipping, terrain, physics or shading.

`copy_indices_charged` reserves the complete bounded buffer of each admitted
attempt before detailed validation. Invalid position attributes, malformed index
lengths or an exhausted transaction vertex cap may fail before scanning, while
still consuming the reserved allowance. Missing buffers or buffers above the
unchanged per-source index cap reserve zero because they are rejected without
scanning. Every rejected copy still consumes a shared attempt.

`copy_indices_scanned` counts entries actually consumed by the triangle iterator,
including skirt triangles discarded from its output. It is at most the charged
count. Budget-deferred candidates use only O(1) metadata, consume no attempt or
reservation and remain next in the deterministic order. Neither counter measures
wall time, GPU time, allocation capacity or clipping-worker work.

The existing five fields of the app's work-log prefix remain intact; the two
explicitly named index fields follow them. Metrics remain opt-in. Source lookup,
allocation overhead and synchronous cleanup still need measurement; matching
logical bounds is not a wall-time guarantee.

## Regression coverage

- Derive targets against real 65×65 world meshes, checking copied surface data
  and inspected-but-discarded skirt indices
- Tiny sources consume 0/1/3/8 remaining attempts; no premature mesh/visibility
  change or asset growth
- Exact normal vertex/index target, next-vertex excess, malformed target+1 index
  length, oversized valid first source and maximum hard source indices
- Missing/invalid data and hard-limit rejection: charged versus actually scanned
  work are distinct and failures consume attempts
- Exact source order and snapshot geometry across 1/2/8-attempt cadences; cancellation
  discards preparation, resets frame counters and preserves displayed ownership
- Production source replacement: final positions/normals/colours/indices match
  across budgets 1/8, old visible terrain/overlays remain until atomic commit
- Retain existing public-API lifecycle and output-upload boundary regressions

## Focused candidate gates (2026-10-03)

Passed using locked/offline dependencies, two build jobs and the existing native
union feature graph after a byte-preserving freshness guard on all 232 Rust,
manifest and shader inputs:

- 23 overlay unit tests, including the seven new copy-pacing tests
- 11 production transaction tests, including new final-geometry/visibility parity
- 7 independent public-API lifecycle tests
- 8 terrain stitching tests; one pre-existing manual timing test ignored
- 14 loading/counter metrics tests
- Strict all-target clippy across render/input/UI/audio/app, strict render docs,
  workspace formatting, dependency architecture checks and a native app build

Independent static review found no remaining findings. Its optional-prefix
fixture correction was followed by a fresh 23-unit pass, strict clippy/formatting
and native relink. The production algorithm did not change after the transaction,
lifecycle, stitching and metrics passes. Only focused gates were run here; this
is not an aggregate-workspace or commercial-distribution acceptance statement.
The later diagnostic controls each passed 193 app tests (one pre-existing optional
fixture ignored), strict all-target app clippy and native builds. These are
control-build gates, not an aggregate acceptance pass for the integration tree.


## Isolated CPU snapshot experiment

On 2026-10-03, two sequential ABBA blocks compared the **actual baseline code**
against the candidate, not candidate code with an attempt budget of two. Separate
immutable diagnostic binaries append the same test-only harness to each exact
production source. Both original clean source trees and native binaries remain
preserved. The native-union test build used opt-level 1 (dependencies 3), debug 0,
no incremental compilation, locked/offline inputs and a byte-preserving freshness
guard before switching the shared build target.

The timer surrounds actual `begin_frame` plus `advance`: validation, allocation,
index filtering, position copying, precision-extent calculation and accounting.
Fixtures, registry setup, command acquisition/flush, output assertions and output
destruction are outside the timer. An unprocessed sentinel and a remaining-real-
source attempt allowance exclude final asynchronous clipping dispatch. No GPU or
async worker runs. `Instant` measures elapsed wall time of this CPU path, not
thread CPU-clock time or the whole application frame.

Each process uses eight untimed warmups and 64 measured phases per case, giving
256 phases per variant/case. Assets are reused warm; checks and destruction
between phases can affect allocator/cache state. All processes use the same
permitted logical CPU on an AMD EPYC 9V74 host. The 17 cases include a 4×4 patch
of 16 actual Balzers DEM tiles with production grids/skirt indices/seam meshes,
64 actual tiny bridges, 32 maximum grids, normal/hard-limit boundaries, early
failures, zero budget and cancellation. The patch is representative source
geometry, not the native application's exact selected cut.

All eight runs exited successfully: **8,704 phases and 43,264 update observations**,
with no work-bound/accounting failures. The 34 serialized fixture/output files
match byte-for-byte between controls. Every phase also checks exact snapshot
positions, indices, origins, footprints and precision extents, unchanged owner
handle/visibility and mesh-asset count. Baseline index charges/scans are derived
from the admitted fixtures outside timing; candidate telemetry is checked against
those values. A reserved buffer may still scan zero entries after early failure.

The following totals are identical between variants; arrows show A→B. CPU
percentiles use linear interpolation at `(n-1)*p`, with no discarded observations.

| Fixture | Copy updates | Copied vertices | Charged/scanned indices | Phase median, µs |
|---|---:|---:|---:|---:|
| 64 tiny bridges | 32→8 | 8,896 | 51,336 | 223.774→202.290 |
| 40-source max65 regional patch | 20→11 | 70,936 | 437,052 | 1663.178→1506.617 |
| 40-source mixed33/65 patch | 20→9 | 45,848 | 283,476 | 1059.272→970.455 |
| 32 maximum production grids | 16→16 | 135,200 | 835,584 | 3183.823→2772.207 |

| Same fixtures, per update | Median, µs | p95, µs | p99, µs | Maximum, µs |
|---|---:|---:|---:|---:|
| 64 tiny bridges | 6.820→24.567 | 7.371→28.574 | 20.977→99.466 | 318.631→947.158 |
| Max65 regional patch | 7.642→166.846 | 222.378→228.688 | 288.330→377.971 | 813.978→2340.191 |
| Mixed33/65 regional patch | 7.401→132.976 | 135.738→206.508 | 194.722→370.895 | 1840.997→1399.752 |
| Maximum production grids | 194.238→162.986 | 245.020→237.999 | 344.101→416.534 | 1629.328→5613.518 |

Packing four times as many tiny sources raises the update median from 6.820 to
24.567 µs; the phase median falls about 9.6%, not fourfold. Regional update
medians also change composition because baseline has many tiny-only updates.
Maximum-grid schedules are identical: 16 updates, each copying two sources,
8,450 vertices and 52,224 indices. Their lower candidate medians cannot be
attributed to fewer updates, and their worse observed tails remain a risk:

| Maximum-grid ABBA block | A p95/p99/max, µs | B p95/p99/max, µs | Updates >1 ms, A→B |
|---|---:|---:|---:|
| First | 250.562 / 393.648 / 1629.328 | 219.842 / 360.423 / 527.316 | 1→0 |
| Second | 239.487 / 310.590 / 552.003 | 261.561 / 471.336 / 5613.518 | 0→10 |

All slow samples are retained. Of 4,096 maximum-grid updates per variant, A has
one above 1 ms and B has ten, all in the second block. The 5.613518 ms sample is
unique in magnitude but accompanies other elevated candidate updates; it is not
a removable outlier. Lower medians recur in all four candidate processes, while
the tail shift is not uniform across blocks. Without scheduler events or
per-thread CPU-clock measurements, neither interruption nor a code-level cause
is established. Independent review reached the same limited interpretation.

Indivisible sources can remain costly. Two valid 1,572,864-index buffers produce
one A update versus two B updates; update median is 12.130→5.920 ms and maximum
25.930→12.079 ms, with identical total geometry. This exercises the unchanged hard
source limit and does not promise a submillisecond snapshot bound. Early-rejection,
transaction-exhaustion, zero-budget and cancelled cases retain exact failure and
charged/scanned semantics. The initial harness-only malformed position fixture
was corrected before timing because Bevy rejects its construction; both controls
used the same corrected missing-position fixture. That failed preflight is retained.

## Matched native reset block

The predeclared order was **A1, B1, B2, A2**, each a fresh process. A is exact
`ef971772`; B is exact `5f7ec3e`. Both have an identical private, default-off trace
patch, enabled only for measurement. It adds F8-handler markers and per-frame
`Time<Real>` interval/post-state records without synthesizing input or changing
budgets, physics, camera, materials or shaders. B retains its production index
telemetry, so the native experiment measures the full candidate with equal added
tracing, not isolated snapshot CPU work.

Controls use the same fixed Balzers fixture at 120 m AGL, Swift Sport chase view, regional
DEM/scenery and assets, cloud cover zero, render stats, software Vulkan/llvmpipe
and **1180×812 physical client pixels**. Immutable executable and input hashes are
checked before each launch. Reading the same inputs deliberately warms OS caches;
no cache flush is attempted. Each process converges, performs one excluded native
F8 warmup, then measures reset IDs 2/3/4, with at least ten seconds of unchanged
ready state before/between/after measured resets. F8 is dispatched through the
actual desktop keyboard path. No scene, build, probe, screenshot or other app
interaction overlaps a measured bracket. All four processes close normally with
exit 0. All 12 reset analyses report no control or work-policy violation.

Readiness requires the new request and committed revision, no pending terrain,
overlay, scenery or replay-seek work, and the exact reference sorted tile IDs and
residency signature: **119 surfaces, 271 visible bridges, 15 registered roots
(11 optional), four precision-hidden and zero omitted**. Counts alone do not
establish the match. Application F8 processing begins the monotonic measurement;
CUA dispatch-call brackets are retained separately and are not substituted for
OS keydown latency. The lower/upper endpoints bracket the fully matched ready
observation, not the internal terrain commit or GPU presentation.

Complete, uncut frame intervals overlapping the reset/readiness bracket include
the successor interval containing completion work. Boundary overshoot is retained.
Native percentiles use nearest rank; with n=36 or 50, p99 equals maximum here.
Periodic percentile reports are neither averaged nor substituted for raw samples.

| Run/reset | Ready observation bounds, s | Complete intervals | Frame p50, ms | Frame p95, ms | Frame p99=max, ms |
|---|---:|---:|---:|---:|---:|
| A1/2 | 21.813–22.263 | 50 | 449.851 | 467.647 | 736.106 |
| A1/3 | 21.895–22.362 | 50 | 452.929 | 481.619 | 740.048 |
| A1/4 | 21.898–22.352 | 50 | 454.445 | 489.878 | 695.039 |
| B1/2 | 16.143–16.605 | 36 | 460.499 | 556.430 | 686.071 |
| B1/3 | 15.769–16.223 | 36 | 459.417 | 484.760 | 760.232 |
| B1/4 | 15.441–15.913 | 36 | 453.894 | 491.290 | 778.793 |
| B2/2 | 15.286–15.759 | 36 | 453.026 | 479.528 | 703.584 |
| B2/3 | 15.730–16.161 | 36 | 456.872 | 495.390 | 714.480 |
| B2/4 | 15.701–16.133 | 36 | 458.722 | 474.558 | 693.721 |
| A2/2 | 21.814–22.299 | 50 | 454.869 | 487.953 | 719.302 |
| A2/3 | 21.779–22.209 | 50 | 451.795 | 482.163 | 719.771 |
| A2/4 | 21.958–22.369 | 50 | 454.239 | 476.406 | 724.469 |

Every measured reset performs the same **53 copies / 57,942 copied vertices**
and **19 uploads / 545,811 uploaded vertices**. Copy-active updates fall from
**27 to 11**, while upload-active updates remain **nine**. These are separate
phase totals, not claims that all uploads fit one update.

The six A upper readiness observations range **22.208759–22.368796 s**; the six B
observations range **15.759392–16.605100 s**. Each B reset spans 36 complete
intervals versus 50 for A. These are observed ranges, not confidence intervals.
Every B upper observation is below every A lower observation (21.779–21.958 s).
Independent source/hash/control/raw-distribution review recommends the candidate
for this bounded warm reset-to-matched-readiness improvement. It does not establish an FPS or general frame-tail gain: B's worst reset
p95/max are **556.430/778.793 ms**, versus A's **489.878/740.048 ms**. Tail results
remain mixed, alongside the unresolved isolated CPU tail observations above.

The parser initially rejected A1's expected missing-audio-device ALSA diagnostics.
A classification-only correction records those known startup messages separately;
all original logs and the initial refusal remain retained. It does not change
frame/readiness parsing, waive other errors, or discard a timing run. Missing
physical audio also means this experiment does not validate audible playback.

Trace formatting/log I/O and transition-only exact-cut inspection add observation
cost. The ready observation includes scenery/replay completion and its inspection,
not presented-frame completion. There is one ABBA block, warm caches, one scene,
one software renderer and a small per-reset frame sample. Native cut identity and
functional geometry checks do not establish bitwise native-image equality or
hardware-GPU behavior. The integration acceptance below retains these limits.

## Evidence and integration acceptance

Private receipt sets are identified for audit by their human-readable experiment
labels: **“CPU snapshot ABBA, 2026-10-03 04:10 UTC”** and **“native-abba01, matched
Balzers F8 resets, 2026-10-03”**. They retain exact source/binary/input SHA-256
manifests, immutable controls, diagnostic harness/trace changes, raw observations,
all preflight outcomes, per-run percentile tables, exact-work strata, independent
review and native dispatch/exit records. No raw private logs, fixture data, local
machine paths or held screenshots are embedded in this source document.

The CPU experiment passes geometry/accounting/bound checks. The native block
passes its recorded control checks and supports the narrow readiness result.
Independent review supports the bounded reset-latency benefit; it does not clear
all frame-tail concerns. The private trace instrumentation is not in production.

The clean integration source `7e51d7ad` completed all 15 gates on 2026-10-03:
**1,875 Rust tests** (965 headless, 5 documentation, 905 native), **144 Python
tests**, **190 commercial-profile app tests**, and **51 untimed benchmark smoke
cases**, with zero failures. Three normal and one commercial-profile optional or
manual tests remain explicitly ignored. Formatting, strict workspace clippy and
docs, dependency architecture, normal build and commercial-profile build all pass.
The source before and after the suite is identical. These source checks do not
clear the separate fail-closed binary distribution authorization gate.

A final uninstrumented integration binary also completed the fixed forest
150 m AGL capture with **89 displayed surfaces / 204 visible bridges**, revision
5/5, no pending work, four precision-hidden and two omitted optional overlays,
matching the accepted baseline. The complete **1280×720 PNG bytes and pixels**
match that baseline exactly (SHA-256
`e9f0497e75b6ccbc05cad3bed006a8a5636beda3984af8c87b21990d20df113c`).
This is a separate offscreen settled-image check, not byte-equality evidence for
the native ABBA screenshots. The capture overlapped compilation and supports no
performance inference. Final changes after the tested source are this QA
acceptance record only.
