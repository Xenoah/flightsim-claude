# Regional scenery CPU and lifecycle QA — 2026-10-02

## Historical checkpoint and exact measured input

Independent tests exercise CPU mesh construction, Bevy ECS asset ownership, and
terrain-overlay transactions. No window, render device, GPU submission, hardware
frame-rate, screenshot appearance, or physical controller is tested here.

This historical CPU measurement/check window was 2026-10-02 16:38–16:40 UTC, using:

- Integration HEAD `54531359623822fe72ee0a555ef7a79d9f4e4e2b`
- Uncommitted diff SHA-256
  `0bdf1ddae3bc7b3e18eb20026fed2e5231d192720c508197919015a17c2c33ed`
- That diff affected `scenery_runtime.rs`, `scenery_exclusions.rs`, and
  `terrain_overlay_transactions.rs`: curved-Earth exclusion fixes and explicit
  material types in the existing drape test driver
- HEAD and diff hash were checked before and after the run and did not change
- Rust/Cargo 1.93.0; Linux x86-64 container reporting AMD EPYC 9V74 and nine
  available logical processors; other heavy task builds/captures paused
- `--profile dev`: workspace code at opt-level 1, dependencies at opt-level 3;
  debug information and incremental compilation disabled, two build jobs,
  `RUSTFLAGS='-D warnings'`

These are **development-profile CPU measurements**, not release-profile or GPU
performance claims. The first benchmark attempt omitted the target's Rust flags
and was stopped during prerequisite compilation; it produced no measurement.
The successful run used the existing integration target with matching flags.

Later changes are outside this measured checkpoint. In particular, commit
`2417598` separated the 2.3 km tree-exclusion query from the 4.5 km visual
selection, and commit `1440fc8` added scoped regional terrain refinement. The
regional fixture subsequently expanded from 187 to 765 DEM tiles. The numbers
below remain evidence for the exact earlier input above; they are not current
scene counts, current build timings, or final application acceptance. Consult
[the regional terrain measurement](regional-render-detail-2026-10-02.md) and the
separate final integration record for their respective inputs and limits.

## Regression coverage and results

| Check | Verified result |
|---|---|
| App `scenery_runtime::tests`, integration `5b587b8` | 20 passed, one optional fixture probe ignored; 1.67 s test execution |
| Renderer `scenery_bounds`, historical measured input above | 12 passed; 0.01 s test execution |
| Actual map-start transaction, historical measured input | One passed; 0.05 s test execution |
| Optional Liechtenstein scenery + regional DEM probe, historical measured input | One passed; counts/timing below |
| Criterion `scenery_mesh` | Five cases completed with 20 samples each |

The focused app suite proves:

- Completed async work stages at most one mesh asset per update; old geometry is
  retained until the exact replacement overlay revision commits
- Cancelled or stale-generation results never upload; reset happens before a
  completed old task can be polled into the new scene
- Empty/out-of-coverage results remove the previous scene
- Clear, actual R restart, F8 rewind, and map relocation remove new, old, staged,
  incoming, and retired ground registrations/assets as applicable
- A first optional swap into an empty registry blocks a second swap even though
  its retired list is empty; clear reopens admission
- Old/new assets stay within the two-cohort transition bound; independent ground
  and solid geometry totals agree with reported scene totals
- Source reordering does not change deterministic batches or building geometry
- Exclusion cell/global/broad-reference and primitive caps fail closed; indexed
  lookups agree with a separately exhaustive geometric lookup
- Missing mesh output still consumes conservative candidate-work debits; a
  zero-output rejected-forest scene cannot multiply the 8,192 scene budget by
  its number of batches
- Live simulation state, ground height, elapsed time, recording bytes and final
  replay state stay exact while visual scenery is prepared/staged/reset and
  detail settings change; visual CLI options do not alter replay conditions

The renderer tests also cover rejected-tree cancellation on the next candidate,
zero/small caller budgets, hard caps despite unlimited requested values, split
solid/ground totals, and preserving solid buildings when ground work is masked.
A crossing road/paint and land fixture is checked against runway/apron interiors
using an independent 2D separating-axis intersection test: the unmasked fixture
must overlap, and the masked output must have no interior overlap while keeping
surrounding scenery and immutable source geography. Long 1/10/20/30/39 km airport
segments are checked at ordinary, dateline and near-pole anchors; their surface
midpoints remain excluded despite ECEF chord curvature, while ground 100 m away
remains available.

The initial eight app failures were an invalid *test fixture*: the database
constructor correctly rejects an empty database. The fixture now uses valid
source data 20 km outside coverage. Those failures were not hidden as product
passes; the corrected suite was rerun. No full-workspace or final-release gate
is implied by these focused results.

## Actual CPU mesh timings

`cargo bench --profile dev --locked -j 2 -p flightsim-render --bench scenery_mesh -- --noplot`

Each case uses deterministic synthetic geographic features and a constant
450 m ellipsoidal elevation callback. Source construction is outside the timed
loop; allocation and CPU mesh preparation are inside it. Warm-up is 500 ms,
measurement is approximately 2 s, and sample size is 20. The values below are
Criterion's **median point estimate and bootstrap 95% median confidence interval**
from `new/estimates.json`, not its separately printed fitted slope estimate.

| Fixture | Median ms (95% median interval) | Solid vertices | Ground vertices | Trees / candidates | Skipped source features |
|---|---:|---:|---:|---:|---:|
| 64 dense 24×18 m, 15 m-high buildings | 1.451 (1.444–1.485) | 38,784 | 0 | 0 / 0 | 0 |
| 64 residential roads, 600 m each | 1.702 (1.675–1.767) | 0 | 9,600 | 0 / 0 | 0 |
| 16 forest polygons, 300×300 m | 1.045 (1.037–1.048) | 9,216 | 3,072 | 128 / 128 | 0 |
| Three 900×900 m forests, all tree candidates rejected | 1.327 (1.317–1.342) | 0 | 4,608 | 0 / 512 | 0 |
| 64 buildings with a 1,024-vertex caller budget | 0.494 (0.490–0.505) | 606 | 0 | 0 / 0 | 63 |

The final capped case emits one whole building, never partial walls. Renderer
candidate counts above are actual attempts, including rejected attempts. App
scene candidate statistics may conservatively charge a batch's assigned
allowance when no mesh is returned; they are an upper work debit, not always an
exact attempt counter.

These timings exclude database loading, spatial selection, DEM disk/cache
lookups, exact terrain-overlay clipping, GPU transfers, shaders and frame time.
They do not establish that the application maintains any target FPS. Container
scheduling and the short sampling window limit generalization.

## Historical optional real regional probe

The separately supplied Liechtenstein database had SHA-256
`ef6c802053df442a1db8a871e4591476bb5f845bb4ec394a1bf6df9cf81a867e`.
Its complete source database contained 13,932 roads, 17,906 buildings and 4,067
land polygons. The probe used the matching 187 regional DEM tiles, with observer
at the database centre, 47.187868 N, 9.549603 E.

One CPU build measured **122.614 ms**, including scene selection, exclusion
construction, physical-source DEM sampling and CPU ground-template preparation;
it excludes database file decoding (completed before the build), GPU upload,
exact displayed-cut clipping, and frame presentation.

| Result | Count |
|---|---:|
| Selected features | 4,096 |
| Output batches | 65 |
| Total vertices | 390,225 |
| Ground vertices | 90,000 |
| Depicted roads / buildings / land polygons | 965 / 1,736 / 231 |
| Procedural trees / candidate debits | 0 / 0 |
| Skipped features | 1,164 |
| Truncated | true |

At this measured checkpoint, saturation of the 4,096-feature visual selection
intentionally disabled vegetation because its shared exclusion set might be
incomplete. Since `2417598`, vegetation is disabled for query truncation only
when the separate 2.3 km exclusion query reaches its cap; saturation of the
4.5 km visual selection alone no longer does so. Other exclusion safety limits
can still suppress vegetation. The zero-tree count and 122.614 ms build above
therefore must not be carried forward as measurements of the revised policy.
Neither policy establishes complete regional coverage or surveyed tree absence.
The 90,000 ground vertex ceiling also deliberately limits detail.

The probe is opt-in and never assumes a private/local dataset is available in CI:

```bash
FLIGHTSIM_SCENERY_FIXTURE=/path/to/liechtenstein.fsscenery \
FLIGHTSIM_SCENERY_TILES=/path/to/tiles \
  cargo test --locked -j 2 -p flightsim-app optional_real_region_build \
  -- --ignored --nocapture
```

The optional pack's own provenance, attribution and redistribution terms remain
separate from this CPU/runtime verification. No raw QA logs, replay files, input
PBF, or private telemetry is included in this document.

## Reproduce the focused tests

```bash
cargo test --locked -j 2 -p flightsim-app scenery_runtime::tests
cargo test --locked -j 2 -p flightsim-app scenery_map_tests
cargo test --locked -j 2 -p flightsim-render --test scenery_bounds
cargo fmt --all --check
bash scripts/check-architecture.sh
```

## Bounds and remaining limits

Current scene construction caps are 4,096 selected features, 128 output batches
(up to 104 solid and 24 ground), 600,000 total vertices including 90,000 ground
vertices, 2,048 trees and 8,192 candidate-work debits. Renderer batches cap at 64
features, 65,532 total vertices, 128 trees and 512 candidates. Scene tree
exclusions use 128 m cells, at most 512 references per cell, 262,144 indexed
references and 512 broad references. These are count/ownership limits, not
resident GPU-byte or latency guarantees.

Airport ground precedence is conservative footprint masking, so it can clear
extra scenery near pavement edges. It does not lower source roads or change
physical terrain. Buildings and trees remain decorative and have no collision
or navigational-obstacle claim. Ground overlays track displayed terrain cuts;
these tests do not establish automatic solid-height rebuild at a stationary
camera when a late regional DEM replaces the prior physical source. Keep that
case distinct from ground-only exact draping and from the tested explicit
reset/reselection lifecycle.
