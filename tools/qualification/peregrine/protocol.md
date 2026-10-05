# Peregrine Sport Jet: frozen first-pass qualification protocol

Frozen 2026-10-04 before numerical qualification. Base source is
19f90a9140b8bd8caaafd65ef296a41a440f7103. This is an original fictional
experimental profile, not a measured aircraft, certified envelope, stealth
capability, app preset or manual handling demonstration. No mesh is supplied.
Profile-v2, dry-jet law 1, identity schema 2 and replay-v4 are unchanged.

## Authoring basis

`author_profile.py` is the complete reproducible specification. It authors
every coefficient explicitly; none is a real-aircraft polar. The 1,750 kg fixed
mass comes from six uniform-box idealizations. Their mass-weighted design CG
is translated to body origin; the parallel-axis theorem gives all three inertias.
The coplanar centered boxes imply Ixz=0. This idealization is internally
consistent but not a mass survey. Wing area 13.2 m², span 8.4 m and taper .45
define the root/tip chords and exact trapezoid MAC. Wing mass uses an approximate
rectangular box separately, not a claim of exact trapezoidal mass distribution.
Fixed fuel mass, no burn, no spool, no gear retraction, no actuator/structure model.

An authored CD0=.025 includes .006 allocated to exposed fixed gear, .010 body,
.007 wing/tail skin/form and .002 interference. These are design assumptions,
not measured buildup. The slope starts4.6/rad for this finite-span wing and
rises to5.2; induced drag uses the existing e(M), AR law. The modest drag rise,
stability, damping and effectiveness changes are deliberate bounded schedule
assumptions. They do not model shocks, buffet, Reynolds or aeroelasticity.

The installed-net dry map is
12500*p*(1-.20*M-.12*M²)/sqrt(T), the idle map p*(500-900*M)/sqrt(T),
evaluated only when generating stored knots. Runtime trilinear interpolation
of those knots is authoritative. No second ram drag or engine multiplier.
Static idle500N is an explicit4%-of-dry experimental assumption, with a
frozen250/500/1000N static sensitivity range (2/4/8% of dry). No real-engine
idle claim follows from this range. The initial proposed20N static idle was
rejected in review BEFORE any run because deriving idle from the parking-force
budget would calibrate around the known tanh-contact limitation. It was never
qualified. Nonzero idle can produce persistent creep under the existing smooth
friction law; retain failure honestly without lowering the strict parking gate.
Gear and friction are frozen independently of the parking result.

Primary references support principles only:
- https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/drag-coefficient/
- https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/thrust-force/
- https://www.faa.gov/regulations_policies/handbooks_manuals/aviation/phak

## Frozen gates

No threshold below may be relaxed to pass this profile. Keep all unsuccessful
cases. An unreachable trim is a classified result, never silently omitted.
Changes to profile assumptions require a new version of the evidence directory,
an explicit reason and comparison against this untouched first-pass record.

1. Parse through exact-token v2 loader. Independent Python identity bytes must
   equal Rust canonical bytes; every original tracked file remains byte-identical.
2. Independent algebra for force/moment and tensor-product thrust at static,
   low/cruise/max Mach, knot-adjacent and off-axis conditions: absolute error
   <=1e-7 N/Nm or relative1e-11 (whichever larger), no negative drag/nonfinite.
3. Trim 0/3000/6000m × Mach.2/.35/.5/.55/.6; use ground far below for altitude0
   pure tests. Separate acceleration and angular residual<=1e-6. Cruise claims
   require throttle<.9, |elevator|<.5, |alpha|<=8deg. 60s held controls require
   altitude drift<=10m, speed drift<=1.5m/s. Report600s trajectories and boundedness
   (no terminal; altitude deviation<250m, speed deviation<15m/s, |bank|<45deg).
4. Rates120/240/480Hz over identical frozen-environment10s cases: max shared
   state errors120-vs480<=.1m position/.01m/s velocity/.01deg orientation.
   240-vs480 error must not exceed120-vs480 except roundoff floors
   1e-6m/1e-7m/s/1e-7rad. Contact and knot-crossing order reductions are classified.
   Include local12x12 finite-difference Jacobians at high-q points and two
   perturbation step sizes. No growing ordinary mode>=.05/s accepted; slower
   spiral/phugoid modes reported with600s response. No hidden stabilizer.
5. Positive/negative elevator/aileron/rudder pulses(.02 for.25s) and release,
   full surface commands(.1s), throttle0/1(3s), flap0/1 cases at nominated
   pressure/Mach extremes. Check sign, finite state, domain and response. Track
   excursions beyond8deg alpha/5deg beta/45deg bank as unqualified handling.
6. Stall peak and warning margin0.85*actual positive peak, flaps0/.5/1 at all
   knots. Authored initial poststall and recovery sequence must regain alpha
   below warning within15s without terminal and lose<500m in30s. Full script
   disclosed; no manual-control claim.
7. Stationary parking means ALL ground velocity components<.001m/s continuously
   10s after deceleration, matching existing Cedar criteria, not rounded0kt.
   Test rest settling and25m/s braking separately, each60s; no state reset.
   Retain10m/s head/5m/s crosswind and2deg slope outcomes under same gate.
   Report stopping distance, each leg compression, max roll/pitch and creep.
   Taxi with.08 throttle10s then brake: acceleration>1m/s, stationary gate;
   no qualified parking if only braking/taxi passes.
8. Scripted takeoff60s must achieve>100m AGL and airborne; approach from60m at
   60m/s, half flaps,2m/s descent must contact within40s with sink.5–4m/s.
   Brake after contact and meet stationary gate; go-around from same approach
   must achieve positive climb and>100m altitude within30s. Scripts explicit.
9. Inclusive domain edges/adjacent outside, high-q dive through upper Mach,
   initial and intermediate-stage rollback. Failed attempts preserve all13 state
   words; host clock/controller/log/contact/recorder prefix exact. Cover all
   observable stage reasons or mark missing; never invent stage coverage.
10. v4 encode/decode/encode bytes exact, full-state checkpoints/final exact,
    terminal exact including cursor0, repeated seeks and pause/restart, max240
    attempts per call. Existing replay/model/source contracts unchanged.

Full nominal qualification does not cover weather/climate/manual/native/GPU/
Windows/structural behavior or authorize preset admission. First pass must report
blockers before prolonged tuning. If law/domain/integrator changes are needed,
stop and propose a separately versioned contract rather than modifying law1.

## Before-run independent review correction

The initial authored positive rudder side force and positive yaw moment were
inconsistent with an aft tail. Before any numerical run the signs and magnitude
were corrected: CYrudder=-.085→-.080, Cnrudder=.029→.027, Clrudder=-.009→-.008.
The equivalent aft lateral-force arm is b*Cn/CY≈-2.8 to-2.9m, near the tail-box
center -2.82m aft of CG. The negative rolling moment corresponds to lateral
force at a fin above CG. Controls here are normalized commands, not radians.
The previous commit is retained for review but is not a qualified profile.

Exact scenario scripts/initial states and nominated points are frozen in
`crates/flightsim-sim/examples/peregrine_qualification.rs` before compilation.
They are time-scripted numerical tests; their success could not establish manual
piloting. Convergence uses the same frozen environment at every external rate.
