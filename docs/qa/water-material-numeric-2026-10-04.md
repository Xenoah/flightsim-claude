# Water material: numeric and resource checks, 2026-10-04

This record covers the initial original procedural water slice. It does not
establish visual acceptance, GPU performance, driver coverage or a public release.
Application integration and near/far/day/night/cloud captures remain a separate
gate. The implementation contract and limits are in [water quality](../water-quality.md).

## Checks performed

On the isolated `agent/ocean-quality` checkout, with Rust 1.93.0 and the shared
optimized development build environment:

- `cargo test -j 2 -p flightsim-render water --lib --offline`: 12 passed. This is
  eleven water tests plus the existing authored-water/negative-land palette test.
- `cargo clippy -j 2 -p flightsim-render --all-targets --offline -- -D warnings`:
  passed after correcting two test-only lint findings.
- `bash scripts/check-architecture.sh`: passed with the toolchain environment loaded.
- `git diff --check`: passed.

The tests check geographic ocean/inland/dry classes at bundled atlas reference
locations, negative dry-land and elevated/negative lakes without changing height,
matching cube face edges/corners, f64 phase reduction under polar/dateline/rebase
frames (less than 3 mm equivalent wavelength displacement), extreme finite times,
invalid-time disablement, actual WGSL optics-helper Naga parsing/validation, pair
selection without holes or duplicate draws, default Light's absent owned water
assets, cancellation during preparation, and shared-mesh proxy creation/teardown.

The numeric pipeline-pair test is not a rendered cold-pipeline capture. Likewise,
helper validation does not replace actual composed Bevy shader compilation. The
render gate checks the native current-view material and requested prepass pipeline
caches; integrated render evidence must still exercise that path.

## Isolated CPU preparation measurement

Command:

```
cargo bench -j 2 --profile dev -p flightsim-render \
  --bench terrain_polar_normals --offline -- water_mask
```

No native app or GPU workload was running. Criterion used ten samples. The dev
profile matches the proof executable's repository profile (local crates opt-level
1, dependencies opt-level 3); these are not release-profile or hardware-GPU results.
The atlas was validated once outside measurement and shared across iterations.

| Operation | Criterion reported interval | Estimate |
|---|---:|---:|
| One 8,192-texel batch | 1.9858–2.0050 ms | 1.9928 ms |
| Complete 512²×6 RG8 mask | 404.95–420.46 ms | 412.47 ms |

The batch excludes its fresh builder allocation/zero-fill setup. The complete
mask includes that staging allocation and all 192 geographic sampling batches;
GPU upload, shader compilation and frame cadence are excluded. Criterion extended
the full-mask collection to about 4.1 seconds to obtain ten samples.

Retain the fixed 8,192-texel/update budget. The complete mask needs 192 application
updates regardless of GPU speed; software rendering can therefore keep the Light
fallback for a long wall-clock interval. Do not capture upper quality based on a
short fixed delay. Wait for `preparing_mask == false` and `ready_pairs > 0`, or the
`water material ready` log, and verify failures are absent. A larger batch would
trade preparation latency for additional update CPU time and needs a separate
measured policy decision.

## Resource meaning

The upper mask payload is exactly 3,145,728 bytes. The first activation owns a
single staging buffer, then one render-world-only cube image. High/Ultra reuse it.
The runtime creates one proxy per displayed terrain/bridge entity and shares each
existing mesh handle. Light/cancel destroys the proxies and owned material/image
handles and drops preparation storage and map capacities. The engine can retain
shared compiled pipelines; no claim of zero total engine cache growth is made.
The default Light path returns before terrain queries or water-asset allocation.
