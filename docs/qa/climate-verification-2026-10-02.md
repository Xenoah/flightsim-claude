# Climate verification, 2026-10-02

## Executed headless checks, 03:23 UTC

The initial successful headless all-target run used Rust/Cargo **1.93.0** on
Linux, two build jobs, `RUSTFLAGS=-D warnings`, debug information disabled for
dev/test profiles, and incremental compilation disabled. It covered `core`,
`fdm`, `world`, `sim`, `tilegen` and `net`: **885 passing Rust tests across 60
test-binary result groups**, no failures, plus **28 passing Criterion smoke
cases**. These totals were independently counted from
`headless-initial-0318.log` in the task's build-log bundle.

This was the worktree based on `c261f49`, including the subsequent climate
checksum-function closing-brace fix and rustfmt changes. It is a local worktree
result, not an exact-final-commit CI result or a published-build claim. Initial
rustfmt parsing had exposed that syntax defect; the recorded successful run
occurred after its repair.

Climate-specific passing groups:

| Scope | Passing tests | What was exercised |
|---|---:|---|
| `world::climate::tests` | 10 | NOAA reference values, Gaussian interpolation, geographic/seasonal contrasts, lapse, seams, calendar validation and corrupt atlas inputs |
| `sim/tests/climate_integration.rs` | 5 | Unchanged disabled ISA, density changes, actual force changes, frame-cadence independence and exact restart/replay |
| `sim/tests/global_climate_flight.rs` | 3 | Real bundled global terrain plus climate over seven regions in January/July; actual dateline and north-pole crossings |
| `sim/tests/replay_world_climate.rs` | 6 | Version-1 byte preservation, version-2 round trips, truncation, malformed flags/phases/presence, dataset identity mismatch and writer fail-before-output |
| Existing replay fidelity/file-boundary/hostile groups | 50 | Continued original replay numerical and hostile-input regression coverage |

Combined world/climate flights are deliberately short two-second scenarios.
They assert finite state/air/ground, actual source-derived ground, bounded ECEF
motion and continuous physical air through geographic seams. They do not
establish human handling quality, long-flight stability, airport accuracy or
meteorological truth.

The five new climate Criterion cases ran in **test/smoke mode only**. No timing
estimate, FPS claim or optimization conclusion follows from those smoke cases.

### Workspace lint checkpoint, 03:36 UTC

The complete workspace all-target clippy run subsequently finished successfully
with warnings denied (`clippy-round5-0336.log`, recorded exit code 0). Earlier
iterations corrected integration/UI lint errors; no additional climate or
simulation-climate algorithm change was needed after the headless run. This is
a source/build check, not a rendered or interactive application test.

## Release statistical benchmark, 04:35 UTC

The release benchmark was precompiled at 04:26 UTC, with no measurements during
the concurrent app capture/build work. The lead then established a quiet window
with the other build owners. The statistical run completed at 04:35:48 UTC,
exit code 0, and the quiet window was released immediately afterward.

Environment: Linux 6.18.44 x86_64; reported CPU model **AMD EPYC 9V74 80-Core
Processor**; process affinity CPUs 0–8; Rust/Cargo 1.93.0, LLVM 21.1.8; release
optimization, thin LTO, one codegen unit and `RUSTFLAGS=-D warnings`. This is a
shared cloud host: other task-owned app/build processes were paused, but unrelated
host background load was not controlled. The CPU model's product name is not
a claim that 80 cores were allocated to this task.

Criterion 0.8.2 configuration: 1 s warm-up, 3 s requested measurement, 50 samples
per lookup/clone case, 10 samples for the explicitly configured map group,
100,000 bootstrap resamples and 95% confidence. The map case collected 110
iterations over an estimated 4.895 s because its minimum sample/iteration
schedule exceeded the requested three seconds. Plots were disabled.

| Case | Criterion point estimate | 95% confidence interval |
|---|---:|---:|
| Full sample, midlatitude | 172.18 ns | 170.81–173.70 ns |
| Full sample, dateline | 173.45 ns | 172.38–174.64 ns |
| Full sample, north pole | 119.13 ns | 117.89–120.56 ns |
| Warm bundled snapshot clone/drop | 3.703 ns | 3.676–3.735 ns |
| Full sample sweep, 720 × 360 points | 44.725 ms | 44.159–45.503 ms |

These are Criterion time **point estimates, not medians**. A full sample includes
all twelve months needed for the annual zone label. Atlas loading is outside the
timed loops; the clone case measures a previously validated snapshot. The sweep
uses preconstructed coordinates at ellipsoidal altitude zero, not actual terrain
heights. It excludes terrain queries, coordinate generation, palette work, UI
updates, GPU uploads and rendering. **44.725 ms is not total map interaction
latency or an FPS measurement.** No temperature-only optimization was introduced
on the basis of speculation; this result establishes the current cost instead.

Raw estimates, configuration, executable SHA-256 and component source hashes are
preserved in [climate-benchmark-2026-10-02.json](climate-benchmark-2026-10-02.json).
The captured text log is `climate-bench-statistical-0435.log` in the build-log bundle.

Equivalent reproduction after precompilation:

```bash
cargo bench -j 2 -p flightsim-world --bench terrain -- \
  climate --warm-up-time 1 --measurement-time 3 --sample-size 50 --noplot
```

## Offline data checks

- All 9 climate Python path/registration tests pass in the default Python
  environment with NumPy and without h5py; actual NetCDF baking reports a clear
  missing-dependency error when h5py is absent
- Independent data review compared every monthly field against source
  quantization and verified the Gaussian coordinate array and added polar rows
- Full alternate-output baking with the reviewed terrain-preparation v1 and v2
  manifests reproduces the same bundled bytes. The shared geoid input is
  unchanged; the climate bake does not consume terrain/lake surface corrections
- Climate atlas SHA-256 remains
  `b1841fe206dd0b1154868335f7ff09ea9a0e5f17c87baa70029240871fd3bf1a`,
  1,180,456 bytes; replay payload identity is `0x33f6a12038b9f6b8`

See [climate design and reproduction](../data/global-climate.md) and
[independent data review](global-data-independent-review-2026-10-02.md).

## Pending at this checkpoint

- Workspace clippy passed at the later checkpoint above; documentation checks
  were not established by this climate report
- Statistical climate CPU measurements subsequently passed as recorded above;
  end-to-end map latency and GPU performance remain separate measurements
- Bevy app/render tests, visual captures, interactive map checks and physical
  GPU behavior were not established by this headless result
- Final-source rerun/CI and any publication remain separate gates

Focused reproduction commands, in a provisioned Rust environment:

```bash
cargo test -j 2 -p flightsim-world climate
cargo test -j 2 -p flightsim-sim \
  --test climate_integration --test global_climate_flight \
  --test replay_world_climate --test replay_file_boundaries \
  --test replay_fidelity --test replay_hostile
cargo clippy -j 2 -p flightsim-world -p flightsim-sim --all-targets -- -D warnings
cargo bench -j 2 -p flightsim-world --bench terrain -- climate
```
