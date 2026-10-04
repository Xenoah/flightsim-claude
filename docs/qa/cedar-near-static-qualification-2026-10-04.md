# Cedar law 2: unchanged calm braking and bounded aircraft qualification

Date: 2026-10-04 UTC. **The original calm braking trajectory now completes the
near-static interval and accelerates after brake release inside the fixed law-2
domain. Stationary parking still fails because braked idle creeps.**

This is an original authored numerical approximation, not real-aircraft or
measured propeller validation. It adds qualification helpers, tests and evidence
only. No production FDM, aircraft profile, gear, mass, controls, coefficient
positive rows, friction, identity, replay, app, public preset or domain limit is
changed by this qualification.

## Source and exact configuration

- Production law-2 source: `9f6a9449226bc6be5fee8d471afcbff2ad4c71f6`
- Reviewed test-only correction: `d18d953dcaa9775783b0a4adb16a540698f09e5a`,
  consumed locally as `b6f4ae3`; production law bytes are unchanged
- Original complete Cedar candidate: `be5094dd49a88d5a3709c7435dca7bfca287e7df`
- Original profile SHA-256:
  `a4b13ac7c59830967666bb874e25416587a5fbf9945cbad248afa5bc7c9344e1`
- Original support helper SHA-256:
  `7444518e1f93c7567a10b0b9714b4f8cd571457cadf7fac1df120b3a87689609`

The original profile is copied byte for byte into a **test fixture**, then read
through the production `AircraftProfileV3::parse` exact-token loader. Its old
schema-3/law-1 fingerprint remains `f83d082e814871c0`. The entire canonical
forward-configuration byte stream is compared directly before and after
`near_static::TurbopropAircraftConfig::from_forward`; it must be identical.
The copied law-1 helper differs only in its test-fixture include path and that
line's wrapping. Its original trim, start, full-state, controls and disk geometry
helpers are retained. None is installed as production aircraft support.

The [explicit negative-row fixture](../../crates/flightsim-sim/tests/fixtures/cedar-law2-negative-rows.json)
has the predeclared J=-0.01 and -0.005 rows, using the original pitch axis and
recipe `CT=0.5*(beta-atan(J/(0.75*pi)))`,
`CP=1.22*CT/2*(J+sqrt(J*J+8*CT/pi))+0.005+0.01*beta`.
The [authoring check](../../tools/qualification/build_cedar_near_static_rows.py)
regenerates those exact stored rows and verifies the original profile hash.
The Rust harness parses their decimal tokens with Rust's correctly rounded
parser before construction. There is no runtime extrapolation or row generation
inside the FDM. For negative J, this is the authored continuation reference,
not an empirical or universal reverse-flow power law.

The immutable limits remain J>=-0.01, adverse induced-velocity ratio<=0.10 and
transverse induced-velocity ratio<=0.10, intersected with every old envelope
limit. No profile-4, identity-4 or replay-6 allocation occurs.

## Unchanged critical trajectory

Start: the original 1.6 m CG-height, 25 m/s calm-air running state, turbine
fraction 0, shaft 180 rad/s, blade pitch 0.08 rad; original airframe and gear.
All six effective controls are neutral except maximum brakes for the first
20 seconds. At 20 seconds only brakes are released; idle throttle and surfaces
remain unchanged for another 10 seconds. No state reset, velocity correction,
parking force, stabilization or hidden control is applied.

| External rate | Completed | Braked time below 0.1 m/s | Peak speed during 10–20 s | Distance during 10–20 s | Speed at 30 s |
|---|---:|---:|---:|---:|---:|
| 120 Hz | 30 s | 15.8500 s | 0.02018895 m/s | 0.20061697 m | 2.69879509 m/s |
| 240 Hz | 30 s | 15.8458 s | 0.02018909 m/s | 0.20061744 m | 2.69879509 m/s |
| 480 Hz | 30 s | 15.8458 s | 0.02018932 m/s | 0.20061877 m | 2.69880426 m/s |
| 960 Hz | 30 s | 15.8469 s | 0.02018935 m/s | 0.20061888 m | 2.69880426 m/s |

At the finest rate, release begins at 0.01996512 m/s; speed reaches
0.40341075 m/s after one second and 2.69880426 m/s after ten seconds.
The transient negative axial-flow interval spans approximately 4.129–4.585 s,
with minimum axial velocity -0.10065936 m/s. Later near-static motion is forward
creep. The continuous ten-second speed<0.001 m/s parking criterion is explicitly
**false at every rate**. Passing the domain/duration/release qualification does
not supply a stationary full stop or a completed ordinary parking workflow.

All encountered negative force-stage queries, including Initial, K1–K4 and
weighted Endpoint, are retained in the full evidence. Across all rates:

- Minimum signed J: -0.0014900941973
- Maximum adverse ratio: 0.0099377172, under the fixed 0.10 limit
- Maximum transverse ratio: 0.0040725829, under the fixed 0.10 limit
- Minimum full propeller-disk clearance: 0.2411704771 m
- Minimum original static-blade swept-envelope clearance: 0.2394557454 m
- Maximum sampled strut compression: 0.1112606237 m, below 0.18 m main stroke
- 8,496 negative-stage records, from 558,000 total observed stage evaluations

The exact original law-1 fixture still rejects K2/internal substep 2 at
J=-4.8781986805208105e-6 in the original x86_64 Linux GNU evidence, preserving
all 16 state words. Cross-platform tests retain exact input identity and use the
[independent same-runtime stage witness](near-static-turboprop-law2-2026-10-04.md#cross-platform-derived-output-witness-correction)
for derived signed J and diagnostics; the Linux output pin remains scoped to
that platform. Law 2 accepts that
identical physical state, controls and environment in six substeps. This witness
is preserved alongside, rather than substituted for, the full trajectories.

## Stage observation and force histories

The production model performs and accepts each full step. A qualification-only
observer then reconstructs its Initial/K1/K2/K3/K4/Endpoint query states using
the public derivative, state-offset, analytic-turbine and sampled-governor APIs,
with the actual reported subdivision count. Every reconstructed final endpoint
must match all 16 production state words exactly. The observer never writes
back to the simulator and does not replace production guards. This establishes
stage coverage of the accepted driver, not an independent mechanics theorem.

Signed J, signed axial velocity, current thrust, absorbed power, signed thrust
work, shaft RPM, pitch and both physical ratios are computed at every negative
stage. Shared 20 Hz histories also retain complete state bits, ECEF motion,
attitude, effective controls, net nongravity/aero/contact force vectors, all three
gear clearances, disk clearance and the original static-blade bound. Contact
force is obtained as the residual of the accepted acceleration minus gravity,
aero and propeller force; no contact model is copied or altered. Minimum disk
clearance and gear compression are observed at every committed step. These are
sampled dynamic geometry checks, not a continuous arbitrary-terrain collision
proof or an animated-pitch blade mesh validation.

For each step with no negative stage, a fresh law-1 model receives the identical
pre-step state/control/environment. Its report and endpoint must be bit-identical
to law 2. The critical trajectories contain 53,174 such positive-flow witnesses.

## Resolution comparison

The fixed source shared-history tolerances are 0.01 m/s speed, 0.001 rad attitude
pitch, 0.10 rad/s shaft, 0.001 rad blade pitch, 0.01 m altitude, 0.002 m clearance,
and 100 N per contact-force component. Every comparison passes. The source
checkpoint establishes these tested thresholds, not when they were chosen.

The contact-aware driver selects six/three/two/one substeps at external
120/240/480/960 Hz respectively. Thus 120 and 240 Hz use identical 1/720 s
internal steps, while 480 and 960 Hz use identical 1/960 s internal steps.
The corresponding paired 20 Hz histories are bit-identical. This is two distinct
internal resolutions observed through four requested external rates; it must not
be represented as four independent grids or a measured RK4 convergence order.

The maximum change between the two internal resolutions over the full shared
30-second history is:

| Quantity | Maximum difference |
|---|---:|
| Ground-speed magnitude | 0.0000855916 m/s |
| Body-air-velocity component | 0.0000855653 m/s |
| ECEF position component | 0.0000937730 m |
| Attitude pitch | 0.0000008343 rad |
| Relative shaft speed | 0.001229126 rad/s (0.0117373 RPM) |
| Blade pitch | 0.0000199234 rad |
| Signed J | 0.0000017078 |
| Propeller thrust | 0.313780 N |
| Contact force component | 0.929751 N |
| Gear clearance | 0.0000013066 m |
| Full disk clearance | 0.0000025406 m |

Per-step minima vary slightly because the endpoint sampling density differs;
the complete common-time comparison and each run's extrema are both retained.
The narrow error envelope, identical finest paired histories, sustained
near-static behavior and unchanged release response support this bounded
qualification without claiming an asymptotic error order.

## Original flight/contact matrix

Only after all four critical runs completed, the original 21 cases were rerun:
five 60-second trims/climb cases; their five 10-second roll/rudder-release cases;
calm/head-crosswind/sloped braked idle; scripted takeoff; braked ground power
cycle; headwind braking; three 1/2/3 m/s touchdown cases; flight throttle pulse;
and the original scripted poststall recovery. All initial conditions and
time-scripted controls match the original helper.

All 76,080 steps remain positive-flow and match law-1 reports and all endpoint
words bit for bit. No case rejects and no new negative-stage query occurs.
The regression pins each named case's complete expected step count at 120 Hz,
the 76,080-step total, and zero negative-stage records in every case.
The original minimum static-blade clearances reproduce, including 0.33555 m
calm idle, 0.32532 m sloped idle, 0.27812 m ground power cycle, 0.25161 m
headwind braking and 0.25748/0.29042/0.28589 m touchdown cases. All strut
compressions stay below 0.18 m. Takeoff and recovery retain their original
successful endpoints. This adds no full-envelope or pilot-handling claim.

## Artifacts and local checks

- [Readable summary and pairwise comparisons](cedar-near-static-summary.json)
- Complete critical histories and negative-stage records: external QA archive,
  `cedar-near-static-critical.json.gz`, 2,272,520 bytes
- Complete original matrix histories: external QA archive,
  `cedar-near-static-matrix.json.gz`, 7,727,640 bytes
- [External compressed/decompressed sizes and SHA-256 receipts](cedar-near-static-evidence-receipts.json)
- [Executable qualification harness](../../crates/flightsim-sim/examples/cedar_near_static_qualification.rs)
- [Qualification tests](../../crates/flightsim-sim/tests/cedar_near_static_qualification.rs)

Full raw and compressed histories are intentionally external to the source
checkpoint. The two compressed archives total 10,000,160 bytes and preserve the
raw evidence bytes exactly; compression uses mtime=0. The source retains the
reproducible harness, compact readable summaries and hash inventory. Original
raw files and command logs remain in the shared QA workspace. No evidence was
deleted. The harness can regenerate the histories locally; an archive is not
silently treated as an available repository file.

Checks used the provided 1.93.0 toolchain, existing shared Cargo target, offline
mode, two jobs, disabled debug information and warnings denied:

| Check | Result |
|---|---|
| Critical example, all four requested rates | Passed; 30 s each |
| Same example with `--matrix` after its critical gate | Passed; 21 original cases |
| `cargo test --offline -j2 -p flightsim-sim --test cedar_near_static_qualification` | 3 passed; zero failures/ignored |
| Scoped example/test Clippy with `-D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `bash scripts/check-architecture.sh` | Passed |
| Negative-row rebuild/profile hash check | Passed |
| `git diff --check` | Passed |

Review follow-up: after strengthening the matrix regression to pin every case's
complete parity count and absence of negative stages, the focused
`original_flight_and_contact_matrix_preserves_positive_flow_bits` test passed
(one passed, two filtered out), and scoped test Clippy passed with warnings
denied. The accepted raw/compressed histories, summaries and hash receipts are
unchanged; this follow-up did not regenerate the evidence archives.

Reproduce from the repository root:

```sh
python tools/qualification/build_cedar_near_static_rows.py --check
cargo run --offline -j2 -p flightsim-sim --example cedar_near_static_qualification -- docs/qa/cedar-near-static-critical.json --matrix
python tools/qualification/analyze_cedar_near_static.py
cargo test --offline -j2 -p flightsim-sim --test cedar_near_static_qualification
```

The analyzer reads locally regenerated raw evidence or external compressed
evidence selected with `--evidence-dir /path/to/archive`. The prior
independent pure-law review's aggregate tests are separate evidence; this
qualification does not relabel them as newly executed tests. No GPU, native
application, Windows, production host/replay, publication or empirical aircraft
acceptance was performed here. Stationary parking, production versioning and
application adoption remain separate gates.
