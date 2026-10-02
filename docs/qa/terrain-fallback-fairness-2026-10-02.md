# Terrain source scheduling and exact-cut convergence (2026-10-02)

## Scope and diagnosis

The default-size north-pole selection has 4,094 desired leaves. The earlier
scheduler at `aea8d96` did eventually reach those exact IDs: this was a long tail,
not a demonstrated deadlock. At 2,561 live / 4,094 resident IDs, one outstanding
fallback mesh prevented an entire 1,534-leaf subtree from replacing its covering
root. Comparing distance across the primary and fallback queues repeatedly
selected nearby missing-primary retries instead of that farther eligible tile.
A matching tile count alone also does not establish that two cuts have equal IDs.

The fix changes only arbitration between already eligible source reads:

- Alternate primary and fallback attempts whenever both are eligible, retaining
  the turn across updates so a one-read budget is fair too
- Keep the existing cold-globe root-seed exception, primary oldest-attempt order,
  fallback distance order, and 120-update retry cooldown
- Keep all primary-ancestor eligibility checks. An available real ancestor still
  outranks fine fallback, and a late real tile is still retried and replaces its
  fallback atomically
- Count successful, missing and failed attempts alike when advancing the turn;
  both kinds of source work continue sharing the same read budget
- Preserve mesh-preparation budgets, the 8,192-ID resident limit, cache limits,
  and complete non-overlapping cut activation without changing mesh or seam code

`TerrainSelectionState::desired_len()` reports raw requested selector leaves
before availability handling. `matches_desired()` compares the exact live and
requested ID sets, not their lengths. Both describe the last selector update;
they do not imply that a renderer's deferred bridge transaction has committed,
that the current camera has been sampled again, or that a truncated LOD selection
satisfies its original screen-space-error target.

## Reproduced update/read counts

An optimized standalone probe imports the actual `terrain_selection.rs` and uses
the real world selector. Its primary source always returns missing; fallback
always returns an available synthetic 2×2 DEM. There is no Bevy or GPU in the
probe. These are deterministic selector-call/work counts, not frame times or a
rendering-performance benchmark.

Settings: 16 px SSE, 1,080 px viewport height, 60° FOV, maximum level 13, root
geometric error 20,000 m, 512 MiB DEM cache, and read/mesh budgets of eight.
The cold camera is `(90°, 0°, 1215 m)`; after exact convergence the same state is
moved to `(89.9914°, 90°, 1282 m)`. Both use a 14.90934 m surface reference and
have 4,094 desired IDs. Calls below are one-based; convergence compares sets.

| Work to exact desired-ID convergence | Earlier scheduler | Fair scheduler |
| --- | ---: | ---: |
| Cold selector calls | 1,878 | 1,195 |
| Cold source attempts | 15,024 | 9,560 |
| Cold fallback generations | 4,096 | 4,096 |
| Cold mesh preparations | 4,096 | 4,096 |
| Moved-camera selector calls | 639 | 428 |
| Moved-camera source attempts | 5,112 | 3,424 |
| Moved-camera fallback generations | 1,710 | 1,710 |
| Moved-camera mesh preparations | 1,924 | 1,967 |

Both schedulers expose the first refined cold cut on call 1,194. The earlier
scheduler then spends 684 additional calls before exposing the final subtree;
the new scheduler exposes it on the next call. The moved-camera preparation
count increases by 43 because read ordering also changes retained/cached-mesh
preparation ordering. It is not claimed that all kinds of work decrease.

## Permanent regression coverage

`crates/flightsim-render/src/terrain_selection.rs` includes:

- The complete 4,094-leaf cold and moved-camera sequence, exact desired-ID
  equality, complete globe coverage and non-overlap after every update, and
  unchanged read, mesh, cache and resident limits
- Continuous ready fallback competing with both a fresh primary dependency and
  an overdue retry whose real coarse tile has appeared after an earlier failure,
  for budgets one/eight, both distance orders, successful/failed fallback turns,
  and a one-tile cache
- Missing and failed fallback cooldown/recovery into a complete sibling cut,
  for budgets one/eight; a partial one-child cut is intentionally not used as
  proof of visible convergence because its covering parent must remain
- Equal desired/live counts with different IDs, successful later convergence,
  and empty-selection diagnostics
- The existing real-ancestor precedence, same-ID replacement, late real parent,
  seed coverage, one-tile-cache, failure recovery and deterministic-order tests

Restoring only the old distance-based arbitration makes the continuous-demand
regression fail immediately, demonstrating that it exercises the changed policy.
The overlap helper now walks every tile's exact ancestor path instead of testing
every pair; geographic-quadtree overlaps can only occur on those paths, so large
cuts are checked without weakening the invariant.

## Verification boundaries

The final source passed the following native checks with the recovered Rust
1.93.0 toolchain, established `-D warnings` build/doc flags, and `-j2`:

- `cargo test --locked -j2 -p flightsim-render --all-targets`
- `cargo clippy --locked -j2 -p flightsim-render --all-targets -- -D warnings`
- `cargo doc --locked -j2 -p flightsim-render --no-deps`
- `cargo fmt --all --check`, `scripts/check-architecture.sh`, and `git diff --check`

All 213 render-library tests and 34 render integration tests passed, including
the corrected missing/error fallback fixture and large cold/moved regression.
Two existing tests stayed ignored: the external Haneda DEM fixture and the
manual quiet-window planning-cost measurement. The standalone complete selector
suite also passed all 35 tests. An earlier run
had one failure confined to the incomplete one-child fixture described above;
it was corrected without changing production behavior or widening its limit.

GPU-visible final-cut commitment and actual frame times require separate
integrated scene checks; the integration suite does exercise the bridge and
streaming systems. No public release, remote CI, or hardware-GPU result is
implied here.
