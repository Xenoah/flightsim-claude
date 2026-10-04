# ADR-0018: Keep a running turboprop's complete state and power balance explicit

- Status: pure foundation implementation; independent acceptance and host integration pending
- Date: 2026-10-04

## Decision and scope

Add `flightsim-fdm::turboprop` as a separate physical family, kind 3 and numerical
law 1. Its complete state is the unchanged 13-scalar rigid body plus modeled
idle-to-maximum turbine fraction x, positive relative shaft speed omega (rad/s),
and blade pitch beta at 0.75 radius (rad). x is a validated bounded type, never
measured N1. Immutable component definitions are raw SI configuration boundaries;
public runtime physical quantities use core unit wrappers. Legacy propeller
revision 2 and dry-jet revision 1, their state, arithmetic and bytes are unchanged.

This foundation adds no aircraft preset, profile/identity/replay dispatcher,
world sampler, app, asset, sound or renderer integration. Future hosts must
record and restore all 16 scalars and commit environment/contact/control/clock
history only after successful full-state steps. Calling `step(0)` checks an
actual start environment without changing any state bits. Construction and
`set_state` alone establish structural validity, not environmental support.

## Physical law

Turbine maps contain paired idle/max output-shaft watts on common pressure and
temperature axes. Interpolation is temperature then pressure; x interpolates
between the resulting idle/max powers. Static pressure is divided by 101325 Pa;
absolute static temperature by 288.15 K. There is no density exponent, exhaust
thrust, gearbox multiplier, fuel or additional spool state.

- dx/dt = (u-x)/tau, rise tau when u>x, fall tau otherwise
- Qd = min(output torque limit, Pavailable/omega); Pdelivered = Qd*omega
- Rotation sense s is ±1 by the right-hand rule about body +X
- Absolute axial spin Omega = omega+s*p must lie in its positive authored interval
- n = Omega/(2*pi) in rev/s; J = Va/(n*D), signed axial air-relative Va
- T = CT*rho*n²*D⁴; Pp = CP*rho*n³*D⁵; Qp = Pp/Omega

Paired CT/CP are interpolated pitch then J on one rectangular grid, never
independently clipped or extrapolated. Only `isolated_axial_propeller` is
admitted. Airframe baseline drag contains the unpowered body/nacelle. Installed
effective-thrust maps require a new reviewed convention. Signed braking cells
do not imply reverse flow, feathering, shutdown or validated engine-out behavior.

Every cell satisfies CP-J*CT >= -64*epsilon*max(1,abs(CP),abs(J*CT)) continuously:
check each pitch endpoint's quadratic in J at both ends and its interior minimum.
CP is nondecreasing with pitch at each J knot. Additionally, at every node AND
every runtime interpolated query with CT>0:

    CPideal = CT/2 * (J + sqrt(J² + 8*CT/pi))
    epsilon_disk = 64*epsilon*max(abs(CP),abs(CPideal))

Reject CP<=0 unconditionally (both zero signs), nonfinite/zero derived ideal
power, or CP<CPideal-epsilon_disk. This is the unducted axial finite-disk
assumption. It compares absorbed aerodynamic power, not instantaneous engine
power: a slowing rotor may supply stored kinetic energy. Nodal/sampled success
is not a continuous certificate for this stronger nonlinear inequality. The
pointwise runtime guard is mandatory and rejects the whole step on violation.

Locked complete-aircraft inertia is I_L. Represented rotor axial inertia is I_r,
and I_B=I_L-I_r*ex*exᵀ is separately checked for the same relative XZ conditioning
margin, finite inverse and the existing determinant assertion. With body rates b,
non-propeller moment Ma and H=s*I_r*omega*ex:

    I_B * b_dot = Ma - s*Qd*ex - b × (I_L*b + H)
    omega_dot = (Qd-Qp)/I_r - s*b_dot.x

Thrust acts through the CG. No extra -s*Qp reaction is applied to the body.
I_B also supplies the effective rotational mass in contact-frequency estimates;
using locked I_L there would underestimate contact response. Shared aero, gear
forces, gravity, atmosphere and core coordinate helpers remain unchanged.

The independent mechanical check is
E=0.5*b·I_L*b+s*I_r*omega*p+0.5*I_r*omega² and
E_dot=Ma·b+Qd*omega-Qp*Omega. Total angular momentum changes by Ma-s*Qp*ex.

## Integration and bounded work

The host cadence remains 1/120 s. A public call accepts 0..8/120 s and selects
1..8 equal substeps once, without retry. At each substep start freeze
r_beta=clamp(gain*(omega-reference),-fine_rate,coarse_rate). At RK4 abscissae
0,h/2,h/2,h use exact held-command x=u+(x0-u)*exp(-t/tau) and saturated
beta=clamp(beta0+r_beta*t,minimum_pitch,maximum_pitch). Only rigid-body and
relative-shaft dynamics are integrated by RK4; every force stage uses its own
actual x, beta, omega, atmosphere and relative wind. The endpoint is separately
evaluated, including clearances, before commit. This sampled hold is part of
law 1; it does not claim fourth-order convergence of a continuous governor.

The initial substep count is ceil(max(1, duration ratio, body/contact ratios,
dt/min(tau)/.05, dt*abs(omega_dot)/min_supported_omega/.05,
dt*abs(Omega_dot)/min_supported_Omega/.05,
dt*max(pitch_rates)/(pitch_stop_width)/.05)). Contact uses the existing maximum
gear phase and initial lookahead dt; stage checks use current contact. Every
stage/endpoint rechecks its h ratios against 1+32*epsilon. Later growth rejects
rather than silently retrying, clipping or enlarging work. Rotor blade azimuth
is not a resolved coordinate and Omega itself is not a body phase limit.

One initial plus five force evaluations per substep gives at most 41. The
stepping call graph uses immutable preallocated tables, stack values and fixed
arrays, with no heap allocation or global state. This is a structural work/
allocation bound, not an allocator measurement, wall-time guarantee or app FPS.
The entire external call commits once; any failure preserves all original bits.

## Frozen diagnostics and precedence

Error stages are Initial=0, K1=1, K2=2, K3=3, K4=4, Endpoint=5; substeps are
zero-based 0..7 and Initial always has substep 0. Optional diagnostics appear
only after their finite values are established: pressure, temperature, Mach,
J, beta, relative shaft, absolute spin, tip Mach, crossflow ratio. Error values
are fixed-size and carry no heap allocation. A future codec must validate and
explicitly encode these values, not serialize Rust enum memory.

Closed invalid-input detail codes: TimeStep=1, State=2, Quaternion=3, Position=4,
AtmosphereOffset=5, Ground=6, Wind=7, RelativeVelocity=8,
AtmosphereTemperature=9, AtmosphereDensity=10, TurbineFraction=11,
ShaftRate=12, BladePitch=13, ComponentQuery=14, Derivative=15,
IntermediateState=16. Power-bound detail codes: NonpositivePower=1,
BelowIdealDisk=2, InvalidDerivedBound=3. Other distinct reasons are outside
atmospheric altitude, outside authored seven-axis envelope, outside power map,
outside propeller map, outside Mach-aero schedule, and substep budget exceeded.
Existing jet/replay reason enums are not broadened.

Precedence: duration and outer budget; raw rigid and engine state; geometric
altitude; atmosphere offset, ground then wind; relative velocity; atmosphere
temperature/density and sound/pressure; finite ratios/absolute spin; authored
pressure/temperature/Mach/relative/absolute-shaft intervals; derived tip and
crossflow checks; power table; finite J and propeller domain; paired disk bound;
aero schedule; derivatives; stage-resolution budget. Finite out-of-profile shaft
speed is rejected before computing J. All raw vectors are checked before norms.

## Admission bounds and limitations

Power axes each have 2..16 knots (<=256 cells); propeller axes each 2..32
(<=1024 pairs); spacing is >=1e-9. Pressure ratios 0.1..2, temperature 0.25..2,
watts 0..5e6 with idle<=max, time constants 0.1..30 s, torque cap 1..100000 Nm.
J 0..4 starts at zero; pitch 0..pi/2; CT/CP -2..2. Diameter 0.5..6 m; rotor
inertia 0.1..1000 kg m². Governor gain .0001..10; rates .001..1 rad/s; reference
and shaft intervals 20..1000 rad/s; stops increasing >=1e-9. Tip Mach is (0,.8],
crossflow ratio (0,.1]. Envelope fits component tables, reference fits relative
shaft interval, and stops fit pitch axis. These are numerical admission bounds,
not operating ratings. Serialization resource/depth/exact-number enforcement is
the later host decoder's responsibility.

Running-only, one equivalent rotor, fixed gear/mass, forward axial inflow and
bounded crossflow. No start/stop, thermal limits, gas-generator inertia, gearbox
spools, reverse/beta range, feathering, fuel, damage, disk inflow state, swirl,
P-factor, slipstream lift, compressible-tip correction or real-aircraft fidelity.
Tests use original numerical fixtures. No measured engine map or third-party
implementation is copied, and no named aircraft/preset is qualified here.

## Mechanics references

The physical conversion is independently written from the momentum/ideal-disk
mechanics in [NASA Glenn](https://www.grc.nasa.gov/WWW/K-12/BGP/propth.html) and
[MIT Unified Engineering §11.7](https://web.mit.edu/16.unified/www/SPRING/propulsion/notes/node86.html).
[JSBSim propeller documentation](https://jsbsim-team.github.io/jsbsim/classJSBSim_1_1FGPropeller.html)
is conceptual context for paired coefficients and rotational loads, not imported
code or a source of this fixture's numerical values. Measured propeller data
transcription and full authored aircraft qualification remain later gates.
