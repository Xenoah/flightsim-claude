# Running turboprop pure foundation: numerical qualification

Date: 2026-10-04. Base: `b38f41e94b6231037c54c538a5b99c33b19ca7dc`.
Initial API checkpoint: `cbe064f973457953910e6f59edfac94308edb684` (explicitly
unaccepted pending the later checks and reduced-inertia boundary fix below).
Final tested source is the tree containing this report, on the isolated
`agent/turboprop-fdm-foundation` branch. No GitHub publication is part of this work.

## Scope and evidence

[ADR-0018](../adr/0018-bounded-running-turboprop.md) fixes the new kind-3/law-1
mechanics, state, rejection precedence and bounded stepping. The implementation
is additive core SI wrappers and FDM modules. It changes no old physical-law
arithmetic, physical preset, identity stream, replay codec, wind, world sampler,
app, renderer or old golden fixture. A separate host integration must preserve
all 16 state scalars and its own transactional clocks/controllers/contact history.

The numerical fixtures and benchmark JSON are original authored approximations.
They are not Cedar parameters, measured engine data, flight handling acceptance,
a real-aircraft comparison or a reusable measured propeller dataset. No copied
third-party implementation is present. No browser/native/Bevy/Windows test is
claimed by this report.

## Independent component checks

- rho=1 kg/m³, D=2 m, n=25 rev/s, CT=.1, CP=.08 produces 1000 N,
  40000 W and 254.64790894703253 Nm at both static inflow and J=.5;
  propulsive efficiency is respectively 0 and .625. Separate signed-load cases
  check braking signs, without claiming engine-out behavior.
- An asymmetric bilinear surface with cross terms tests paired interpolation,
  independent axis spacing and exact authored knot bits. Invalid shape, ordering,
  coefficient, feedback-sign and query cases are rejected.
- Continuous CP-J*CT checking rejects a map with acceptable endpoints and an
  interior negative quadratic minimum. A separate map CT=.2 at J=0, CT=0 at
  J=2 and CP=.1 throughout passes construction but fails the finite-disk bound
  at J=1 (CT=.1, CPideal=.10600553340847296). Its actual runtime failure retains
  valid J/pitch diagnostics and the complete pre-call state.
- Static ideal CP for CT=.1 is .025231325220201606. Tests distinguish exact/
  within-tolerance/rejected-side admission, unconditional rejection of both zero
  signs when CT>0, and derived-bound underflow. A 17×17 tensor grid over every
  cell of the original positive-thrust fixture has minimum sampled margin
  .009968219511354546 at (J=2,beta=.1 rad). A near-zero signed CT transition is
  also sampled. This is sampled coverage; runtime enforcement remains mandatory.
- Exact turbine rise/fall lag is checked against exp(-1) and exp(-3) residuals,
  one-second command history, monotonicity and zero-duration signed-zero bits.
  Independently evaluated bilinear powers and torque cap obey delivered=Qd*omega.
- Governor rate, exact time to each pitch stop, stop release, zero speed error
  and zero-duration behavior are independently checked. No shaft speed is assigned.
- Rational rotational oracle: I_L=[[10,0,-2],[0,12,0],[-2,0,15]], I_r=3,
  b=(.2,-.3,.4), omega=100, Qd=50, Qp=20, Ma=(1,2,3). For s=+1,
  b_dot=(-22617/2525,-489/50,-704/101), omega_dot=47867/2525 and
  E_dot=2996.8 W; for s=-1, b_dot=(23883/2525,511/50,756/101),
  omega_dot=49133/2525 and E_dot=3004.8 W. Both give Omega_dot=10 rad/s².
  Independent momentum checks cover zero external moment/load and zero drive.
  Changing rotor inertia changes acceleration while identical map loads stay exact.
- Contact oracle: m=2000, I_L diagonal=(3000,5000,7000), I_r=1000,
  vertical main-leg lever=(±1.5,-1,0), maximum spring=6*90000 N/m gives
  maximum squared frequency 985.5 using I_B, versus 810 using locked inertia.
  Runtime selects six substeps instead of five. An extreme but valid I_B also
  demonstrates typed initial budget rejection, not silently capped work.

## Transaction and input boundary checks

All 16 state scalar bit patterns are compared on failures at K2, K3, K4,
Endpoint and a later internal substep. The endpoint fixture's upper shaft bound
180.11406013802542 rad/s lies between K4 180.11406012828962 and final weighted
180.11406014776125, so checking K4 alone would accept the invalid endpoint.

Other checks cover zero duration, negative/nonfinite/over-limit duration,
zero/negative/nonfinite shaft speed, finite adjacent out-of-profile shaft speed
before J division, bad pitch/fraction/quaternion/position/raw vector norms,
reverse inflow, excessive crossflow/tip Mach and nonpositive absolute spin.
Diagnostics preserve missing values and signed zero and reject nonfinite input;
compact errors remain below 128 bytes without boxing.

Independent review found an important constructor boundary: algebraically
regrouping yy*(xx*zz-xz²) can round above epsilon while the actual glam matrix
determinant equals epsilon, triggering the existing assertion constructor.
The final guard builds the identical DMat3, checks its determinant AND finite
inverse before invoking MassProperties::new. A catch_unwind regression fixes
locked [1,5.763158480729852,6.247241073718394,2.5594038453272185e-8] and
I_r=.9999999999999999 as a typed error without panic.

## Independent governor convergence and stability

A separate scalar RK4 oracle uses the fixture's explicitly written torque/load
and continuous-governor equations at 1/7680 s, without production power,
propeller, governor, derivative or integration helpers. Its ISA troposphere
calculation independently includes the geometric-to-geopotential conversion.
The production runtime bench restrains translation to its operating point once
per sample, retaining engine states, orientation and physical roll dynamics.
Locked roll inertia is 30000 kg m² for this bench. This avoids interpreting a
freely falling zero-lift test fixture as an aircraft cruise test.

The sampled production governor is expected to approach the continuous oracle
with first-order hold error. Absolute shaft errors after 20 s are:

| Throttle | Altitude m / axial m/s | Torque cap Nm | 120 Hz | 240 Hz | 480 Hz |
|---|---|---:|---:|---:|---:|
| .4 | 0 / 0 | 5000 | .000277634 | .000138774 | .000069376 |
| .7 | 1000 / 40 | 5000 | .003106325 | .001552620 | .000776175 |
| 1 | 3000 / 70 | 1000 | .006355360 | .003176124 | .001587673 |
| 0 | 1000 / 40 | 5000 | .004324914 | .002160675 | .001079644 |

Units are rad/s; each sweep used one accepted internal substep. Largest absolute
120 Hz pitch error was .00002844860 rad. The combined shaft/pitch errors decrease
by about half with each halving of dt, including torque cap and idle response.
The 40 s load-response cases reduce maximum shaft error from 1.2358 to .0774 rad/s
for throttle .4, and 7.7955 to .4797 for throttle .7 between seconds 10–20 and 30–40.
A weak 100 Nm cap reaches the fine pitch stop with supported sub-reference RPM.
These establish this fixture's numerical behavior, not full-envelope stability.

## Work, allocation and timing

Stepping makes one initial evaluation and four RK stages plus one endpoint per
internal substep, with a tested maximum of eight substeps (41 evaluations).
There is no retry loop. Structural inspection of the stepping call graph finds
only borrowed immutable bounded tables, fixed arrays, scalar/stack states and
existing allocation-free atmosphere/coordinate/force helpers. No formatting,
Vec growth, allocation, I/O, wall clock, random source or global mutation occurs
inside step. This is a structural allocation audit, not a measured allocator
count; the repository's unsafe-code ban and dependencies were not changed.

The separate Criterion fixture excludes cloned configuration/construction from
the timed operation. Shared Linux x86_64 environment, AMD EPYC 9V74 virtualized
host, rustc 1.93.0, repository release settings, -j2, existing shared target.
Command: `cargo bench -j2 -p flightsim-fdm --bench turboprop_step -- --quick`.
A quick sample is an indicative local timing, not an app FPS or real-time bound:

| Case | Accepted substeps / evaluations | Quick interval |
|---|---:|---:|
| Airborne 1/120 s | 1 / 6 | 9.0449–9.2400 μs |
| Ground contact 1/120 s | 5 / 26 | 37.484–37.859 μs |
| Maximum duration 8/120 s | 8 / 41 | 63.307–63.846 μs |

## Gates

All gates below pass on the qualified source. The shared target's project crate
roots were touched first, and compilation logs identify this isolated worktree.

- `cargo test -j2 -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen -p flightsim-net -p flightsim-content`: 1,175 tests/doctests, zero failures or ignores in 87 suites; includes the 27 new turboprop cases and unchanged old numerical/identity/replay fixtures
- `cargo clippy -j2 -p flightsim-core -p flightsim-fdm --all-targets -- -D warnings`
- `RUSTDOCFLAGS='-D warnings' cargo doc -j2 -p flightsim-core -p flightsim-fdm --no-deps`
- `cargo fmt --all --check`, `git diff --check`, `bash scripts/check-architecture.sh`
- The final-tree scoped Criterion command above, without a Bevy build or a second target

The documentation gate initially caught three unescaped bracket sequences in
new rustdoc comments; those comments were corrected and the strict gate rerun.
Native/app/Windows, full authored flight scenarios, measured reference
transcription and profile/identity/replay remain separate.
