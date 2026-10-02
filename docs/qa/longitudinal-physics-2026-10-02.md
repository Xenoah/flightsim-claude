# Longitudinal physics diagnosis and bounded repair (2026-10-02)

## Scope and baseline

Baseline source: `a42b9abaa40651731bc4b30243f2b0d990a858c7`.
Both bundled JSON dynamics are tested: Light Single (1043 kg, 119 kW) and
Swift Sport (750 kg, 134.226 kW). They are generic models, not certified replicas
of named aircraft. No lift, drag, stability, damping, control-effectiveness,
weight, installed-power, or trim coefficients were changed by this FDM repair.
Raw diagnostic telemetry is local-only and is not bundled here.

The investigation separates four things:

1. a stable aircraft returning toward its **current trim**;
2. an over-rotation/energy deficit that requires height to recover;
3. an input interface that reaches excessive elevator before a person can respond;
4. a genuine low-speed throttle-to-thrust defect.

The old `ten_minutes_of_flight_does_not_diverge` test only bounded altitude to
−1000…30000 m and checked finite values. It did not demonstrate the matched,
open-loop, ten-minute trimmed-flight condition required by ADR-0004. The new
`longitudinal` integration tests supply that missing evidence.

## Method

`tests/longitudinal_support` solves the following steady, wings-level, still-air
balance, with the actual nonlinear coefficient curve and engine law:

- `T cos(alpha) - D = W sin(gamma)`
- `L + T sin(alpha) = W cos(gamma)`
- `Cm = 0`

Level trim solves alpha, elevator, and throttle at specified indicated/equivalent
airspeed. Climb trim solves alpha, elevator, and flight-path angle for specified
power and airspeed. The small-angle approximation `L=W` is not used. The solver
uses a bounded pre-stall bracket and rejects unavailable power/lift. These are
local equilibrium estimates; after initialization the real ECEF RK4 simulation
runs with **unchanging elevator and throttle**, with no feedback controller.

The model's indicated airspeed is `TAS sqrt(rho/rho0)` (equivalent airspeed;
compressibility/instrument error are outside this low-speed model). Dynamics
use TAS and local density. Tests keep ground far below the trajectory and
explicitly require normal-flight clearance; terrain cannot mask a failed trim.

### Baseline open-loop stability at 1000 m, 85 kt IAS, clean configuration

| Quantity | Light Single | Swift Sport |
|---|---:|---:|
| Solved alpha | 2.532531° | 2.514460° |
| Solved effective elevator | 0.04361185 | 0.04310885 |
| Solved throttle | 0.47716745 | 0.26665756 |
| Maximum altitude drift, 600 s level trim | 0.096 m | 0.082 m |
| Altitude range after +2° pitch disturbance | 998.900–1001.974 m | 998.799–1001.865 m |
| IAS after 600 s, +10% initial speed | 84.998997 kt | 84.999089 kt |
| Vertical speed after 600 s, +10% initial speed | −0.008705 m/s | −0.007947 m/s |

`Cm_alpha=-0.89/rad` and `Cm_q=-12.4` have the stabilizing signs. Disturbance
energy dissipates, rather than requiring an autopilot to hide divergence. A
small residual local-flat/ECEF curvature mismatch is allowed by a 1 m altitude
bound, not thousands of metres. The regressions include both signs of the
pitch/speed disturbance and require the last-minute speed/vertical-speed
amplitude to be below 2% of the first-minute amplitude.

### Baseline release is conditional, not an altitude-hold feature

From 1000 m, 80 kt IAS, pitch 10°, flight-path angle 5°, full power, clean:

| Released elevator setting | Light minimum altitude loss | Swift minimum altitude loss |
|---|---:|---:|
| Existing default trim 0.09 / 0.08 | 0.000 m | 0.000 m |
| True surface neutral, zero trim | 43.690 m | 5.682 m |

This does not promise safety for every starting state. From 45 kt IAS, pitch
40°, flight-path angle 20°, releasing to default trim costs **91.997 m / 59.447 m**
before recovery. Full back elevator commands a zero-pitch-moment alpha near
36.9°, deeply beyond the lift peak. Holding it continues to stall. No artificial
attitude lock, forced altitude floor, stall suppression, or automatic trim was
introduced.

`stall_angle=16°` is the logistic blend midpoint, not the exact `CLmax` angle.
The curve peaks near 13.1° clean and 12.7° full flaps. Treating 16° as onset
would be misleading. Normal disturbance regressions require alpha below 10°;
separate tests require a genuine lift reduction, drag increase, stall, and
height-consuming recovery. Post-stall/spin fidelity and aircraft certification
are not established by these checks.

### Baseline full-power climb envelope

Steady force balance, nominal mass, clean, ISA. Positive values are climb;
negative values mean there is insufficient excess power at that requested speed.
These are modeled instantaneous values at the stated altitude, not a POH claim.

| Altitude | IAS | Light ROC m/s | Swift ROC m/s |
|---|---:|---:|---:|
| 0 m | 60 kt | 4.559 | 8.543 |
| 0 m | 80 kt | 5.850 | 11.587 |
| 0 m | 110 kt | 2.841 | 9.165 |
| 0 m | 130 kt | −0.551 | 6.394 |
| 2000 m | 80 kt | 3.809 | 8.660 |
| 4000 m | 80 kt | 1.955 | 6.023 |
| 4000 m | 110 kt | −1.688 | 3.088 |
| 6000 m | 80 kt | 0.249 | 3.670 |

More pitch does not create more excess power. Tests additionally require climb
to decline when throttle is reduced, density altitude increases, ISA temperature
is raised by 25 K, or mass increases by 15%. A 40 kt IAS normal climb cannot be
solved in the pre-stall range. A 60-second open-loop climb initialized from the
balance must gain the predicted order of height without accelerating away from
the specified IAS. The generic model has no turbocharger, mixture/RPM dynamics,
propeller pitch map, fuel burn, CG movement, or thrust-line pitching moment.

A further near-rotation release test starts at 100 m with pitch 10°, flight path
5°, full power, and existing default trim. At 60 kt IAS Light loses 9.729 m
before recovering; at 65/70/80 kt it loses no initial height. Swift loses no
initial height in these cases. Peak alpha remains below 6°, so the Light dip
is an energy/trim-capture transient, not a stall. This is not permission to
release the controls at an arbitrary low height.

## Actual FDM defect and repair

The old law set its reference speed to `available_power / static_thrust`, then
used `available_power / max(V, reference_speed)`. At rest those terms cancelled:
**any positive throttle, including 0.01%, produced 2400 / 2600 N**.

The new law retains the existing constant-efficiency power limit and replaces
the fixed static cap by `T_static * throttle^(2/3)`, then multiplies by density
ratio. Under the existing approximation `P ~ throttle * density_ratio`, ideal
static actuator-disk scaling `T ~ P^(2/3) rho^(1/3)` gives that exponent and the
same density factor. It is still a simplified static cap, not a full propeller
map. At 12.5% throttle it gives 25% static thrust (600 / 650 N); at 0.01% it gives
5.171 / 5.602 N. Zero power gives zero thrust continuously.

Tests sweep throttle, TAS, and density, requiring monotone throttle response,
finite nonnegative force, and `T*V <= throttle*Pmax*eta*density_ratio`. The
full-power curve is preserved; normal high-speed cases remain power-limited.
Partial-power low-speed trajectories change, so old-build replays must not be
advertised as bit-compatible merely because aircraft profile JSON is unchanged.
`FDM_MODEL_REVISION=2` is exported for binding into replay fingerprints; rejection
policy belongs to the application integration.

## Keyboard pulse evidence for the input owner

`longitudinal_trace --pulse` applies a key-like ramp at the fixed 120 Hz rate,
then centers it at the existing per-profile centering rate. Initial state is
solved 80 kt full-power climb trim at 1000 m; runs are 60 s. This isolates input
rate from aerodynamic coefficients and frame-end sampling.

| Elevator ramp rate | 0.5 s pulse Light peak alpha | 0.5 s pulse Swift peak alpha |
|---|---:|---:|
| Existing 2.5 / 2.0 per second | 35.201° | 34.904° |
| 1.2 per second | 13.979° | 14.562° |
| 1.0 per second | 11.573° | 11.819° |
| 0.8 per second | 9.563° | 9.684° |

At 0.8 per second, a 1 s hold still exceeds 30° alpha and a 3 s hold exceeds 43°.
This supports evaluating a more controllable **keyboard elevator ramp**, while
retaining the ability to stall. It does not justify reducing aerodynamic control
effectiveness or changing joystick authority. Actual input/application changes
and rendered-frame cadence checks are tracked by their owning implementation.

## Reproduction and verification

From the repository root, with the existing supported Rust/native environment:

```sh
cargo run --locked -p flightsim-fdm --example longitudinal_trace > /tmp/longitudinal.csv
cargo run --locked -p flightsim-fdm --example longitudinal_trace -- --pulse > /tmp/pulse.csv
cargo test --locked -p flightsim-fdm --test longitudinal
cargo clippy --locked -p flightsim-fdm --all-targets -- -D warnings
```

`longitudinal_trace` writes diagnostic traces to stdout and summary rows to
stderr; no files are created by the example itself. To reproduce the pre-repair
baseline, use the same diagnostic/test-support source with a42's `aircraft.rs`.
Do not overwrite the frozen baseline checkout.

New tests cover true trimmed flight and disturbance decay at 1000/3000 m, full-back stall and
release, energy/force balance, throttle continuity, climate/weight/power trends,
120/240 Hz timestep convergence, and bit-identical 6/15/30/60/144 Hz plus jittered
render schedules given the **same timestamped fixed-step inputs**. This last
condition is important: it does not excuse render-frame input sampling errors.
GUI, real-controller handling, pilot feel, and real-aircraft performance require
separate verification. Shared end-to-end/headless aggregate results are recorded
by the integration owner.

## Primary physics references

- [NASA Glenn: Propeller Analysis](https://www.grc.nasa.gov/www/k-12/airplane/propanl.html):
  momentum relations across an ideal propeller disk and their limitations. The
  static thrust exponent above is derived from those equations with the stated
  power/density approximation, not measured aircraft data.
- [FAA: Aerodynamics for Naval Aviators](https://www.faa.gov/sites/faa.gov/files/regulations_policies/handbooks_manuals/aviation/00-80T-80.pdf),
  climb performance, p.154: climb rate follows excess power divided by weight.
  The tests include the model's nonzero thrust/flight-path angle explicitly.
- [FAA AC 90-89C](https://www.faa.gov/documentLibrary/media/Advisory_Circular/AC_90-89C.pdf),
  §6.6: static and dynamic stability are distinct; perturbing a trimmed state
  without retrimming is an appropriate stability check. These numerical checks
  do not constitute the circular's aircraft flight-test or certification process.
- [FAA PHAK, Chapter 6: Flight Controls](https://www.faa.gov/regulationspolicies/handbooksmanuals/aviation/phak/chapter-6-flight-controls):
  trim relieves the control force needed for a flight condition. Here it remains
  an explicitly simplified equivalent-elevator bias, not a hinge-moment model.

## Separate read-only lift-peak helper

`aero::positive_stall_peak_angle(&aero, &geometry, flaps)` returns an
`Option<Radians>` for the first positive attached-flow lift maximum strictly
between zero and the configured blend midpoint. It evaluates the existing
canonical `coefficients` function without changing its expression or evaluation
order. The bounded search uses 64 intervals and 40 refinement iterations;
this does not search for the later deep-stall flat-plate maximum. Invalid,
disabled-stall, or unusual custom curves without an identifiable interior peak
return `None`. The app owns handling that absence and can cache results while
configuration/flaps stay unchanged.

`tests/stall_peak.rs` compares both aircraft at clean, 25%, 50%, 75%, and full
flaps against an independent 20,000-interval scan of the actual lift curve.
It checks the local slope on each side, a conservative 85%-of-derived-peak
warning point before lift stops increasing, and deterministic absent/invalid
cases. The application must use the derived value for this cue to change; the
helper alone does not alter warning/UI behavior.

Final component verification: `cargo test --locked --offline -p flightsim-fdm`
passed 151 unit/integration tests and one doctest, including ten new longitudinal
and two lift-peak tests. FDM all-target clippy with `-D warnings`, workspace
format check, architecture check, and diff whitespace check passed. The wider
headless/render/application aggregate remains the integration owner's gate.
