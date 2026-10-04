# Jet headless, complete identity and replay-v4 verification

Verified on 2026-10-04 using the repository's serialized Rust 1.93 build lane,
`-j2`, and the existing shared build environment. This is a pure simulator and
standalone command milestone. No app/renderer/native source, legacy force law,
profile-v1 parser, old replay encoding body, old golden or candidate pin was changed.

Prerequisites: bounded component foundation `b609a94`, reviewed runtime `0f1b2b0`,
exact profile-v2 loader `fadb821`, reviewed contact-horizon correction `88b00bd`,
and precommit gear-clearance diagnostic/report `b842330`. These remain separately
reviewed changes. The initial descent experiment exposed a real precontact
lookahead error: K4 anticipated contact beyond the external interval. The FDM
owner corrected that arithmetic before the private law-1 milestone was frozen;
this integration retained its unchanged 35 m/s, 2 m/s descent as an acceptance gate.

## Numerical and command evidence

The public numerical fixture's force tables were not tuned to meet an invented
takeoff time. The feedback takeoff driver and Newton trim solver are explicit
regression aids, separate from held-control natural response checks.

| Scenario | Observed result | v4 reproduction |
| --- | --- | --- |
| Feedback takeoff, 60 s | 7,200 committed steps; CG height 198.947893 m; airspeed 44.867163 m/s; positive final climb 5.477257 m/s | All checkpoints/final state exact |
| All wheels clear 0.5 m | First sampled at 21.458333 s, CG 1.566678 m, airspeed 41.118213 m/s | Contact/log clock reproduced |
| Held-control trim, 1,000 m / 50 m/s, 30 s | CG 1,000.014468 m; airspeed 49.998892 m/s; no feedback during flight | Exact |
| Held-control approach, 60 m / 35 m/s / 2 m/s descent | Actual wheel penetration first sampled at 29.800000 s; minimum clearance −0.002021 m; one contact event by 30 s | Exact |
| Idle then maximum dry, 6 s | Final airspeed 51.628098 m/s from initial trim 50 m/s; separate 3 s idle/max tests demonstrate deceleration/acceleration | Exact |
| Deliberate profile-envelope exit | Mach 0.9010000000000002 rejects at Initial/substep 0/cursor 0; committed duration zero; flight command exits 1 | Expected failure verified; replay command exits 0 |

The trim regression also solves five held-control points: (1,000 m, 35/50/65 m/s),
(3,000 m, 45 m/s), and (5,000 m, 50 m/s). Residual north/down acceleration and pitch
angular acceleration are below 1e-9 in their SI components before the 30 s free
response. Tests assert bounded subsequent altitude/speed drift. These authored
points do not establish a full operating envelope, certified handling or real
engine performance. The approach observes wheel geometry and CG diagnostics;
it does not classify a safe landing, structural damage or body/wing collision.

## Transaction, identity and codec coverage

- Every failure preserves state, prior interpolation endpoint, committed ground,
  all three signed per-leg clearances, elapsed/weather clock, contact history and
  log. Controller candidates commit only after successful physics; terminal/no-step
  calls do not advance pilot state. The report separates attempted and committed work
- Actual rotated gear geometry replaces the legacy CG-height approximation only
  in the new jet path. Inverted, pitched and bundled-sloped cases verify the wiring;
  the approach requires physical minimum clearance <=0, not proximity alone
- Schema-2 identity independently encodes 1,095 canonical bytes and fingerprint
  `9493dfe3f9ba6f76`. Tests independently perturb all 116 numeric physical fixture
  fields (using valid signed-zero perturbations where constrained), ordered gear,
  all scheduled coefficients/cells, and envelope values. Presentation remains excluded
- Ten Python `struct.pack` fixtures independently pin v4, including both terminal
  lengths, empty and all authored weather shapes. Rust roundtrip matches exact bytes;
  every truncated prefix, unsupported tags, reserved bits, malformed lengths/counts,
  invalid controls/states and inconsistent checkpoint ordering are rejected
- Complete/final checkpoints use all 104 state bytes. Single signed-zero flips in
  a checkpoint or terminal query cause reproduction faults. Each clearance sign
  bit independently affects the exact rollback snapshot projection
- Terminal zero is attempted without extra time. Cursor-240 terminal work defers
  correctly when the 240-attempt seek budget is exhausted. Pause/restart/seeks,
  wrong reason/query, unexpected success, model mismatch and ordinary drift are tested
- Unexpected terminal success is probed on a scratch FDM and faults without changing
  the committed cursor/state. Recorder overflow/continuity failure permanently
  freezes its prior authentic final state and cannot attach a later terminal event
- 30/60/144 Hz input partitions produce exact same committed state/time/bytes.
  Bundled terrain/climate, nonzero wind/turbulence and authored Rain metadata have
  separate exact cadence/replay coverage. Unknown nonzero dataset fingerprints
  remain structurally round-trippable and are blocked before reproduction
- Pole/dateline and high-altitude initial positions preserve exact initial bits;
  header position agrees within one micrometre ECEF. Heading remains diagnostic
- Old wrapper APIs still reject v4. The outer model wrapper explicitly retains
  Existing v1/v2/v3; all old byte and aircraft-identity oracles remain unchanged

## Completed checks

- `cargo test -j2 -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen`: 1,092 tests including doctests passed, zero failed
- `cargo clippy -j2 -p flightsim-sim --all-targets -- -D warnings`: passed
- `RUSTDOCFLAGS='-D warnings' cargo doc -j2 -p flightsim-sim --no-deps --document-private-items`: passed
- Changed-file rustfmt check, whitespace diff check and architecture dependency check: passed
- Independent old identity/v1-v3 codec/component and new schema-2/v4 Python encoders: passed
- Separate CLI process tests load the public profile, write trajectory/v4, verify
  ordinary replay, expose a zero-time rejection and replay its exact failure: passed

Independent review followed the wire draft, transaction ownership, resource bounds,
identity completeness, API isolation, source changes and contact correction. Existing
replay source edits only expose selected helpers within the crate; their bodies
were additionally compared byte-for-byte with the prerequisite commit.

No performance/FPS claim, renderer/weather presentation, native manual-flight test,
real-aircraft certification, app model selection, regional-package replay, release
package or publication acceptance is implied. The example deliberately has no GLB.
