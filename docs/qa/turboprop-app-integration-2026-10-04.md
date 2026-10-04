# Running-turboprop app integration verification

Status: automated integration checks passed. Independent review and native
acceptance are separate. No native launch, publication, Cedar physical profile,
or Cedar picker entry was performed by this work.

Base: `9b7caf1583e01620add4d6bbb8445c66d1b76efd`, followed by unchanged cherry-picks
of accepted pure host/replay commits `0ac6892`, `e416fb6`, `47915f4`. Separate
isolated branch: `turboprop-app-session`. The initial `ea21449` checkpoint was
explicitly unvalidated; the final source corrections and evidence follow it.

## Result and ownership

- Exact third profile family and typed live/replay session ownership; no
  non-legacy-implies-jet dispatch remains
- All 16 initial, committed, recorded and replayed scalars retained; body-only
  render interpolation never replaces engine state
- Read-only atmosphere, air data, supported aero and contact presentation stays
  in the pure simulation layer at its committed physical clock
- Complete existing aircraft/model/wind/weather transactions, typed replay locks,
  full-state restart, terminal rollback and authentic recorder-prefix handling
- Explicit fixed-step lateral trim: J/L roll, U/O yaw, K reset, normal 0.01/s,
  Shift fine 0.002/s, bounds ±0.2; zero bypass preserves old input bits
- Live new-flight/restart resets both lateral trims; pause, focus loss and map
  capture preserve settings and release transient commands
- Modeled turbine fraction, relative shaft rad/s and blade pitch are labeled;
  generic audio and the original model's static propeller remain explicit limits

Development builds admit an explicitly selected v3 JSON. Commercial-staging
rejects this new family's live/replay startup before source reads, model/session
preparation or direct map preparation. Its existing v1/v2 inspection exceptions
and pure v3/v5 library APIs remain available. Bundle, picker, allowlists, default,
rights gates and release authorization remain Swift-only/unchanged as applicable.
This does not claim all historical external-profile CLI behavior is prohibited.

The only addition to the accepted physical host is read-only turboprop
presentation. Existing FDM/core/world code, old sim and jet implementations,
v1–v4 codecs, all 21 existing replay goldens, aircraft assets, schemas, Cargo files,
release scripts and workflows are byte-identical to the base. New input defaults
also have explicit signed-zero, subnormal and old-command-stream bit checks.

## Executed checks

Counts overlap between runs and must not be summed as unique tests. Commands,
per-run counts, log SHA-256 and final source hashes are in
[the machine-readable receipt](turboprop-app-integration-receipt.json).

| Package set / configuration | Passed | Existing ignored |
|---|---:|---:|
| app, input, ui, render; ordinary build | 1,112 | 3 |
| core, fdm, world, sim, tilegen | 1,179 | 0 |
| final app + audio after commercial guard | 444 | 1 |
| app, commercial-staging | 319 | 1 |
| app, content, assetgen; region-downloads; all targets | 445 | 1 |

The final ordinary app/audio run comprises 350 app unit tests, one app integration
test and 93 audio tests. Commercial comprises 318 app unit tests and one app
integration test. Regional-download configuration comprises 361 app unit tests,
one app integration test, 36 assetgen tests, and 24 + 23 content tests.

Also passed:

- Workspace all-target Clippy with `-D warnings`
- Regional-download app/content/assetgen all-target Clippy with `-D warnings`
- App all-target Clippy with both commercial-staging and region-downloads
- Workspace private-item rustdoc with regional-downloads and `-D warnings`
- Architecture dependency checker, full formatting check and `git diff --check`

The three baseline ignored tests receive no pass credit:

1. `optional_real_region_build_reports_cpu_work_and_respects_scene_caps` requires
   optional external scenery and terrain fixtures
2. `haneda_ground_and_approach_have_no_missing_runway_footprint` requires external
   normalized Copernicus Haneda tiles
3. `measure_bounded_planning_transaction_costs` is a manual quiet-window latency
   measurement with worst-cut meshes

## What the witnesses establish

GPU-free app cases cover exact decoder/metadata routing; authored signed-zero
engine initialization without warmup; no-step and failed-step controller/trim/
parking rollback; 30/60/120/144 Hz control and full-state replay equality;
non-consuming exports; authentic recording closure; identity mismatch;
recorded weather/wind/climate/source restoration; all three recorded engine
values overriding different profile running-start defaults; rewind, clock,
F5–F8 transport, restart and suspension.

Production GLB loader and ECS tests cover transitions in every direction among
legacy, jet and explicit turboprop launch choices, cancellation, failed candidate
preparation, exact pending forces, camera/model/audio ownership, and original
Cedar static geometry loading under a clearly labeled numerical profile. No
Cedar coefficient or flight qualification is inferred from that loader test.

Typed v5 map/weather/wind locks are exercised without a legacy replay resource
or CLI replay path. Rejected regional starts retain active physical package and
render-source lookups at the fixture's exact 350 m height, as well as flight,
request, pending selection, generation and cancellation state. Tests cover all
active, selected, initial, pending and ready source stages.

Real font/layout tests show the turboprop telemetry/trim banner leaves the stall
warning readable at 640×480, 1180×812 and 1280×720. Production legacy help/HUD
layout is unchanged; this does not fix or qualify the separately reported
Windows help/HUD overlap near the GFX/CLD/monthly labels.

## Narrow source-boundary fixes

The preset guard's old `!is_jet()` logic was equivalent to Legacy on the accepted
two-family baseline. Adding v3 would make a metadata-spoofed turboprop eligible
for Swift/Meadow; the guard now requires Legacy explicitly. This prevents a new
third-family regression, rather than a previously reachable baseline v3 route.

The baseline direct preparation helper overwrote its local `active_region` before
jet source validation. A direct caller with an active package and no replacement
could lose that candidate source before validation; ordinary UI requests already
had a guard. The helper now rejects before reassignment and rejects supplied
regional packages. The shared source error text is short enough that the map
retains the complete unsupported-terrain reason and unchanged-flight notice.

## Remaining gates

Native visuals/manual handling, speaker listening, physical controllers and
Windows app-integration acceptance have not been performed here. No rotor
animation, acoustic fidelity, measured propulsion or real-aircraft performance
is claimed. Cedar's separate physical candidate still has an open calm full-stop
gate and remains outside this app preset catalog. Future source binding and
release readiness must be revalidated by the integration owner; no release gate
or source-pin manifest was weakened in this change.
