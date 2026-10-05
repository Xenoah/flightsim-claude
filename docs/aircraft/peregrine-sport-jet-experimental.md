# Peregrine Sport Jet: experimental numerical qualification

**2026-10-05. Mach 0.55 cruise is demonstrated at 0, 3,000 and 6,000 m in calm
ISA conditions with held controls. Stationary parking fails, and controlled
takeoff, sustained stall recovery and go-around remain unqualified.**

Peregrine is an original, fictional, uncalibrated dry-jet profile. It is an
experimental numerical prototype, with no visual model, cockpit, app registration
or aircraft preset. No stealth capability, weapon, measured-aircraft fidelity,
manual-flight acceptance or release acceptance is claimed.

The authored domain ends at Mach 0.65, but **the complete domain is not
qualified**. Tests near its limits establish numerical behavior at specific
points, not a complete operating envelope or structural limit. Existing aircraft
profiles, models, physics laws, replay contracts and fixtures remain unchanged.
The profile uses dry-jet law 1, profile version 2, identity schema 2 and replay
version 4. Its fingerprint is `63db300941ead277`.

## Source and reproduction

- [External experimental profile](../examples/aircraft-profiles-v2/peregrine-experimental.json)
- [Profile generator](../../tools/qualification/peregrine/author_profile.py)
- [Frozen assumptions and qualification gates](../../tools/qualification/peregrine/protocol.md)
- [Pure Rust qualification harness](../../crates/flightsim-sim/examples/peregrine_qualification.rs)
- [Independent identity, polar and mode analysis](../../tools/qualification/peregrine/analyze.py)

The profile deliberately names `aircraft/peregrine-not-provided.glb`; no model
is supplied. Presentation and control metadata are provisional numerical hints.
The harness independently solves the tested half-flap, 60 m/s approach with
elevator 0.06798130988679493, throttle 0.15842182441037578 and pitch
−0.028882519921364425 rad. The profile's initial generic approach hints are not
an accepted app initialization. Names and visual metadata do not select forces.

Run the numerical qualification and independent analysis from the repository:

```sh
cargo run -j2 -p flightsim-sim --example peregrine_qualification -- \
  docs/examples/aircraft-profiles-v2/peregrine-experimental.json /tmp/peregrine-full
cargo run -j2 -p flightsim-sim --example peregrine_qualification -- \
  docs/examples/aircraft-profiles-v2/peregrine-experimental.json /tmp/peregrine-extra diagnostics
python3 tools/qualification/peregrine/analyze.py \
  docs/examples/aircraft-profiles-v2/peregrine-experimental.json \
  /tmp/peregrine-full /tmp/peregrine-extra
```

Rust 1.93.0 compiled the qualification harness with warnings denied; Clippy with
the configured project lints and formatting checks passed. The separate Python
numerical analysis uses NumPy. The evidence records the actual library
hashes, Cargo dependency fingerprints and all 100 relevant pure-source hashes.
The independent audit verified that binding and repeated the existing executable;
it did not independently rebuild the compiler output. This qualification is not
a full-workspace test run, CI result, native test or performance benchmark.

## Retained evidence and independent verification

The current evidence archive is `peregrine-fresh-evidence-1a2161e.zip`, containing
34,304,451 bytes, with SHA-256:

```text
a83a43e704d588fb99449bdca42d6378c5148527d5faef4ed8cf66bba43768c0
```

It contains the source, reports, summaries, build and analysis logs, replay files,
canonical identities and 133 named raw traces. Each of the 122 unique trace
payloads is stored once as a lossless gzip blob. `trace-map.json` preserves every
original trace path, size, SHA-256 and permission mode; `files-manifest.json`
covers the other payloads. The retained traces sample at 20 Hz and include all
13 rigid-body state words. The convergence harness separately compares all
shared 120 Hz endpoints internally.

After extracting the archive, run `python3 restore_evidence.py NEW_DIRECTORY`.
The destination must not already exist. The restorer verifies the compressed and
raw hashes, reconstructs every named trace and restores its original mode. The
analyzer also accepts ordinary raw or gzipped trace files.

The independent audit verified all 165 manifested payloads, restored and checked
all 133 named traces, and reproduced the archived Python analysis byte for byte.
It then completed a separate full run of the exact bound executable. All 131
result files matched the fresh originals byte for byte: 122 trajectory files,
four canonical identities, three replays and two summaries. This independent
repeat is documented in `peregrine-independent-audit.md` and its accompanying
receipt; it is additional evidence outside the sealed original ZIP. It does not
compare against the lost October 4 artifacts. No aircraft values were tuned
between these runs.

The October 4 evidence was lost in an environment reset before durable storage.
The source was recovered, with the profile and complete Rust harness matching
their retained original SHA-256 hashes. The results reported here come from the
fresh October 5 qualification and independent repeat, not recovered old traces.

## Configuration and physical assumptions

The generator defines a fixed mass of 1,750 kg, wing area of 13.2 m², span of
8.4 m and taper ratio of 0.45. The resulting root and tip chords are
2.167487684729064 m and 0.9753694581280788 m, mean aerodynamic chord is
1.6467923673631164 m, and aspect ratio is 5.345454545454546. The 9.1 m length
is a prospective visual dimension.

Six authored uniform-box components place the design CG at x =
−0.2782857142857143 m and give these inertias about the CG:

| Inertia | kg m² |
|---|---:|
| Ixx | 2081.230666666667 |
| Iyy | 5154.62219047619 |
| Izz | 6974.15819047619 |
| Ixz | 0 |

Their common design midplane gives zero cross inertia. Positive definiteness
and the inertia triangle inequalities are satisfied. This mass distribution is
an idealization, not a measurement or a property inferred from an unbuilt mesh.

The fixed gear contacts are body coordinates (+2, 0, 1.15),
(−0.85, −1.5, 1.15) and (−0.85, +1.5, 1.15) m. Spring, damper and contact values
are frozen in the generator. The gear remains down during every force and
contact evaluation. CD0 starts at 0.025, including an explicit 0.006 allocation
for exposed fixed gear, and rises to 0.033 at Mach 0.65. All 25 coefficients are
authored assumptions scheduled at Mach 0, 0.2, 0.35, 0.5, 0.6 and 0.65; they are
not copied Kestrel polars. No singular compressibility factor, shock, buffet,
Reynolds-number or aeroelastic model is implied.

Aggregate installed maximum dry thrust is 12.5 kN at standard sea-level static
conditions. The stored knots model pressure, temperature and Mach effects;
runtime trilinear interpolation is authoritative. Central static idle is 500 N
(4% of maximum dry thrust). The 250, 500 and 1,000 N cases, corresponding to
2%, 4% and 8%, are frozen exploratory sensitivities, not measured engine data.
Signed idle net thrust at high Mach is allowed. The model has no additional ram
drag, engine-count multiplier, fuel burn, thrust offset, spool dynamics or hidden
actuator.

The proposed 20 N idle was rejected before testing because choosing idle from a
parking-force budget would conceal the smooth-friction limitation. It was never
qualified. Before the first numerical run, the rudder side-force and roll signs
and yaw magnitude were corrected to imply an aft force arm of approximately
2.8–2.9 m, consistent with the tail box. These design choices were frozen before
the numerical results; the profile was not tuned afterward.

NASA's [drag-coefficient explanation](https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/drag-coefficient/)
supports the qS reference-area, induced-drag and Mach/Reynolds distinctions.
Its [thrust equation](https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/thrust-force/)
supports the installed-net convention. The [FAA handbook](https://www.faa.gov/regulations_policies/handbooks_manuals/aviation/phak)
is a primary aerodynamics and flight-controls reference. None supplies this
aircraft's coefficients or validates the authored model.

## Trim, cruise and numerical behavior

All 15 combinations of altitude 0, 3,000 and 6,000 m with Mach 0.2, 0.35, 0.5,
0.55 and 0.6 trimmed. Each passed the separately checked acceleration residual
gate of 1e-6 m/s² and angular-acceleration residual gate of 1e-6 rad/s², the
60 s altitude/speed drift limits of
10 m and 1.5 m/s, and the 600 s bounded held-control check. The worst 600 s
altitude deviation was 9.07174 m and speed deviation was 0.08934 m/s. Controls
remained unchanged throughout; no trajectory driver or stabilizer held the
flight. Sea-level airborne cases use a synthetic ground plane at −1,000 m to
exclude contact forces.

| Altitude | Mach 0.55 throttle | Mach 0.60 throttle | Cruise-margin result |
|---|---:|---:|---|
| 0 m | 0.744150 | 0.941312 | 0.55 passes; 0.60 exceeds the <0.90 throttle gate |
| 3,000 m | 0.725461 | 0.914780 | 0.55 passes; 0.60 exceeds the <0.90 throttle gate |
| 6,000 m | 0.714641 | 0.894601 | Both pass; 0.60 has little additional margin |

The four perturbed 10 s trim cases at 0 m/Mach 0.2, 3,000 m/Mach 0.55,
0 m/Mach 0.6 and 6,000 m/Mach 0.6 passed the 120/240/480 Hz convergence gates.
Each case had to complete its full duration. Maximum differences between
120 Hz and 480 Hz were 2.52e-8 m in position, 2.71e-7 m/s in velocity and
2.42e-6 degrees in orientation. Finer errors decrease or fall within the declared
roundoff allowances. These small errors do not establish a measured fourth-order
convergence rate when quaternion-angle and ECEF comparisons are at roundoff.

The separate untrimmed case near the domain boundary, at pressure ratio 1.07,
temperature ratio 0.76 and Mach 0.645, begins at 31,573.146 Pa dynamic pressure.
It completes all three rates within the same error limits and remains inside
the domain, but decelerates by 6.54 m/s in 10 s: **it is not a cruise point**.
The exact domain corner at 32,364.22 Pa lacks complete local-control and
convergence qualification. The mathematical domain is not a structural
pressure or load limit.

Finite-difference 12 × 12 local Jacobians were evaluated at all 15 trim points
using two perturbation sizes. Additional re-trimmed probes on each side of Mach
knots avoid averaging different schedule slopes. No sampled local mode shows a
positive growth rate of 0.05/s or greater. Near 6,000 m/Mach 0.2, however, a slow
positive mode reaches 0.00033551/s, approximately a 2,981 s e-folding time.
**Bounded unperturbed flight for 600 s does not qualify that slow mode as stable.**
A long perturbed-mode test remains open. Halving the one-sided finite-difference
step changes the nearest eigenvalues by at most 1.03e-9/s. Near the high-pressure
domain corner, the sampled local eigenvalues have no positive growth beyond
roundoff; the fastest magnitude is 17.9622/s.

Independent tensor-product thrust and force/moment algebra agree within
1.82e-12 N for thrust, 5.83e-11 N for force and 7.28e-12 N m for moment across
40 queries. The force reference consumes `cfg.aero().sample` coefficient values,
so it independently checks force/moment algebra, not Mach-coefficient
interpolation. Independent verification of every scheduled coefficient between
knots remains open.

Independent positive- and negative-alpha polar checks agree within 8.89e-16
across 180 cases. Independent scans and refinement locate all 18 positive lift
peaks within 1.73e-9 rad of the runtime helper. The proposed warning at 85% of
the peak angle precedes the actual peak. This is candidate diagnostic evidence;
no app warning behavior was added.

All 64 pulse, throttle and flap runs complete without terminal rejection. Full
surface pulses use the exact ±1 endpoint for 0.1 s; smaller commands use ±0.02
for 0.25 s followed by release. **14 of 64 responses exceed the proposed ordinary
handling limits of |alpha| ≤ 8 degrees or |beta| ≤ 5 degrees.** The maxima are
18.9438 degrees alpha and 6.47492 degrees beta; no case exceeds 45 degrees bank.
Terminal-free completion does not establish ordinary handling acceptance.

## Parking and maneuver failures

Strict parking requires every ground-velocity component to remain below
0.001 m/s for 10 continuous seconds after deceleration, and the complete 60 s
run must finish. Rounded 0 kt or movement below 1 m does not satisfy that gate.
None of the six calm sensitivity/start combinations passes.

| Static idle | Final creep from rest | Final creep after braking from 25 m/s |
|---|---:|---:|
| 250 N | 0.00508479 m/s | 0.00508479 m/s |
| 500 N | 0.01017218 m/s | 0.01017218 m/s |
| 1,000 N | 0.02037062 m/s | 0.02037062 m/s |

The 500 N case also fails with a 10 m/s headwind plus 5 m/s crosswind, and on a
2-degree slope. Its final ground-velocity components reach 0.00943285 m/s and
0.00198112 m/s respectively. The unchanged law's tanh friction produces these
equilibrium creeps. Deceleration, taxi acceleration and approach contact do not
establish stationary parking. Reducing idle or changing friction regularization
to force a pass would conceal the limitation. An actual static-hold/contact
improvement requires a separately reviewed, versioned physical law and
identity/replay contract; none is proposed or changed here.

The solved half-flap, 60 m/s approach contacts the ground at 30.35 s with a
1.86583 m/s sink rate, passing the original contact-only gate. Braking is triggered
by CG height below 1.2 m, not a true contact latch. The aircraft then decelerates
to the same failed idle creep. A complete ordinary landing and parking workflow
is not qualified.

The open-loop takeoff, recovery and go-around scripts do not establish ordinary
maneuvers despite satisfying simple altitude or transient gates:

- Takeoff exceeds 100 m but later reaches extreme angle of attack
- Poststall alpha first falls below the conservative warning in 0.35 s, with
  less than 500 m altitude loss, but recovery is not sustained; after large
  attitude excursions, the run ends near 21.6 m/s
- Go-around ends above 100 m with positive climb but only 15.98 m/s airspeed

**Those simple gates are insufficient for operational acceptance.** Their
numerical passes are recorded separately from the failed controlled-maneuver
claim. No scripted run is manual handling evidence, and no prolonged tuning was
performed after these failures.

## Identity, replay and domain boundaries

The independent Python identity-schema-2 encoder matches the fresh Rust canonical
bytes and fingerprint exactly. Each idle sensitivity stores its own complete
canonical bytes and distinct identity. The 1,200-frame cruise recording,
cursor-zero terminal recording and high-speed dive recording all
export/decode/export byte-identically. Full-state replay, terminal reproduction,
pause, restart and repeated seeks pass with at most 240 attempts per call.

The dive rejects at K2 after one committed frame. The failed attempt preserves
the full host `JetSnapshot` and controller by equality. The host comparison uses
`PartialEq`; explicit bitwise assertions cover the 13 rigid-body words in replay
and direct FDM rollback witnesses. Recorder-prefix preservation is supported by
frame count and exact recording/replay behavior, not an independent before/after
byte-prefix comparison.

Boundary probes retain both requested and actual pressure, temperature and Mach
values because altitude/ISA conversion can round a requested boundary to either
side. The bounded search obtains Initial, K2 and K4 rejection witnesses, all
with exact 13-word rollback. **K1, K3 and weighted-Endpoint witnesses were not
obtained for this profile.** Existing general-law tests are not relabeled as new
profile coverage. Complete stage and exact-boundary qualification remains open;
no runtime domain was weakened to obtain a witness.

Existing physical laws, v1–v6 replay formats, model assets, profiles and golden
fixtures are byte-preserved.
The full legacy test suites were not rerun for this numerical qualification.

## Scope of the milestone

Peregrine remains an experimental numerical prototype. The existing law has
adequate evidence for the nominated trim and near-boundary integration cases,
without demonstrating a need for new high-speed arithmetic. Mach 0.55 has tested
cruise margin; Mach 0.60 is altitude-dependent and marginal at 6,000 m.

Preset admission remains blocked. The full Mach 0.65 domain and a complete
flight workflow are not qualified. Strict parking, controlled maneuvers,
perturbed slow-mode response, complete domain-corner and stage coverage,
weather-flight coverage, and all visual, native, manual and Windows checks remain
separate future gates. No visual model or preset is included in this milestone.
