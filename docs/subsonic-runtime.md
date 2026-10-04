# Bounded dry-jet flight dynamics

`flightsim_fdm::subsonic::JetFlightDynamics` composes the validated
[dry-jet table and Mach aerodynamic schedule](subsonic-foundation.md) into a
separate, opt-in six-degree-of-freedom runtime. Host model kind is `dry_jet_table`
(numeric kind `2`, `u16`); its numerical law revision is `1` (`u32`). Component
schema 1, aircraft-profile schemas and replay schemas are separate identities.
The old `FlightDynamics`, `FDM_MODEL_REVISION = 2`, old force arithmetic and old
fixtures are unchanged. This runtime alone does not create a selectable app
profile, a replay format, a release candidate or verified real-aircraft behavior.

## Configuration boundary

`AirframeDefinition::to_config()` validates an immutable `AirframeConfig` with
name, mass properties, geometry and three fixed landing-gear contacts. There is
no propeller definition, shaft power or dummy engine. `from_config()` copies the
validated actual values back; the stored negative product of inertia is negated
back to authored `Ixz`, including the sign of zero. The definition is a raw SI
DTO; callers must bound serialized bytes before decoding it.

Structural numerical policies match the legacy external physical definition:

| Field | Inclusive bounds |
| --- | --- |
| Mass | 100–100,000 kg |
| Body-axis moments `Ixx`, `Iyy`, `Izz` | 1–1e10 kg m² |
| Product `Ixz` | −1e10–1e10 kg m² |
| Wing area, span, mean chord | 1–1,000 m²; 1–100 m; 0.1–20 m |
| Each body contact component | −100–100 m |
| Spring rate, damping | 1–1e8 N/m; 0–1e8 N s/m |
| Main stroke, bottom-stop travel | 0.001–10 m each |
| Maximum recoil speed | 0.01–20 m/s |
| Each tire friction coefficient | 0–2 |
| Friction transition speed | 0.01–10 m/s |

All fields must be finite. Inertia additionally requires
`Ixx*Izz - Ixz*Ixz > 1e-9*Ixx*Izz`, a conditioning margin for its positive-definite
inverse. Names are nonempty after whitespace trimming and 1–96 printable ASCII
bytes; no name or number is repaired during validation.

`OperatingEnvelopeDefinition` has inclusive `[minimum, maximum]` arrays named
`pressure_ratio`, `temperature_ratio` and `mach`. Their global numerical ranges
are respectively `[0, 2]`, `[0.25, 2]` and `[0, 0.95]`; each interval must span
at least `1e-9`. `OperatingEnvelope::from_definition()` validates those bounds.
`JetAircraftConfig::new(airframe, thrust, aero, envelope)` then requires the
entire envelope box to lie inside the thrust table and its Mach interval to lie
inside the aerodynamic schedule. The host must supply a meaningful authored
operating box; numeric containment is not evidence of trim, stability, stall
handling, engine operability or structural limits.

## Force law and state

Every evaluation recomputes local geodetic position and atmosphere, relative body
velocity, Mach, installed net thrust and all 25 aerodynamic coefficients. It
reuses `aero::body_force_and_moment`, the existing fixed-gear contact law, normal
gravity, core coordinate transforms and rigid-body derivative/offset helpers.
Aggregate installed **net** thrust acts through the center of mass along body
+X. There is no engine-count multiplier, extra ram correction or offset moment.
Command zero means authored idle; signed idle thrust remains meaningful. The
Euler `omega × (I omega)` term remains in the rotational law. Earth rotation is
omitted under the existing ECEF approximation in ADR-0002.

The runtime owns only immutable configuration and `RigidBodyState`. It has no
spool, fuel, actuator, variable-gear or cached-force state. `new` and `set_state`
validate state without inventing an environment. They cannot determine table
support until an environment is supplied. A failed `set_state` leaves the old
state intact. Public `derivative` evaluates the instantaneous law without
advancing the state; it does not promise integration accuracy at that rate.
`ControlInputs` are existing sanitized effective commands, not raw device data.
This runtime does not add further command clipping.

## Contact geometry diagnostics

`JetFlightDynamics::gear_clearances(&state, &environment)` returns validated
`[Meters; 3]` in configured gear-leg order, using the same rotated body contact
points, fixed reference plane, contact ellipsoidal heights and vertical-to-normal
scale as the existing gear force law. Positive means above the plane, zero means
geometric contact, and negative means penetration. Inverted or banked aircraft
therefore cannot produce a contact event merely because CG altitude minus an
unrotated body-axis gear height is small. The helper is additive; old force and
imminent-contact expressions are unchanged.

The public diagnostic validates numerical state/environment and finite geometry;
it neither evaluates forces nor claims atmosphere/table support. It changes no
state. Invalid clearance geometry uses the existing `InvalidInput(Ground)` code.
This is a gear-point diagnostic, not fuselage collision, damage or crash
certification, nor a new flight-force law.

Every successful `JetStepReport` also includes `gear_clearances: [Meters; 3]` for
the accepted endpoint and the **same frozen environment** used by that step's
forces. Each internal endpoint derives the diagnostic from its already-evaluated
frame and validates it before any owned-state commit; zero-duration calls return
validated current clearances. All fallible work remains inside the original
transaction. A host can identify physical gear contact from minimum clearance
`<= 0` and rearm its airborne event after minimum clearance `> 0.5 m`, without
using the integrator's separate 1 mm *anticipation* margin as an event threshold.
The host owns that event policy and must retain its existing snapshot transaction.

## Numerical input policy and fixed error precedence

A `step` first rejects nonfinite/negative `dt`, then duration beyond its resource
budget. It then performs the following checks in order for the current state,
and again for each RK4 stage and each candidate endpoint:

1. Finite state components and finite raw world-velocity norm
2. Finite quaternion norm within `1e-9` of one, a nonzero finite position norm,
   and finite raw angular-velocity norm
3. Core geodetic conversion with finite outputs and geometric altitude in
   **−5,000 to 86,000 m**, inclusive
4. Finite atmosphere temperature offset, finite wind components and norm, then
   valid ground inputs
5. Finite raw relative-velocity components/norm and transformed body-relative
   components/norm, before calling `aero_angles`
6. Standard ISA temperature plus the authored offset must be finite and at
   least 1 K, before ordinary atmosphere sampling; sampled density must be
   finite and nonnegative
7. Valid pressure/temperature/sound-speed conversion to a finite table query,
   authored envelope containment, thrust table support, then aero schedule support
8. Finite resulting derivative, then (during stepping) the substep phase budget

Ground elevation and an optional ground-reference altitude must also be in
−5,000 to 86,000 m. Reference latitude/longitude must be finite and within
±pi/2 and ±pi; each finite north/east slope magnitude is at most 1,000. These
explicit numerical bounds prevent overflow from being swallowed by legacy gear
fallbacks. Reference and plane inputs stay fixed for the whole external step.

Altitude and temperature checks prevent the legacy atmosphere's altitude clamp
or 1 K floor from manufacturing a supported jet state. Raw relative speed and
density checks precede the legacy aero helper that masks nonfinite speed. Raw
world speed/angular speed/wind norms prevent a very large co-moving state and
wind from cancelling to zero airspeed while overflowing ground-contact damping.
No jet speed, height, table query or envelope edge is clipped. Domain comparisons
have no tolerance; the exact evaluated endpoint is inside. State altitude is the
result of core ECEF-to-geodetic conversion, not a separately retained source value.

## Transactional integration and cost

`step(Seconds, ControlInputs, &Environment)` returns
`Result<JetStepReport, JetStepError>`. It accepts durations from zero through
`8/120 s` inclusive. Zero duration still validates the complete current force
query, returns `substeps = 0`, and preserves all state bits. The caller should
normally use the fixed `1/120 s` contract, never a render-frame duration.

For positive duration, the initial required count is the ceiling of the maximum
of `dt/(1/120)`, `|omega|*dt/0.05`, and (for active/imminent ground contact)
`maximum_gear_frequency*dt/0.05`, with a minimum of one. The gear frequency uses
the existing mass/inertia and maximum bottom-stop stiffness calculation. A count
greater than eight fails; it is never reduced to the cap. The duration is divided
into that many equal substeps.

Each substep performs classical RK4 with separately evaluated K1, K2, K3 and K4,
then evaluates the weighted candidate endpoint. State offsets check the raw
quaternion vector and its norm before normalization. Every stage and endpoint
also recomputes its own angular/contact phase requirement over this same
substep duration. Initial contact prediction looks ahead over the requested `dt`.
Stage and endpoint gear checks instead use **zero additional lookahead**, keeping
the existing 1 mm current-contact margin; frequency phase still uses `h`. Looking
another `h` forward from K4 or the endpoint would inspect contact beyond the
requested interval and could repeatedly reject an ordinary still-airborne
approach. Unexpected entry into the current 1 mm/contact region still activates
the stage gear check and rejects a genuinely underresolved step. Ratios above
`1 + 32*f64::EPSILON` fail; the tiny tolerance
covers arithmetic roundoff only. Initial count ceiling, maximum caller duration
and envelope comparisons have no such tolerance. A rising angular rate or new
contact may therefore reject a step that initially fit the budget. This
conservative law deliberately reports underresolution instead of silently
reducing accuracy; it does not establish a stability theorem.

The original owned state is assigned only after **every substep and endpoint**
passes. Any unsupported intermediate state, numerical failure or exhausted
budget returns the original state unchanged. No successful prefix, previous
force or clipped state is committed. There is no retry or adaptive loop. Maximum
work is one initial force evaluation plus five per substep: **41 evaluations per
call**, at most eight substeps and three fixed gear contacts. Reporting adds
three contact-geometry reads per accepted internal endpoint (at most 24), or
three for a zero-duration call. No additional force evaluation is needed; the
already-evaluated endpoint frame is reused. No allocation is
performed while stepping. These are operation/resource bounds, not measured
CPU timings or a frame-rate guarantee.

## Structured failures

`JetStepError` contains `reason`, zero-based `substep`, `stage` and an optional
finite `JetConditions` query. `JetStage` codes are Initial=0, K1=1, K2=2, K3=3,
K4=4 and Endpoint=5. Initial errors occur before attempting substeps. The query is
present only after validating it; earlier errors never attach invalid raw values.

`JetFailureReason` is a closed enum: `InvalidInput`, `OutsideAtmosphereAltitude`,
`OutsideOperatingEnvelope`, `OutsideJetDomain`, `OutsideMachDomain` or
`SubstepBudgetExceeded`. Domain errors retain Below/Within/Above statuses, with
all three axis statuses for boxes. `InvalidInput` carries stable `JetInvalidInput`
`u16` codes:

| Code | Input |
| --- | --- |
| 1 | TimeStep |
| 2 | State |
| 3 | Quaternion |
| 4 | Position |
| 5 | AtmosphereOffset |
| 6 | Ground |
| 7 | Wind |
| 8 | RelativeVelocity |
| 9 | AtmosphereTemperature |
| 10 | AtmosphereDensity |
| 11 | ComponentQuery |
| 12 | Derivative |
| 13 | IntermediateState |
| 14 | AngularRate |

Foundation `EvaluationError::InvalidInput` strings translate to ComponentQuery;
no arbitrary string becomes a runtime reason or prospective wire discriminator.
A host is responsible for visibly stopping an unsupported flight and committing
its own elapsed-time, previous-state and recording transaction consistently.
Encoding these values into a replay still requires a separate versioned codec
and complete model identity; no legacy format is implicitly upgraded here.

## Validation evidence

The new tests use original numerical fixtures, not real-aircraft data. An
independent multilinear thrust polynomial, closed-form zero-angle attached-flow
drag and control moments check forces at static, low and high subsonic speeds
and multiple heights. A separately arranged RK4 trajectory checks stage
reevaluation. Narrow authored boxes isolate K2, K3, K4 and endpoint-only failures;
a later-substep failure proves whole-call rollback. Exact envelope boundaries,
next-representable out-of-domain values, zero-duration validation, duration and
rotation budget edges, initial stiff gear and later angular-budget failures are
covered. A stationary weight/spring-compression case remains settled, and a
controlled ten-second airborne trajectory is finite and exactly equal under
repeated evaluation. Direct corrupted-density and overflowing co-moving ground-speed cases
exercise the numerical guards before legacy defensive fallbacks.

Verified on 2026-10-04: all 274 core/FDM tests including doctests (30 new runtime
checks), strict focused all-target Clippy, rustfmt, strict FDM rustdoc, architecture
checks and the independent Python component-byte fixture check passed. Independent
physics/code review approved the final source after the raw-world-norm overflow
guard and its regression were added. An initial later-stage budget fixture did
not actually exceed the budget; the corrected fixture starts just inside the
phase limit and crosses it under its authored control moment. The production
policy was not weakened. Two pre-existing component rustdoc markup warnings were
fixed without changing component arithmetic.
No simulator/app integration, profile/replay acceptance, GPU/native flight,
real-aircraft fidelity, performance benchmark or release acceptance is claimed.


### Precontact horizon correction

A subsequent headless integration run exposed an ordinary 35 m/s, 2 m/s
unflared descent rejecting at K4 immediately before contact. The initial
whole-step count was sufficient for the requested airborne interval, but the
stage predicate looked an additional full `h` into the next interval. Separating
phase duration from contact prediction corrects that scope error without changing
legacy gear, actual-contact phase thresholds, state/clamp policy, model fields or
component arithmetic. Jet law revision 1 had not been published; this correction
belongs to the same reviewed initial implementation.

Five focused regressions now cover: a 1.03 m CG / 2 m/s precontact window, a short
descent that crosses real contact and settles, a gravity-driven unexpected entry
into the current 1 mm margin that still rejects at K4, the exact original headless
precontact state, and the original held-control approach from 60 m through
touchdown. The physical-data snapshot in `tests/fixtures/jet-contact-dynamics.json`
contains original numerical test data copied from the headless profile plus the
exact original state bits (position/velocity/quaternion/body-rate); it exercises
runtime contact handling, not the profile wire parser or real-aircraft fidelity.
Temporarily restoring the old stage lookahead makes both exact-scenario tests
fail at K4 with the captured query pressure ratio `0.9998766871692274`,
temperature ratio `0.9999765369464686`, Mach `0.102582223417556`; the corrected
horizon passes both without a gentler descent or changed
acceptance condition. The error retains its finite query and whole-call rollback.


### Contact diagnostic verification

Additional checks compare clearances with independent local plane projections
for level, pitched, banked and inverted attitudes, flat and sloped ground, and a
fixed ground reference after horizontal displacement. A fully inverted aircraft
with CG at 1.01 m keeps every gear point more than 2 m clear. Negative clearances
predict the actual reused gear's spring force and moment after independently
subtracting the no-contact derivative; exact zero clearance is also covered.
Successful zero/single/multiple-substep reports match a separate diagnostic call
bit-for-bit, and invalid diagnostic inputs leave state unchanged.
