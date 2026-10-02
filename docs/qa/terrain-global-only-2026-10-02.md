# Explicit global-only loading verification — 2026-10-02

## Scope and contract

The app's source factory now uses immutable `EmptyTileSource` when `--tiles` is
absent. `TileSource::primary_reads_possible()` defaults to true for existing
implementations. A false result promises that primary reads can never succeed
for that source instance, rather than describing current availability.
`Box`, references and `GlobalTileSource` forward the capability. Both configured
`DiskTileSource` and mutable `MemoryTileSource` remain primary-capable, including
when the directory is absent or memory has no tiles yet.

The renderer skips impossible primary requests and their missing-read history.
It still checks cached/resident real ancestors before generating fallback,
seeds complete global coverage, records fallback provenance and respects the
same read, preparation, residency and retry limits. No cache/mesh limit changed.
The physical sampler calls the same direct global height function; cached real
DEM levels still win before fallback. No interpolation, terrain data, FDM,
climate, replay format or runway geometry was changed.

## Deterministic work counts

The existing fair-arbitration baseline and the explicit-empty mode were run
against the same selector source, synthetic constant-height DEMs, 512 MiB cache,
read/preparation budget 8, max level 13, 16 px SSE, 1080 px viewport and 60° FOV.
The ground reference was 14.90934 m. Cold camera: 90°N, 0°, 1215 m; moved camera:
89.9914°N, 90°E, 1282 m. Both desired cuts contain 4,094 different exact tile IDs.

| Stage/source contract | Selector calls | Total reads | Fallback generations | Preparations |
|---|---:|---:|---:|---:|
| Cold, conservative primary discovery | 1,195 | 9,560 | 4,096 | 4,096 |
| Cold, explicitly absent primary | 512 | 4,096 | 4,096 | 4,096 |
| Moved, conservative primary discovery | 428 | 3,424 | 1,710 | 1,967 |
| Moved, explicitly absent primary | 221 | 1,710 | 1,710 | 1,761 |

The default-true mode exactly reproduces the prior fair-arbitration baseline.
The explicit-empty mode performs zero primary calls. Exact desired IDs, not
just equal tile counts, determine convergence. Full coverage and non-overlap
are checked throughout the permanent regressions. Different scheduling also
changes temporary preparation work in the moved case; final desired IDs match.

These are deterministic selector work counts, not measured frame times or a
`cargo bench` throughput result. Renderer bridge transactions, GPU preparation
and app frame cadence are outside these counts. They do not establish smooth
interactive flight at the poles or remove the documented runway/LOD limitations.

## Physical equivalence

The permanent bundled-atlas regression compares 162 repeated samples spanning
both poles, both dateline representations, high terrain, ocean and
below-sea-level land. Every optimized height has the exact same f64 bits as
`GlobalTileSource<MemoryTileSource>` and direct global sampling. Representative
values from the separate paired probe are unchanged:

| Position | Surface height (ellipsoidal m) | f64 bits |
|---|---:|---|
| North pole, 0° | 14.90934082031249730 | `402dd1951eb851ea` |
| South pole, 0° | 2800.07894042968746362 | `40a5e0286ae147ae` |
| Equator, ±180° | 21.25249999999995509 | `403540a3d70a3d64` |
| 31.5°N, 35.5°E | -205.18577777784804539 | `c069a5f1e43d05ca` |
| 27.9881°N, 86.925°E | 5491.64445202963088377 | `40b573a4facee71b` |
| 35.55°N, 139.78°E | 43.74870195555742924 | `4045dfd57736c903` |

These identify the existing coarse atlas, not surveyed local elevations.
A paired six-location flight regression additionally compares full rigid-body
state, sampled ground plane, atmosphere and render interpolation after every
one of 240 fixed steps per location. Flight comparisons use numeric equality
without tolerances; sampled heights separately assert f64 bit identity. This is
a short airborne equivalence test, not independent validation of terrain or handling.

## Regression coverage

- An explicitly absent primary that panics if read proves primary IO is skipped
- A one-tile cache with budgets 0, 1 and 8 retains budgets, two-root seeding,
  complete coverage and fallback provenance; 32 desired leaves use 34 total reads
- Cached and resident real ancestors still take precedence after source reuse
- Fallback failures/misses keep their retry cooldown and recover in both modes
- A configured nonexistent disk directory is later populated with a real parent;
  the visible generated children are replaced by that parent without a hole
- Existing dynamic-memory late-child and late-parent retry tests remain intact
- The app factory preserves primary discovery whenever a tile path is supplied,
  regardless of whether global terrain is enabled

## Gates

All 12 affected-scope gates passed with `-j2`, locked dependencies, warnings
promoted to errors, and the established shared build environment:

- Formatting, `git diff --check` and architecture checks
- `world` + `sim` all targets: 481 passed, 0 failed
- `render` + `app` all targets: 379 passed, 0 failed, 2 pre-existing ignored fixtures
- `world` + `sim` doctests: 3 passed
- Strict clippy and private-item documentation for all four affected packages
- Commercial-staging app tests: 128 passed; strict commercial app clippy passed
- Windows GNU all-target checks for the affected packages and commercial app

The lead must rerun final aggregate and native-scene checks after integration.
No updated native-scene or Windows executable smoke is claimed by this isolated
change. This optimization alone is not distribution, Steam or release clearance.
