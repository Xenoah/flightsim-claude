# Procedural forest canopy undersides

Public baseline `2e3e18578cad61a23ad36db906b17ef665677418` has the same
source tree as local production `e426802d4ade179eb9b41ccd122d2f3398b4babe`.
Compared that production source with candidate
`b50738240c972ef38b6f22b89895fd6559765331`. Each crown now has a downward-facing
hexagonal base: eight added triangles per tree, changing 72 vertices / 24 triangles
to 96 / 32. Static diff review confirms the original 72-vertex emission order,
positions, normals, colours, placement/seed, height and 2 m roots are preserved.
Materials, batching, all hard limits and cutoffs, DEM/contact, FDM and controls are
unchanged. Saturated mixed batches can accept fewer trees at the higher per-tree
vertex cost; limits are not increased to hide that tradeoff.

## Matched fixed views

The same replay fixtures, regional inputs, production materials and settled terrain
were captured before and after. All three candidate processes exited 0. The view
beneath the crown now shows opaque foliage where the original showed bare trunk
and sky. The lower outer view closes the exposed underside while retaining the
original outer foliage. This establishes the fixed-view hole correction, not
photorealism or general visual quality across every camera position.

| View | Trees before / after | Added vertices |
|---|---:|---:|
| Beneath crown | 346 / 346 | 8,304 |
| Lower outer crown | 346 / 346 | 8,304 |
| Aerial control | 339 / 339 | 8,136 |

Each delta is exactly 24 vertices per accepted tree. Other per-pose feature counts
are unchanged; the two low views have different selections and are compared only
against their own baselines. Every final cut has 89 displayed/live/desired surfaces,
204 visible bridges, no pending planning/stitching, and overlay revision 5/5.
The aerial PNG is byte-identical before/after, SHA-256
`e9f0497e75b6ccbc05cad3bed006a8a5636beda3984af8c87b21990d20df113c`.
Screenshots and raw capture records are retained privately.

## CPU preparation cost

One authorized, sequential A-B-B-A Criterion block used immutable binaries,
identical fixture bytes, locked dependencies and the existing dev profile
(workspace optimization 1, dependencies 3; debug info and incremental compilation
disabled). Each process used 20 samples, 500 ms warmup and approximately 2 seconds
measurement. All four exited 0; all 80 samples, including outliers, were retained
without retries. FlightSim builds/captures and reviewer scripts were quiet;
background desktop apps were not controlled system-wide.

The unchanged `scenery_mesh_cpu/16_forests_300m` fixture actually accepts **58**
trees from 512 candidates, with 16 land polygons and 3,072 ground vertices.
Solid vertices change from 4,176 to 5,568; total vertices from 7,248 to 8,640.
It measures production mesh building, allocation and returned-mesh destruction,
excluding input loading, DEM I/O, app selection, ground draping and GPU work.

| Run | Median (µs) | 95% confidence interval (µs) |
|---|---:|---:|
| A1 | 906.104 | 900.580–920.991 |
| B1 | 914.792 | 910.516–920.909 |
| B2 | 921.720 | 918.278–927.059 |
| A2 | 893.041 | 888.869–895.717 |

Paired median differences are +0.96% and +3.21%. Comparing the two controls' mean
run medians gives a descriptive +2.08%, or +18.68 µs per fixture. This is not a
pooled estimate or a statistical-significance claim, and supports no GPU, FPS or
frame-time conclusion. The A run medians vary by 1.46%; B by 0.76%.

## Validation status

Passed: 18 scenery unit tests (including four focused canopy checks), 12 bounds
checks, six lattice checks, and 34 app scenery checks (one existing ignored).
The focused checks cover emitted f32 winding/closed rims/front-face rays, complete
tree rollback at 95/96/191/192 vertices, cancellation before second-tree emission,
and an actual 128-tree / 12,288-vertex deterministic batch across origin rebasing.
Strict native-package clippy, formatting, architecture and app build passed.
The clean integrated source `2720ed4b34b46eb42a1e35017eca00b8b56ba6ac`
completed all **15 aggregate gates** on 2026-10-03: **1,879 Rust tests**
(965 headless, 5 documentation, 909 native), **144 Python tests**, **190
commercial-profile app tests** and **51 untimed benchmark smoke cases**.
There were zero failures; three normal and one commercial-profile pre-existing
optional/manual tests remained ignored. Strict workspace clippy/docs, formatting,
architecture, normal and commercial-profile builds passed. Source identity was
unchanged across the suite. The only subsequent change is this reviewed QA
acceptance record; the captured candidate's production code is unchanged.

The cap fix is accepted with the stated geometry cost and benchmark limits.
These source checks do not clear the separate binary-distribution authorization
gate. Existing release-rights/dependency conditions remain in force.
