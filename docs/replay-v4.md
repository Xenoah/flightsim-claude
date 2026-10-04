# Explicit jet replay v4

This additive format is accepted only by `ModelReplayFile` / `JetReplayPlayer`.
`ReplayFile`, `CurrentRecording`, and `CurrentRecorder` remain v1/v2/v3 APIs.
No aircraft name selects a physics model. Schema-2 hashes detect changes; they
are not authentication. Regional packages and arbitrary caller terrain sources
are unsupported.

All integers and IEEE-754 f64 bit patterns are little endian. No parser repairs
values, normalizes quaternion bits, clamps commands, or relabels old recordings.
The header is `FSREPLAY` (8 bytes), version u16=4, conditions length C u32.
C is capped at 4096 and must consume exactly 255+N+W bytes:

1. Name length N u32 (0..256), then N UTF-8 bytes (informational only)
2. Identity: algorithm u16=1, schema u16=2, model kind u16=2, FDM law u32=1,
   fingerprint u64
3. Simulation law u32=1
4. Exact 112-byte v3 environment block (start lat/lon/alt/heading, wind direction/
   speed, turbulence intensity f64; seed u64; epoch/rate f64; world flags u64;
   global terrain fingerprint u64; climate annual phase f64; climate fingerprint u64)
5. Terrain kind u8: 0 is globally flat ellipsoid-relative ground, 1 is the bundled
   global terrain. Flat elevation f64 follows; for global it must be positive zero.
   The kind must agree with the environment's world flag. Global/climate payload
   fingerprints must match the supported bundled datasets before reproduction.
6. Weather length W u32 followed by the unchanged complete v3 weather block:
   W=0 denotes explicit Legacy; W=62/86/96/120 contains validated authored parameters.
   The v3 weather resource bound (512) and parameter meanings remain authoritative.
7. Initial state: ECEF position xyz, ECEF velocity xyz, quaternion xyzw, body angular
   velocity xyz, all f64 (104 bytes). Start metadata must denote this position to
   1 micrometre ECEF; heading is diagnostic metadata, with no Euler equality gate.
   Metadata is never repaired.

After the bounded conditions:

- Successful fixed-step count N u32, at most 1,000,000; each input stores six f64
  values in aileron/elevator/rudder/throttle/flaps/brakes order (48 bytes). Every
  successful input represents exactly 1/120 second. Failure contributes zero duration.
- Checkpoint count K u32, at most 8,334. Each checkpoint is cursor u32 + state (104 bytes).
  Cursors are exactly 120,240,... below N, then mandatory final N. For N=0 the
  single mandatory checkpoint is cursor 0. These are drift evidence, never complete
  simulation snapshots; rewind always reexecutes from the initial state.
- Terminal block length L u32. L=0 means ordinary end. L=61/85 contains exactly one
  failed attempted input at cursor N: cursor u32, controls (48 bytes), reason u8, detail u16,
  stage u8, zero-based substep u32, query-present u8, optional three f64 values
  pressure ratio/temperature ratio/Mach. Flag 0 requires L=61; flag1 requires L=85.
  All queries must be finite, pressure/Mach nonnegative and temperature positive.

Terminal reason tags: 1 invalid input (detail is the closed JetInvalidInput code),
2 outside atmosphere altitude (single axis), 3 outside operating envelope (three
axes), 4 outside jet table (three axes), 5 outside Mach schedule (single axis),
6 substep budget (detail 0). An axis is Below=0/Within=1/Above=2. Three axes occupy bits
0..1 pressure, 2..3 temperature, 4..5 Mach; remaining bits must be zero. Outside
reasons must contain at least one outside axis. Stages Initial=0/K1=1/K2=2/K3=3/
K4=4/Endpoint5. Substep is 0..7; Initial requires substep 0. Unknown tags, reserved
bits, unsupported revisions and inconsistent block lengths are errors.

Playback reexecutes at most 240 fixed steps per public advance/seek call. It checks
every stored checkpoint against actual full state bits. At the terminal cursor it
reattempts that control input and requires exact reason, stage, substep and query
bits and an unchanged committed simulation snapshot. Unexpected success or a
changed rejection fails playback. Pause creates no work; frame zero, restart and
seeking reset clocks, contact history, log and fractional budget.

## Model identity schema 2

FNV-1a64 starts at 0xcbf29ce484222325, xor each byte then wrapping multiply by
0x100000001b3. The canonical stream is ASCII `flightsim/model-identity\0`,
algorithm u16=1, schema u16=2, kind u16=2, FDM law u32=1, followed by:

- mass f64 and nine column-major inertia-matrix f64 values
- wing area/span/mean chord f64
- rolling/braking/lateral friction and transition speed f64
- gear count u32=3; every leg in stored order: contact xyz, spring, damping,
  maximum stroke, bottom-stop travel, maximum recoil speed (eight f64)
- envelope pressure min/max, temperature min/max, Mach min/max (six f64)
- canonical dry-jet component byte length u32 and complete component bytes
- canonical Mach-aero component byte length u32 and complete component bytes

Signed zero is retained, including derived inertia signs. Names and visual/input/
sound metadata are excluded. The immutable FDM constructors own validation.
The FDM law pins all integration/rejection arithmetic; the simulation law pins
120 Hz committed time, prospective environment sampling, terrain/contact/log
semantics and frame-budget cancellation. No propeller crash thresholds or
propeller-specific crash claims are implicitly applied to jets.

The API accepts already-sanitized effective `ControlInputs` only. Six raw wire
scalars are strictly validated before constructing that type, including terminal
controls. Invalid-input tags are TimeStep=1, State=2, Quaternion=3, Position=4,
AtmosphereOffset=5, Ground=6, Wind=7, RelativeVelocity=8, AtmosphereTemperature=9,
AtmosphereDensity=10, ComponentQuery=11, Derivative=12, IntermediateState=13, AngularRate=14.

Initialization derives airborne from the minimum actual rotated per-leg signed
normal clearance >0.5 m. Contact is observed at <=0 m and re-arms above 0.5 m.
These clearances use the FDM's exact sloped-plane geometry and are computed
before a successful FDM commit; they are retained in the committed snapshot. A fixed default GroundSampler uses
10 m probes and its existing bounded slope law for bundled terrain; flat ground
uses the explicitly recorded elevation. Ground is sampled at the pre-step CG
and held through integration/contact/log. There is no configurable crash limit.

A failed transaction preserves the state, prior interpolation endpoint, committed
ground, elapsed/weather clock, flight log and contact history. Only its terminal
latch changes and unconsumed fractional/frame budget is discarded. Controller
cloning must isolate all mutable pilot state; shallow shared-mutability clones
cannot provide this guarantee.

The player verifies and reattempts a terminal-at-zero during construction/restart.
At final N it never completes before verifying the terminal; the terminal attempt
uses one unit of the 240-attempt work budget but zero simulated duration. A next
unpaused call with zero time settles a terminal deferred by the work cap. Pause
performs no automatic work; explicit seek continues its bounded reconstruction
even while paused, including a terminal when seeking N. Seek 0 resets everything.

The recorder requires a pristine nonterminal simulation at cursor/time zero. Its
final state is retained internally. A count/continuity error closes it permanently
before appending that report; later states or terminal events cannot be attached
to an earlier truncated recording. Finish returns its authentic prior final state.

The state validator uses the jet law's finite vector-norm/geometric-altitude
checks and quaternion norm tolerance 1e-9, preserving every bit. The legacy codec
retains its distinct 1e-12 tolerance. Both 0 and negative 0 are valid ordinary
control/state/query scalars where their numeric domain permits zero; their sign
bits still participate in identity and exact reproduction checks.

Structurally valid unknown nonzero bundled terrain/climate fingerprints can be
read and exported byte-for-byte for inspection. Simulation construction and
playback require matching installed bundled fingerprints. Unknown model or
simulation laws never pass structural parsing.

Physical elapsed time and reported recording duration both use repeated
1/120-second additions, never cursor multiplication. The maximum encoded v4
size is 48,900,814 bytes: 14 header + 631 conditions + 4 count + 48,000,000 controls +
4 count + 900,072 checkpoints + 4 terminal-length + 85 terminal. The cap is a serialized size
bound, not a promise about resident memory or wall-clock cost. Ten independently
encoded fixtures cover ordinary zero-frame, both terminal lengths and all authored
weather shapes.

For jet contact diagnostics, `Touchdown.position` is the CG geodetic position
at the first accepted endpoint with actual wheel contact. `sink_rate` is the
pre-step CG vertical descent rate. Neither is the exact wheel contact coordinate
or the wheel's normal closing speed. Inverted/pitched states use rotated gear
geometry; body/wing collisions, structural damage and qualified landing scores
remain outside this headless model path.
