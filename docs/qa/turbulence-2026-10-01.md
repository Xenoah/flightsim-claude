# Turbulence numerical validation, 2026-10-01

Issue: [#5](https://github.com/Xenoah/flightsim-claude/issues/5), including its
[explicit separation of numeric and human validation](https://github.com/Xenoah/flightsim-claude/issues/5#issuecomment-5424628037).

## Status and scope

- Deterministic light/moderate/severe reference scenarios: implemented
- Attitude, acceleration and control-reserve regression envelopes: implemented
- Numerical results and subjective evaluation: separated below
- Human handling/playtest, physical controller feel, and pilot impressions: **not performed**

The reference runs are automated flights of `AircraftConfig::light_single()`.
They demonstrate bounded numerical behavior and reproducibility on the tested
build. They do **not** establish real-aircraft fidelity, meteorological turbulence
categories, structural load limits, human workload, or whether the handling feels
good. Issue #5's human-playtest completion criterion remains open.

Preset intensity values, correlation scales, aerodynamic coefficients, control
gains and physical controller mappings were not adjusted. Three defects were
repaired: frame-dependent gust timing, gust-blind reported true airspeed, and a
gust-field discontinuity at the date line and poles. The global field mapping and
vector basis changed to ECEF; this changes seeded atmospheres and their measured
statistics. It is a continuity repair, not a claim of physical or subjective tuning.

## Reproduce

From the repository root:

```sh
cargo run --locked -p flightsim-sim --example turbulence_validation > turbulence-summary.csv
cargo run --locked -p flightsim-sim --example turbulence_validation -- \
  --trace-dir /tmp/turbulence-traces > turbulence-summary.csv
cargo run --locked -p flightsim-sim --example turbulence_diagnostics
cargo test --locked -p flightsim-sim --test turbulence_envelopes --test turbulence_integration
cargo test --locked -p flightsim-fdm --test turbulence_global
cargo test --locked -p flightsim-fdm -p flightsim-sim --all-targets
cargo clippy --locked -p flightsim-fdm -p flightsim-sim --all-targets -- -D warnings
```

`--trace-dir` is optional. It writes 24 CSV traces with every 120 Hz sample;
stdout always contains the 24-row summary. Stderr records the aircraft dynamics
fingerprint, full settled initial states and director targets. CSV write/flush
failures and failed envelopes cause a nonzero exit. No network, renderer, external
terrain, real weather, or physical controller is required.

Test environment: Linux x86_64, rustc 1.93.0, development/test optimization as
configured in the workspace; base checkout `143adbc889c91470ae83f7833f8374126bf05f41`
plus the current working-tree repairs. Runs used a separate Cargo target and
`--offline -j 1`; these flags are optional when dependencies are already available.
No cross-OS/CPU/compiler bitwise equivalence claim is made.

## Fixed scenario definition

Shared executable definition:
`crates/flightsim-sim/examples/support/turbulence_scenarios.rs`.
Both the CSV generator and regression tests use this exact fixture.

| Setting | Value |
|---|---|
| Aircraft | Built-in generic Light Single, 1,043 kg, 16.17 m² wing, 11 m span; dynamics fingerprint `e8ca8e4cac33cd04` |
| Initial position before settling | 35.55° N, 139.78° E, 1,200 m ellipsoid height |
| Initial attitude | Roll 0°, pitch +2°, heading north/0° |
| Initial velocity | North 50 m/s cruise or 35 m/s approach; east/down zero |
| Atmosphere and surface | Existing standard atmosphere; empty terrain source with documented flat 0 m fallback |
| Steady wind | Zero; this deliberately isolates gust response |
| Controller | Existing default `FlightDirector`, recomputed each physics step |
| Calm settling | 60 s; then reconstruct from the settled state and reset simulation time to zero |
| Measured duration | 120 s, including the full gust-onset transient |
| Physics and control cadence | Fixed 1/120 s; 14,400 measured samples per run |
| Seeds | 1 (also the app's CLI preset seed), 7, 4242 |
| Presets | Calm 0; light 1.5; moderate 3.0; severe 6.0 m/s intensity parameter |
| Cruise targets | 1,200 m AGL, heading 0°, speed target 50 m/s, flaps/brakes 0 |
| Approach targets | Descent 1.83 m/s, heading 0°, speed target 35 m/s, full flaps, brakes 0 |
| Throttle | Default director speed loop; no override |

The approach fixture is an **airborne approach-configuration segment**, not a
complete landing/flare. It starts high enough to keep terrain contact out of the
measurements. The existing director has PD steady-state error: its cruise altitude
settles around 1,191 m rather than exactly 1,200 m, for example. Nothing is reset
or discarded after the gust begins. Calm runs are included as controls.

`intensity` is a **per-horizontal-component amplitude bound**, not RMS wind speed.
The value-noise interpolation does not normalize variance. Vertical amplitude is
0.7 times that parameter, and measured RMS depends on position, time, seed and
flight path. The FDM documentation now says this explicitly. “Light”, “moderate”
and “severe” are game preset names, not calibrated operational classifications.

## Measurements and acceptance envelopes

- Roll/pitch: local NED attitude from the physical state, degrees
- Heading error: shortest wrapped angular distance to the commanded north heading
- Translational acceleration: magnitude of `(velocity_after - velocity_before) / dt`
  in ECEF, m/s²; it includes gravity and is not the accelerometer's specific force
- Body-normal load `nz`: subtract modeled ECEF gravity from that acceleration,
  rotate to pre-step body axes and take negative body Z divided by standard
  gravity `g0 = 9.80665 m/s²`. Positive is support against body-down. This is a
  one-step average at the center of mass, not a cockpit sensor model
- RMS load disturbance: RMS of `nz - 1`; the calm offset from 1 is retained
- Controls: the normalized commands actually supplied for each step. Each surface's
  available normalized reserve is `1 - max(abs(command))`; all three axes are
  recorded separately. “Minimum surface margin” is the smallest of those reserves
- Saturation: any surface at absolute command >=0.98; summary counts axis-samples,
  so simultaneous saturation of two axes counts twice. Throttle min/max are
  reported separately and are not included in the surface-reserve budget
- True airspeed and stall fraction: the same gust-relative aerodynamic API used
  by the simulation, not ground speed or an assumed calm-air conversion
- Gust trace: the field sampled at the reported post-step time and position; this
  is not a copy of the held pre-step-position gust used inside the integrator
- Quaternion normalization error, minimum terrain clearance and finiteness are
  checked independently

The following **proposed game regression budgets** were set before the reference
measurement, not fitted tightly to these outputs. Their intent is progressively
larger excursions while retaining upright, non-stalled, controllable automated
flight. They are intentionally broader than the measured trajectories. They are
not real-aircraft operating limits, a weather classification, or human acceptance
criteria. A future intentional handling change should review both its new numeric
results and human feedback rather than simply relaxing a failing assertion.

| Preset | Absolute roll ≤ | Absolute pitch ≤ | ECEF acceleration ≤ | Body-normal load range | Surface reserve ≥ |
|---|---:|---:|---:|---:|---:|
| Calm | 2° | 12° | 2 m/s² | 0.8–1.2 g | 65% |
| Light | 10° | 15° | 8 m/s² | 0.4–1.7 g | 45% |
| Moderate | 20° | 20° | 14 m/s² | 0.0–2.4 g | 25% |
| Severe | 35° | 30° | 24 m/s² | −0.5–3.5 g | 5% |

For every preset: no surface saturation; AGL >=100 m; stall fraction <1;
quaternion norm error <=1e-12; all measured quantities finite; every requested
physics step executes; no crash or divergence. Any failure causes the scenario
runner and regression test to fail. These bounds intentionally do not assume that
all metrics must grow monotonically as strength increases: the closed-loop paths
travel through different parts of the field.

## Reference results

Each row aggregates the worst result over seeds 1, 7 and 4242; pitch/load columns
are the union of observed ranges. The individual-seed CSV retains the full values.

| Scenario | Preset | Max abs roll | Pitch range | Max acceleration | Body-normal load | Min surface reserve |
|---|---|---:|---:|---:|---:|---:|
| Cruise | Calm | 0.000° | 1.719 to 1.724° | 0.001 m/s² | 0.998 to 0.998 g | 97.89% |
| Cruise | Light | 2.190° | 1.227 to 2.111° | 1.655 m/s² | 0.929 to 1.167 g | 96.08% |
| Cruise | Moderate | 4.428° | 0.750 to 2.520° | 3.381 m/s² | 0.867 to 1.343 g | 94.09% |
| Cruise | Severe | 8.873° | -0.236 to 3.404° | 7.049 m/s² | 0.753 to 1.716 g | 86.41% |
| Approach configuration | Calm | 0.000° | -3.620 to -3.561° | 0.0004 m/s² | 0.997 to 0.997 g | 84.07% |
| Approach configuration | Light | 3.395° | -4.631 to -2.589° | 1.568 m/s² | 0.903 to 1.157 g | 82.09% |
| Approach configuration | Moderate | 6.692° | -5.630 to -1.498° | 3.197 m/s² | 0.810 to 1.323 g | 79.89% |
| Approach configuration | Severe | 11.710° | -7.608 to 0.637° | 6.647 m/s² | 0.623 to 1.674 g | 74.94% |

All 24 reference scenarios pass these envelopes. The three additional 10-minute
severe cruise runs also pass. No surface saturated in these runs. This reserve is
specific to this automated controller and flight segment; it does not prove a
pilot's workload, recovery ability or available control force.

## Defects found and repaired

### 1. Gust time depended on rendering-frame grouping

Previously, `FixedStep::advance` moved its elapsed clock to the end of the whole
rendering frame before `Simulation` iterated through the physics steps. Every
step in that frame therefore sampled the gust at the same final time. Holding
identical controls for a 60 s severe/seed-1 flight exposed the difference:

| Comparison | Before: position difference | Before: velocity difference | After |
|---|---:|---:|---|
| 30 vs 120 Hz | 1.033768732 m | 0.051373695 m/s | State bits identical |
| 60 vs 120 Hz | 0.342262262 m | 0.017043560 m/s | State bits identical |
| Calm at both comparisons | 0 m | 0 m/s | State bits identical |

The repair gives `Simulation` a private elapsed accumulator advanced once per
executed physics step, always in the same arithmetic order. Gust sampling retains
the existing 120 Hz end-of-step convention. Reported elapsed time, touchdown and
crash times use that same clock; restarting resets it. No public API or core
`FixedStep` change was needed.

Tests compare every state component's bits at equal executed-step boundaries for
15/30/60/120/240 Hz, irregular fractional frames, and 59/144/165 Hz. They hold the
same controls so controller sampling differences cannot explain the outcome.
Same-binary trajectory tests also compare all 14,400 state/control samples for
each light/moderate/severe cruise and approach seed-1 flight.

**Replay compatibility:** the clock repair alone changes old turbulent recordings
made with multiple physics steps per frame. The additional global-field repair
below changes seeded gust values at every cadence, so **all old turbulent
recordings may drift from their old keyframes**, even at 120 Hz. Same-version
recordings remain deterministic. Existing
state-only `rewind_to` still preserves time and accumulator by design; it is not
an exact reset-to-historical-time API. This repair does not claim to solve that
separate replay contract.

### 2. Reported airspeed ignored turbulence

`Simulation::airspeed()` previously subtracted only steady wind while
`aero_angles()` included both steady wind and gusts. With the old field at the diagnostic's initial
state, the outputs were 50.000000000 versus 49.731685220 m/s. Across that original
reference suite the discrepancy reached 5.444439 m/s in severe approach seed 1.
With the repaired ECEF field, both diagnostic outputs are 52.045842425 m/s.

`airspeed()` now delegates to the same aerodynamic airspeed calculation used by
stall evaluation, preserving the documented physical definition. Tests check
calm/light/moderate/severe cases with nonzero steady wind against independently
formed relative velocity. The final suite's reported/aerodynamic disagreement is
zero. Three integration tests were observed failing before these repairs and
passing after them.

### 3. The gust field had a date-line/pole seam

Previously the field sampled scaled latitude/longitude and treated its three
noise channels as local NED components. Across the date line its sampling
coordinates jumped. At a pole even nearly identical coordinates could rotate
those local channels into very different physical wind vectors.

The repair uses `Geodetic::to_ecef()` from core for the noise coordinates, scaled
by the unchanged 150 m correlation length. Three independently seeded channels
produce an ECEF vector. Its length is capped to 1 before converting to NED; this
rotation-invariant cap preserves the existing horizontal per-component amplitude
bounds after rotation. The existing 0.7 vertical multiplier is then applied in
the local frame. Both position sampling and the physical wind vector are now
continuous around the globe, without duplicating geodetic conversion in FDM.

| Severe seed 1, t=10 s | Point separation | Before physical gust jump | After physical gust jump |
|---|---:|---:|---:|
| Equator/date line | 0.222673891 m | 5.364902951 m/s | 0.000005179 m/s |
| Near north pole | 0.223422865 m | 9.804113814 m/s | 0.000010198 m/s |

Six new global tests cover both poles, nearby points across the date line,
equivalent longitude representations, a moving wrapped date-line crossing,
ordinary north/east/vertical movement, and preset component bounds at many
latitudes/longitudes/altitudes, including unattenuated noise-lattice corners that
exercise the vector cap. Pole comparisons are made in ECEF, not between
rotating NED components. Three of these tests failed with the previous field.
The unchanged 24 reference envelopes and all three 10-minute severe runs pass
with the repaired field; none of the acceptance limits was relaxed.

**Seed/replay compatibility:** the same seed now specifies a different global
atmosphere. The radial vector cap and new basis also change statistical
properties, despite unchanged intensity parameters. Old turbulent recordings are
not guaranteed to reproduce their old keyframes. Same-build deterministic replay
and calm behavior are retained; this is not a meteorological calibration or a
human handling judgment.

## Known limits retained

1. **The existing `FlightDirector` speed loop uses ground-relative body speed.**
   This is already documented in its API. In a controlled, unchanged 50 m/s
   northbound state, calm, 10 m/s headwind and 10 m/s tailwind correspond to TAS
   50/60/40 m/s but all receive throttle 0.55. The present scenarios use no steady
   wind, and record actual gust-relative TAS rather than claiming precise airspeed
   tracking. No controller gains or wind behavior were changed here.
2. **No physical pilot/controller validation.** Numerical normalized command
   reserve does not include device calibration, dead zones, latency, stick travel,
   centering force, keyboard rate limits or subjective discomfort.
3. **No physical weather calibration.** The field remains a deterministic value-
   noise gameplay approximation with 150 m spatial and 4 s time scales, and the
   deliberately reduced vertical component. These runs cannot validate its
   spectrum against measured turbulence or establish real operational categories.
4. **Limited aircraft/environment coverage.** Only the generic Light Single,
   standard atmosphere and synthetic flat fallback terrain are used. There is no
   new-aircraft certification, landing evaluation, crosswind matrix or terrain
   collision assessment in this report.

## Negative and independent checks

- NaN, ±infinite, negative and zero intensities reproduce calm flight
- All three strength presets scale the same field at fixed time/position before
  feedback; different seeds produce different trajectories
- Intentionally corrupted roll, pitch, acceleration, load, surface reserve,
  saturation, terrain clearance, stall fraction, quaternion norm, NaN metrics and
  zero-sample reports are rejected by the envelope checker
- A freely falling, initially stationary aircraft verifies the measurement
  convention: approximately 9.8 m/s² translational acceleration but near-zero
  specific normal load; gravity is not counted twice
- Restart repeats the gust trajectory; zero-duration frames do not change time/state
- Invalid CSV output destination fails with exit 1; an unknown CLI option fails
  with exit 2 rather than silently changing the standard scenario

## Human playtest worksheet: not yet assessed

Use a consistent aircraft, input device and view. Record exact build, device,
calibration/dead zones, trim, camera/view, frame-rate conditions and weather. For
an existing manually flyable approach setup (different from the high-altitude
numeric fixture), use the same command with each of `light`, `moderate`, `severe`:

```sh
cargo run -p flightsim-app -- --engine piston \
  --approach 1.5 --start 35.55,139.78 --difficulty beginner --wind 0/0 \
  --turbulence light
```

The explicit turbulence flag overrides the difficulty preset. Use the same start
and seed-1 preset across attempts, and save an F9 recording if useful. A playback
is evidence of what happened, not a substitute for a person operating the controls.

| Preset | Pilot/device/build | Can hold attitude/track? | Abruptness / oscillation | Workload / control feel | Accept or change? |
|---|---|---|---|---|---|
| Light | Not assessed | Not assessed | Not assessed | Not assessed | Pending |
| Moderate | Not assessed | Not assessed | Not assessed | Not assessed | Pending |
| Severe | Not assessed | Not assessed | Not assessed | Not assessed | Pending |

Record observations independently of pass/fail in the automated suite. A pilot's
requested change should identify the particular effect (e.g. excessive pitch
jolt, insufficient lateral motion, tiring high-frequency corrections), then rerun
the numerical suite after any intentional retuning.

## Verification log

Final command results are recorded here after the last source edit. Numerical
passing status above applies to the listed fixtures, not the complete application
or an unperformed manual playtest.
- `cargo test --locked --offline -j 1 -p flightsim-fdm -p flightsim-sim --all-targets --no-fail-fast`:
  **339 tests passed, 0 failed** (137 FDM + 202 sim), including 19 new turbulence
  tests. An initial broader run exposed one synthetic sim GeoTIFF fixture with an
  unspecified height datum after the strict tile-generation gate was added;
  that analytic 300 m ellipsoidal fixture now declares its datum explicitly, and
  all 14 acceptance tests pass
- `cargo test --locked --offline -j 1 -p flightsim-fdm -p flightsim-sim --doc`:
  **3 doctests passed**
- `cargo clippy --locked --offline -j 1 -p flightsim-fdm -p flightsim-sim --all-targets -- -D warnings`:
  **passed**
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline -j 1 -p flightsim-fdm -p flightsim-sim --no-deps --document-private-items`:
  **passed**
- `rustfmt --edition 2024 --check` for all changed/new Rust files in this work:
  **passed**; `git diff --check`: **passed**
- `bash scripts/check-architecture.sh`: **passed**
- Final ECEF-field CSV: **24 PASS rows**, exactly **14,400 rows in each of 24
  traces**; repeated summary output is byte-identical. SHA-256 of that summary:
  `d13b97e92f548750c752d4a48af66658c1fc6f392aeb8e3ef1494bd9a6100b88`
- The clock/TAS repairs alone preserved the old field's 120 Hz physical-reference
  columns. The subsequent global-field repair intentionally changes those
  trajectories; the final table above is freshly measured using the new field
  against the **unchanged** acceptance envelopes
- Manual human handling/playtest: **not run**; broader app/renderer/physical-device
  validation is outside this numerical report
