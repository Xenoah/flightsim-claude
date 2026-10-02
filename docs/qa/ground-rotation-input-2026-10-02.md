# Ground rotation and keyboard input verification (2026-10-02)

## Finding and scope

**An airborne release test does not establish runway takeoff behavior.** The
previous 0.8-per-second keyboard elevator candidate made a short airborne pull
less abrupt, but a half-second pull at the 65 kt EAS tutorial cue did not rotate
either bundled aircraft reliably. A slightly longer pull could unload the nose
wheel and then continue applying enough elevator to exceed the lift peak.

This follow-through tests the actual parked landing-gear state, acceleration,
nose-wheel unloading, rotation, and subsequent open-loop climb. The candidate
changes keyboard mapping and guidance, not aircraft lift, drag, inertia, gear
geometry, pitch stability, trim, or available engine power. Full elevator range
and genuine stalls remain available. These are generic simulator profiles and
**none of the speeds below is a published real-aircraft POH rotation speed**.

The diagnosis started from integrated source `e0267dd`. The opposing-key
correction is `e097f4f`. Earlier FDM equilibrium/climb evidence and primary
references remain in [longitudinal physics QA](longitudinal-physics-2026-10-02.md).
Raw telemetry, replay files, and execution logs are not included here.

## Ground mechanics and native reproduction

On level ground, the configured main wheels are aft of the centre of mass by
0.80 m (Light Single) and 0.70 m (Swift Sport). With no aerodynamic load, the
ideal static nose-wheel load fractions are 33.33% and 34.15%, respectively.
These are consequences of the modeled contact geometry, not measured aircraft
weight-and-balance data.

For a level, quasi-static approximation, the nose-up aerodynamic moment must
exceed `(aft main-wheel arm + wheel height * rolling friction) * (weight - lift)`
to unload the nose wheel. At 65 kt EAS and nominal mass this is approximately
5.53 kN m / 3.56 kN m, corresponding to effective elevator 0.574 / 0.550 at
zero angle of attack. This approximation is explanatory only: the integrated
run also contains strut damping, changing lift, angular acceleration, and
forward acceleration.

A 0.8/s ramp held for 0.5 s reaches only 0.49 / 0.48 effective elevator including
existing trim. Once rotation starts, however, the same substantial elevator is
no longer balancing the ground reaction. Continuing the pull or slowly
centering it can overshoot in the air. The gear force and pitch-moment signs
were checked; these results do not establish a mechanical sign defect or
justify moving the wheels or changing aerodynamic coefficients.

The flat-ground `Simulation::parked` probe agrees closely with the recorded
native Light run: a sampled 0.417 s pull at about 65 kt leaves pitch near 0.41°
and CG height 0.982 m, then returns to the ground roll. The native result was
approximately 0.43° / 0.983 m. Actual native liftoff occurred much later near
95.9 kt EAS under the remaining default trim. Native Swift's approximately
0.908 s pull did rotate, but its reconstructed peak AoA reached 19.32° after
release. Independent reconstruction matched all recorded keyframes in the
reviewed native runs. These native observations diagnose the 0.8/s candidate;
they are not verification of the later 0.25/s candidate.

### Original 0.8/s ground-pull duration sensitivity

Both profiles: calm, ISA, flat sea-level runway, nominal mass, clean, throttle
ramped from zero at 0.25/s, one pull starting at 65 kt EAS, then release to
unchanged default trim. Approximate results:

| Pull duration | Light Single | Swift Sport |
|---|---|---|
| 0.5 s | No sustained rotation; later natural liftoff near 96 kt | No sustained rotation; later natural liftoff near 100 kt |
| 0.75 s | Brief nose rise, then settles back into the ground roll | Liftoff near 73 kt; peak flight AoA 7.7° |
| 1.0 s | Liftoff near 69 kt; peak AoA 15.7° | Peak AoA 23.9° |
| 1.25 s | Peak AoA 34.5° | Peak AoA 38.3° |

The actual clean lift-curve peaks are about 13.12° / 13.15°. A fixed tap duration
is therefore inappropriate takeoff advice. Angle metrics exclude arbitrary
near-zero-speed spawn angles and are evaluated after the rotation-speed cue.

## Candidate keyboard mapping and guidance

- Elevator keyboard ramp: **0.25/s**
- Elevator keyboard centering: **5.0/s**; other axes retain their own rates
- Existing persistent trim: Light 0.09, Swift 0.08
- Training cue for these bundled profiles and stated conditions: **75 kt EAS**
- Pull until the **first visible nose rise**, then release and assess the climb;
  do not hold continuously to force the aircraft upward

The 3° pitch threshold is a **flat-runway test fixture** for visible nose rise.
It is not a universal target on sloping runways, whose initial attitude differs.
A 5° threshold was also tested as a later response, not recommended as a target.

This is a transparent input ramp and centering change. There is no flight-state
feedback in production controls, no automatic trim, no pitch lock, no altitude
floor, and no reduced maximum elevator. Neutral-to-full keyboard travel takes
4 seconds. Analog absolute authority is unchanged. The tradeoff is gentler
keyboard entry into large pitch commands; deliberate sustained pulls still
reach the full surface and stall the aircraft.

### Why these values

The ground sweeps included ramp rates 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, and 0.8/s;
centering rates from the existing 1.8/1.6 up to 3, 5, and 8/s; and 65/70/75 kt
EAS cues. Releasing at a measured cue was delayed by 0, 0.15, or 0.30 s to model
late observation/reaction. These delays are not a substitute for native input
sampling and rendering verification.

No rate in the original 0.4–0.8/s sweep with the old centering was robust to a
0.3 s delay at the 65 kt cue. Even 0.4/s at 75 kt, first-rise 3° plus 0.3 s,
reached 13.91° / 13.60°. Faster centering removes residual elevator after key
release without changing physical damping. The 0.25/s plus 5/s combination
provides more margin than 0.3/s plus 8/s for the delayed, later-response cases,
while retaining quicker full authority than 0.2/s.

### Candidate sea-level bounds

Actual parked gear state, clean, nominal mass, calm ISA, 75 kt EAS cue, then
unchanging full throttle and default trim for the rest of a 90 s run:

| Flat-runway pitch cue | Reaction delay | Light peak AoA | Swift peak AoA |
|---|---:|---:|---:|
| 3° | 0 s | 5.37° | 5.39° |
| 3° | 0.15 s | 7.90° | 7.83° |
| 3° | 0.30 s | 10.47° | 10.22° |
| 5° | 0 s | 7.15° | 6.98° |
| 5° | 0.15 s | 9.79° | 9.46° |
| 5° | 0.30 s | 12.13° | 11.60° |

All these cases lifted promptly, remained airborne, and climbed without a
stall or crash. The conservative warning may sound before the lift peak;
a below-peak result does not mean the warning is always inactive. The 3° fixture's pull duration ranges were 1.292–1.592 s for
Light and 1.183–1.483 s for Swift, including the specified delay. These ranges
help reproduce a controlled native experiment; they are **not universal tap
durations**. Earlier release and later release can produce different outcomes.

The final summary above also uses the actual updated `AxisState` and `RampAxis`
inside `Simulation::advance_with_controls`, with integer 18/36-step reaction
delays. This avoids adding an accidental extra step through floating-time
comparison. It reproduces the complete control ramp and parked-state physics.

The app regression
`bundled_ground_rotation_has_reaction_margin_and_unstalled_open_loop_climb`
uses the real profile's `pilot_controls`, `SampledPilotInput`, fixed-step
`advance_with_controls`, and `Simulation::parked`. Its eight cases cover both
profiles, 3°/5° cues, and 0/0.3 s delays. It requires actual liftoff within one
second of release, no return to ground, no crossing of the derived lift peak,
climb above 100 m after 90 s, zero transient elevator, unchanged trim, and
retained full throttle. No subsequent corrective input or test autopilot hides
an unstable open-loop result.

## Opposite-key response and recovery

Reducing the ramp exposed a separate input defect: pressing the opposite key
could unwind a positive deflection **more slowly than simply releasing it**.
At 0.25/s, a nearly full positive axis could remain positive for seconds while
W was held. The corrected `AxisState` returns toward zero at the faster of the
command/centering rates, consumes exactly that portion of the step, then uses
the ordinary command rate for any time remaining in the opposite direction.
This changes input mapping, not aerodynamic stabilization.

Recovery checks linked the actual updated `AxisState`, not only a handwritten
approximation. They start at 2000 m, 80 kt EAS, pitch 10°, flight path 5°, full
power, clean, and the profile's default trim. A deliberate pull is followed by
either release or a short W command. Approximate results for 0.25/s and 5/s:

| Profile | Deliberate pull | Peak AoA | Below initial altitude, release | Below initial altitude, 0.5 s W |
|---|---:|---:|---:|---:|
| Light | 3 s | 32.3° | 2.5 m | 2.3 m |
| Swift | 3 s | 31.3° | 0 m | 0 m |
| Light | 5 s | 42.9° | 109.2 m | 109.0 m |
| Swift | 5 s | 42.1° | 67.6 m | 67.4 m |

The two altitude columns mean **2000 m initial altitude minus the minimum
altitude over the run**. They are not descent from the pull-release altitude
or pre-recovery crest, which can be substantially greater, and must not be
read as required recovery clearance. All recovered in these high-enough
fixtures. This retains real stalls and height-consuming recovery; it does not
promise recovery after a low-level stall. Holding W too long can itself lose more height. Brief airborne pulls
of 0.1, 0.25, 0.5, and 1 s followed by release were also checked through the
actual API: maximum AoA was 8.78° / 8.55°, with no initial height loss in that
particular fixture. Those airborne results supplement, rather than replace,
the grounded tests above.

## Density altitude and weight limits

The same 0.25/s, 5/s, 75 kt EAS configuration was tested at 3000 m ISA using
`Simulation` on a flat elevated surface. A separate direct-FDM flat-ground
probe used the actual parked-state initializer, canonical gear/force equations,
and updated `AxisState` with an explicit ISA+20 K atmosphere at 2000 m. This
avoids pretending that monthly climate data are an exact prescribed ISA offset.
Its sea-level reference agrees with the `Simulation` probe. Mass sensitivity
increases mass 15% with the existing inertia/CG; it is not a validated loading
or balance model for a real aircraft.

Worst peak AoA across 0–0.3 s reaction delays at the **3° first-rise fixture**:

| Condition | Light | Swift | Interpretation |
|---|---:|---:|---|
| Sea-level ISA, nominal mass | 10.47° | 10.22° | Below lift peak; remains airborne |
| 3000 m ISA, nominal mass | 11.73° | 11.49° | Below lift peak; remains airborne |
| 2000 m ISA+20 K, nominal mass | 11.59° | 11.39° | Below lift peak; remains airborne |
| Sea-level ISA, mass +15% | 12.17° | 11.72° | Reduced margin |
| 2000 m ISA+20 K, mass +15% | 13.37° | 13.11° | Light crosses the peak; Swift has negligible margin |
| 3000 m ISA, mass +15% | 13.46° | 13.36° | Crosses lift peak at the longest delay |

At nominal mass, waiting until 5° and then another 0.3 s also crosses the lift
peak at 3000 m ISA (13.94° / 13.50°) and in the warm 2000 m case
(13.90° / 13.18°). Combined warm/high and increased mass can produce still
larger over-rotation and a ground return with that late cue. This is why the
guide must say **first visible nose rise**, and why the sea-level later-response
margin must not be advertised as valid for every load, field, weather, or slope.

## Verification boundaries

- Ground sweeps use the real `Simulation` and bundled dynamics, with one
  cue-triggered test pull followed by unassisted flight; they are not production
  autopilot code or a fitted flight-director test
- Actual input-API recovery/mistap checks include the opposing-key correction;
  the app-owned grounded regression covers the complete profile/control wiring
- The explicit warm/weight probe is direct FDM with the same flat-ground model,
  not an assertion that the monthly-climate path supplies exactly ISA+20 K
- The candidate does not remove stalls, guarantee takeoff on arbitrary terrain,
  or certify aircraft performance; high/weight/late-release failures above are
  retained as observed boundaries
- Native confirmation of the final profile/guide build, its real sampled key
  durations, frame cadence, and warnings is a separate integration acceptance
  step. Passing numerical fixtures alone is not that native confirmation
