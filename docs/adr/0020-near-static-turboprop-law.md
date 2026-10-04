# ADR-0020: Isolate a bounded near-static adverse-inflow approximation

- Status: accepted pure-FDM foundation; aircraft and host adoption remain separate
- Date: 2026-10-04

## Decision and compatibility

Add `flightsim_fdm::turboprop::near_static` as explicit kind 3 / FDM law 2.
The existing `turboprop` API, law-1 constants, component schema, closed error
types and numerical behavior remain unchanged. Profile v3, identity schema 3
and replay v5 continue to select law 1 exclusively. No new serialized component,
profile, identity or replay version is allocated here. No app selection,
aircraft preset or engine startup/parking behavior is added.

`near_static::PropellerMap::from_forward(forward_map, negative_rows)` takes an
already validated law-1 map and two explicit paired coefficient rows. Their
fixed J coordinates are -0.01 and -0.005; pitch uses the existing forward axis.
The final negative cell shares the exact existing J=0 row. The combined J axis
must still fit 32 knots and 1024 pairs. `TurbopropAircraftConfig::from_forward`
retains the whole validated law-1 configuration and builds this component.
It changes no airframe, atmosphere, turbine, governor, inertia, envelope or gear.
The complete physical state remains the same 16 scalars, without inflow history.

All J>=0 queries (including either zero sign) delegate to the accepted law-1
component, preserving its guard precedence, interpolation and load arithmetic.
Negative J uses the explicit signed grid, pitch then J, with no extrapolation,
absolute-value substitution, clamping or runtime row generation.

## Authored operating set

This is an original, quasi-steady approximation for small adverse freestream
velocities close to static powered operation. It is not an empirical propeller
model or a theorem about reverse flow. Negative freestream alone does not
determine local disk/wake flow. Local recirculation, dynamic inflow, developed
vortex-ring state, windmill brake, reverse/beta, feathering, slipstream effects
and ground-effect corrections remain unmodeled.

For J<0, require positive CT and CP and the conservative law-specific floor

    CPh = sqrt(2/pi) * CT^(3/2)
    CP >= CPh - 64*epsilon*max(abs(CP),abs(CPh))

Nonpositive CP or CT is rejected unconditionally. Nonfinite or zero derived
floor is rejected; values are never repaired. Validate both negative rows and
their shared J=0 closure at construction and check every negative query again.
Convexity of CT^(3/2) means a convex bilinear mixture of valid positive corners
satisfies the ideal-arithmetic floor throughout these cells. Actual floating
point evaluation still needs the runtime guard. Positive CT/CP and J<0 also
give CP-J*CT>0 throughout the new branch; the unchanged forward map retains its
continuous work-inequality check. CP must remain nondecreasing with pitch at
every new row, matching the accepted feedback convention.

Every negative-J force evaluation, including Initial, K1–K4 and Endpoint,
additionally establishes the current positive induced-velocity scale

    A = pi*D^2/4
    vh = sqrt(T/(2*rho*A))
    adverse_ratio = -Va/vh <= 0.10
    transverse_ratio = hypot(Vy,Vz)/vh <= 0.10

These fixed 0.10 ratios and J>=-0.01 are predeclared authored limits, not measured
operating boundaries or numerical tolerances. No ratio tolerance is added.
Nonfinite or underflow-to-zero loads/scales/ratios reject. A negative Va whose
computed J underflows to zero also rejects rather than entering the forward
branch. The existing pressure, temperature, Mach, positive shaft/absolute spin,
helical tip and transverse/tip limits remain in force. Thus the operating set
is an intersection of limits, not the entire coefficient rectangle.

## Mechanics, integration and diagnostics

All dimensional loads retain T=CT*rho*n²*D⁴, Pp=CP*rho*n³*D⁵ and Qp=Pp/Omega,
with n=Omega/(2*pi) and Omega=omega+s*p. The new runtime calls the existing
coupled-rotation function with the retained forward configuration. This shares
the accepted torque, rotor inertia, angular-momentum and energy arithmetic:

    E_dot_rot = Ma dot b + Qd*omega - Qp*Omega

No extra propeller reaction torque is introduced. Negative T*Va stays signed;
J*CT/CP is not labeled forward-flight efficiency there. The rotor can supply
stored kinetic energy, so absorbed power is not capped by instantaneous engine
output.

The bounded RK4/analytic-turbine/sampled-governor driver is mechanically copied
into a separate runtime to avoid widening law-1 errors or old replay terminals.
Shared state, derivative, stage, invalid-input and step-report types are reused.
Shared turbine, governor, aero, atmosphere, contact and rotation helpers are
unchanged. The distinct error/diagnostic plumbing, propeller evaluator and
post-propeller negative-flow guard are the only numerical dispatch differences.
Forward trajectory equivalence is tested bit for bit.

The call still selects 1..8 equal substeps once, with at most 41 evaluations,
no retries, hidden controls, additional state, heap allocation in the step,
or clocks/global randomness. Zero-duration calls validate without changing
state. All stages and the weighted endpoint must succeed before one final
whole-call commit.

The separate error namespace adds `NearStaticPowerBound`,
`OutsideNearStaticDomain` and `InvalidNearStaticScale`. The fixed-size diagnostic
record adds optional hover velocity, adverse ratio and transverse ratio only
after all are established finite. New flow checks occur after the old envelope,
power map and signed propeller query, before Mach-aero evaluation. No old reason
enum or serialization mask is broadened. Future encoding is a separate review.

## Provenance and acceptance boundary

The test-only Cedar component definitions and exact boundary state were
extracted from the original candidate at
`be5094dd49a88d5a3709c7435dca7bfca287e7df`. The copied boundary JSON retains all
16 canonical hexadecimal state words, original controls, calm atmosphere and
contact information. It is not a new production profile or parser.
The negative test rows use the original authoring continuation:

    CT = 0.5 * (beta - atan(J/(0.75*pi)))
    CPref = CT/2 * (J + sqrt(J² + 8*CT/pi))
    CP = 1.22*CPref + 0.005 + 0.01*beta

For negative J, CPref is only a near-static continuation reference. NASA and MIT
support the physical discussion, not these authored coefficient values. Stored
paired rows are interpolated at runtime. Tests also use simple independent
numerical maps, positive/negative shaft rotation, scalar power/torque/energy
oracles and explicit adjacent-outside domains.

The exact law-1 K2/substep2 negative-domain rejection and complete rollback
remain pinned on every platform. The historical J=-4.8781986805208105e-6 bit
reference is scoped to its x86_64 Linux GNU arithmetic. Every platform also
checks all nine optional diagnostic words against an independently reconstructed
same-runtime law-1 K2 state, including J computed directly from its signed body
velocity and absolute spin. This test-only observer uses public force queries
and explicit RK4/state updates, never the production step or private integrator.
The exact fixture inputs and physical bounds stay unchanged; no cross-platform
numerical tolerance is introduced. Law 2 must accept that identical attempt in
six substeps without editing physical state, controls, wind, gear or surface forces.
That one-step result is provisional. Aircraft acceptance additionally needs the
original 25 m/s braking scenario through an extended near-static interval,
brake release and renewed acceleration, 120/240/480/960 Hz convergence, every
encountered negative-stage flow ratio and the supported flight/contact matrix.
Limits must not be widened to pass those cases. The known approximately
0.02 m/s braked idle creep remains distinct from the still-open stationary
parking criterion (<0.001 m/s continuously for ten seconds).

Validation receipts are in [the pure-law QA report](../qa/near-static-turboprop-law2-2026-10-04.md).

## Primary mechanics references

- [NASA/TP-2005-213477, printed pp. 2–3](https://ntrs.nasa.gov/api/citations/20060024029/downloads/20060024029.pdf): adverse freestream, flow regimes and limits of momentum-theory estimates
- [MIT Unified Engineering §11.7](https://web.mit.edu/16.unified/www/SPRING/propulsion/notes/node86.html): forward-flow actuator-disk mechanics
- [NASA Glenn propeller thrust](https://www.grc.nasa.gov/WWW/K-12/BGP/propth.html): momentum context
- [ADR-0018](0018-bounded-running-turboprop.md): accepted running law 1 and its unchanged mechanics
