# Aircraft replay identity and the v3 envelope

Status: complete aircraft identity and the bounded v3 codec are implemented in
`flightsim-sim`. New app recordings use v3 complete aircraft identity with the
exact resolved initial weather selection (Legacy remains the default). `ReplayFile` and the additive `ReplayFilePlayer` preserve
v1/v2/v3 on import, playback and export. Historical `Recording`/`Recorder`/`Player`
APIs retain their previous signatures and semantics. The app now requires the
explicit legacy compatibility policy below. Modeled-weather playback is enabled
through the full renderer/executed-clock bridge; profile-v2 remains a separate gate.

## Why a new identity contract is necessary

The frozen `replay::aircraft_fingerprint` hashes 68 f64 values followed by
`flightsim-fdm-model` and the little-endian `FDM_MODEL_REVISION`. It omits
`aero.yaw_rate_p`, which the yaw-moment equation uses. With model revision 2,
Light Single hashes to `0505e6644bb29a53` and Swift Sport to
`06068a31d11a75e0`; changing only that coefficient leaves either hash unchanged.

A v1/v2 recording contains no aircraft profile. Its name is informational, and
neither its hash nor a passing 50 m positional drift check can recover the missing
coefficient. Matching a bundled name does not prove that the original aircraft
was bundled: a custom aircraft could retain both the name and this partial hash.
An identity match is not file authentication or a cross-platform trajectory
guarantee. FNV-1a is only a non-cryptographic change detector.

The old algorithm, `Conditions` fields, `with_aircraft`, durations, stored
fingerprints and physics are unchanged. Legacy reader/writer semantics and bytes
are retained; shared record/numeric helpers replace duplicated codec logic.
Historical `Recording::write_to` still selects v1 when world/climate are disabled.
`ReplayFile::write_to` instead retains the explicit source version, including v2
with an all-zero world block. Neither path upgrades legacy aircraft evidence.

## Implemented API

`flightsim_sim::replay::identity` provides:

- `AircraftIdentity::for_config(&AircraftConfig)`: complete aircraft parameter
  identity for the current FDM law. The caller validates external profiles first
- `AircraftIdentity { algorithm: u16, schema: u16, fdm_model_revision: u32,
  fingerprint: u64 }`: explicit descriptor; equality includes every field
- `RecordedAircraftIdentity::Complete(AircraftIdentity)` and
  `RecordedAircraftIdentity::LegacyPartial { fingerprint: u64 }`: distinguish
  actual recorded evidence. A v1/v2 u64 is always the latter
- `RecordedAircraftIdentity::verify(&AircraftConfig) -> AircraftCompatibility`:
  `CompleteMatch`, `LegacyPartialMatch`, or `Mismatch`
- `Conditions::aircraft_identity()`: exposes existing v1/v2 evidence as partial
- `Recording::check_compatibility_with(&AircraftConfig) ->
  Result<AircraftCompatibility, ReplayError>`: retains numeric/keyframe and
  bundled terrain/climate checks, then classifies aircraft identity. An aircraft
  mismatch is `Ok(Mismatch)`; corrupt data or world/climate mismatch is an error

`CompleteMatch` means the supported aircraft identity matches. It says nothing
about external terrain, platform arithmetic or an unvalidated aircraft config.
`LegacyPartialMatch` means only that the frozen algorithm matches the selected
configuration and current model revision. It does not identify a trusted builtin.
An unknown algorithm/schema, changed model revision, changed complete hash, or
different legacy hash is `Mismatch`; no fallback from a complete mismatch to a
legacy hash is allowed. Pre-model-revision alpha.21 hashes remain mismatches.

The historical `check_reproducible_with -> Result<(), ReplayError>` remains a
source-compatible gate with unchanged acceptance behavior. Its `Ok(())` means
only legacy partial compatibility. Application startup uses the classified API
and the explicit policy below; that older API is retained for existing callers.

## Current recording and explicit dispatch API

- `EnvironmentConditions` contains only the existing environmental/world fields.
  Its defaults preserve legacy behavior; `with_world_climate` selects bundled
  dataset identities for a new flight. Copying these values from legacy conditions
  does not create aircraft identity or upgrade a recording
- `CurrentConditions::for_aircraft(config, environment)` captures the complete
  identity and informational name for a **new** flight, with `WeatherSelection::Legacy`.
  Its fields are name, `AircraftIdentity`, `EnvironmentConditions`, and validated
  `WeatherSelection`. There is no default/fabricated aircraft identity, legacy u64
  slot, or legacy-recording conversion
- `CurrentRecorder` records the same effective controls/pre-step keyframes with
  the existing cap and interval. `CurrentRecording::write_to` writes only v3;
  `CurrentRecording::read_from` accepts only v3
- `ReplayFile::{V1(Recording), V2(Recording), V3(CurrentRecording)}` reads explicit
  versions 1/2/3 and exports its retained variant. Shared accessors expose name,
  classified identity, environment, typed weather, frames, keyframes, duration and
  drift. `check_compatibility_with` validates values/world data before classifying
  aircraft evidence. Parsing a supported weather block is not permission to play
  it without the corresponding presentation integration
- `ReplayFilePlayer` consumes an explicit `ReplayFile` and exposes the original
  source through `recording()`. It shares a private timing cursor with historical
  `Player`, preserving pause, speed, overflow, zero-duration, seek and end rules.
  A player alone does not grant aircraft or presentation compatibility
- `Recording::read_from` remains v1/v2 only. `write_v1_to` and `write_v2_to` are
  explicit legacy encoders; v1 export rejects enabled world/climate before writing.
  Historical `write_to` keeps its original automatic v1/v2 selection. A legacy
  object is never assigned a newly computed complete identity on export
- Version constants are explicitly named `LEGACY_FORMAT_VERSION = 1`,
  `WORLD_FORMAT_VERSION = 2`, `CURRENT_FORMAT_VERSION = 3`. `FORMAT_VERSION` is
  retained as a source-compatible alias for the legacy API's maximum, 2

Current read/write boundaries reject unsupported identity algorithm/schema/model
revision before accepting data. A supported but different fingerprint remains
`Ok(Mismatch)` from the classifier. The pure descriptor classifier can still
classify unsupported descriptors as `Mismatch` without parsing a file. All
recording writers validate their values before the first write; ordinary I/O
failure can still leave a partial stream. Weather scenarios cannot be constructed
from unchecked DTOs: the private validated representation is required.

Use `ReplayFile` for format-preserving import/export. Removing its format tag and
calling the historical writer intentionally opts back into that writer's old
v1/v2 selection semantics. Do not use `CurrentRecorder` to rewrite old frames as
if the original flight had recorded complete identity.

## Canonical complete identity, algorithm 1 / schema 1

All integers are unsigned little endian. Scalars are IEEE-754 binary64 SI values
encoded by `to_bits().to_le_bytes()`, without normalization, rounding or sorting.
Signed zero is preserved. Field order below is normative. The name, visual model,
camera, audio and input mapping/ramp settings are excluded: replay records the
effective six controls already supplied to physics.

FNV-1a starts at `0xcbf29ce484222325`. For each byte: XOR the byte into the hash,
then multiply by `0x100000001b3` modulo 2^64. Hash exactly these 594 bytes:

| Offset | Bytes | Content |
|---:|---:|---|
| 0 | 28 | ASCII `flightsim/aircraft-identity` followed by one NUL |
| 28 | 2 | Algorithm ID = 1 |
| 30 | 2 | Parameter schema = 1 |
| 32 | 4 | `FDM_MODEL_REVISION` |
| 36 | 80 | Mass, then all nine inertia matrix entries in column-major order |
| 116 | 24 | Wing area, span, mean chord |
| 140 | 200 | The 25 aerodynamic scalars below |
| 340 | 2 | Propulsion kind = 1: current static-cap/constant-efficiency propeller |
| 342 | 24 | Maximum shaft power, propeller efficiency, static thrust |
| 366 | 32 | Rolling, braking, lateral friction; transition speed |
| 398 | 4 | Fixed landing-gear leg count = 3 |
| 402 | 192 | Three legs in stored order, eight scalars each |

The aerodynamic order is:

```
lift_zero, lift_alpha, lift_flaps, stall_angle, stall_blend_rate,
drag_min, oswald_efficiency, drag_flaps, side_beta, side_rudder,
roll_beta, roll_rate_p, roll_rate_r, roll_aileron, roll_rudder,
pitch_zero, pitch_alpha, pitch_rate_q, pitch_elevator, pitch_flaps,
yaw_beta, yaw_rate_p, yaw_rate_r, yaw_aileron, yaw_rudder
```

Each leg is `contact.x, contact.y, contact.z, spring_rate,
damping_coefficient, max_stroke, bottom_stop_travel, max_recoil_speed`.
Gear order stays explicit because changing force-summation order may change
floating-point results. Inverse inertia is a deterministic private-constructor
derivative of the recorded inertia matrix, not an independent input. There are
69 encoded f64s and 64 independently configurable physical scalar parameters.

The implementation exhaustively destructures `AircraftConfig`, `Geometry`,
`AeroCoefficients` and `EngineConfig`: a new public field requires a deliberate
identity decision at compile time. Private mass/gear representation changes still
require owner review of this coverage and the external definition. A change to
field order or coverage requires a new identity schema; a changed force law
requires a changed model revision. Do not mutate schema 1 in place.

Independent Python `struct.pack` encoding of the shipped profile dynamics gives
the following. Reproduce it with `python3 docs/qa/replay_identity_reference.py`:

| Revision-2 fixture | Frozen legacy | Complete algorithm 1 / schema 1 |
|---|---|---|
| Light Single | `0505e6644bb29a53` | `b7fa864dc47824f7` |
| Swift Sport | `06068a31d11a75e0` | `172167174cf90012` |

The tests pin these values, mutate every independent physical scalar, test gear
order and model revision, and independently encode v1/v2 bytes for legacy
classification and unchanged writeback. They deliberately prove that a changed
`yaw_rate_p` remains *partial* under legacy evidence and mismatches complete
evidence. None of these tests claims to authenticate the data.

## Format v3: an explicit bounded conditions envelope

V3 carries identity algorithm, parameter schema and FDM revision, plus an
optional separately versioned and validated weather block. It does not reuse the
old untagged u64 slot or synthesize a second legacy fingerprint. The dispatch
wrapper explicitly matches versions 1, 2 and 3. The legacy reader checks v2's
world extension against `WORLD_FORMAT_VERSION`, never the newest format number.

Whole-file layout (no implicit alignment/padding):

| Offset | Bytes | Content |
|---:|---:|---|
| 0 | 8 | Existing `FSREPLAY` magic |
| 8 | 2 | File format = 3 |
| 10 | 4 | Conditions byte length C, at most 4096 |
| 14 | C | Conditions payload below |
| 14+C | 4 | Frame count F, existing maximum 1,000,000 |
| 18+C | 4 | Keyframe count K, existing bound `F / 120 + 1` |
| 22+C | 56F | Existing seven-f64 frame records, unchanged |
| 22+C+56F | 108K | Existing u32 frame index + thirteen-f64 keyframes |

The conditions payload is:

| Relative offset | Bytes | Content |
|---:|---:|---|
| 0 | 4 | Aircraft display-name byte length N, at most 256 |
| 4 | N | UTF-8 display name |
| 4+N | 16 | Identity: algorithm u16, schema u16, FDM revision u32, fingerprint u64 |
| 20+N | 32 | Existing start latitude/longitude/altitude and heading f64s |
| 52+N | 16 | Existing wind-from and wind-speed f64s |
| 68+N | 16 | Existing turbulence-intensity f64 and seed u64 |
| 84+N | 16 | Existing visual epoch and time-rate f64s |
| 100+N | 32 | Existing v2 flags, terrain fingerprint, climate phase, climate fingerprint |
| 132+N | 4 | Optional weather-block byte length W, at most 512 |
| 136+N | W | Optional weather block |

Require `C == 136 + N + W`, checked before frame/keyframe allocation and before
any simulation mutation. The name is independently bounded to 256 bytes before
its small allocation; C and W are parsed with bounded subreaders, not buffers. Parse conditions through a reader bounded to exactly C bytes: no
conditions field or weather payload may consume the following frame counts.
Require exact consumption of that bounded payload before reading frame counts;
reject truncation, underconsumption or any field that exceeds the C-byte boundary.
Always include the world/climate block in v3, even when its fields are zero. Its
existing flag, absent-field and finite/range rules remain authoritative.
The complete identity descriptor has no legacy fallback and is not accompanied by
a fabricated legacy fingerprint.

`W == 0` means the exact existing wind/turbulence and ISA-or-climate behavior,
including the legacy presentation path. It must not silently enable a new weather
scenario or derive weather from current UI defaults. A present block starts with:

| Block offset | Bytes | Content |
|---:|---:|---|
| 0 | 2 | Weather parameter schema |
| 2 | 2 | Weather source kind |
| 4 | 4 | Weather model-law revision |
| 8 | W-8 | Exact schema-defined resolved scenario payload |

Reject `0 < W < 8`: a present weather block must contain its entire eight-byte
header. The header and schema-defined payload must both fit within the bounded
conditions reader and consume exactly W bytes.

Schema-1 weather uses exactly the [modeled-weather contract](modeled-weather.md):
62 bytes (no layers), 96 (cloud), 86 (fog), or 120 (both). The parser rejects
unknown schema/source/model/preset/precipitation/morphology, reserved flags,
inconsistent lengths, nonfinite/out-of-range scalars, cross-field conflicts and
mislabeled preset values. It validates length against layer flags before reading
layers and constructs `WeatherScenario` from the decoded resolved values; it never
regenerates a preset or accepts an opaque payload. Physical wind/turbulence remain
in their canonical fields above. Graphics/effects quality is excluded.

The envelope retains conservative caps of C <= 4096 and W <= 512. Accepted schema
1 actually requires at most C = 136 + 256 + 120 = 512 bytes. Frame and keyframe
limits remain 1,000,000 and F/120 + 1; the maximum accepted v3 file is therefore
56,900,606 bytes. No new file-sized buffer or unbounded weather allocation exists.
As with the historical streaming reader, bytes following a complete recording
are left for the outer stream; trailing bytes **inside C or W** are rejected.

New explicit-weather time must derive from executed simulation time. No particle
history, physical spool state or new control axes are introduced by this design.
If later models require those, update the recording/state contract deliberately.

## Application policy and remaining gates

New live app flights, restart and world-map Start use `CurrentRecorder` with the
complete selected aircraft identity and the validated initial weather selection.
`WeatherSelection::Legacy` (W=0) remains the default; an authored selection retains
its exact resolved parameter block. F9 writes v3 even when global terrain and
climate are disabled. No existing file is rewritten or upgraded. App playback
retains its original `ReplayFile`; there is no legacy-to-current conversion. F9 remains disabled during playback.

Normal `--replay FILE` requires complete identity. V1/v2 require the separate
`--legacy-replay-compatibility` option and both of these checks:

- The original partial fingerprint is one of the two frozen revision-2 Light
  Single / Swift Sport values above
- The selected **complete** configuration matches that fingerprint's frozen
  complete revision-2 baseline; an aircraft name never establishes eligibility

Successful opt-in still cannot establish the original `yaw_rate_p`. A persistent
`LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified`
notice remains visible while running, paused, seeking, complete or faulted.
The startup diagnostic explains the same limitation. A historical recording made
with a different omitted yaw coefficient remains inherently ambiguous even when
an operator explicitly assumes the supported baseline. Custom selected legacy
dynamics, old physics and complete-identity mismatches remain rejected; the flag
cannot bypass those checks. Exact playback is a same-build/configuration/terrain
claim, not authentication or a cross-platform floating-point guarantee.

Supported modeled-weather v3 files restore their exact validated weather block,
including Custom scenarios, without regenerating named presets. Ambient/fog
extinction remains independent of cloud quality; elapsed weather time is actual
executed simulation time after each step/seek/restart. `--weather` and
`--weather-seed` with replay are rejected before any startup condition copy.
Legacy semantics do not mean an explicit Clear scenario. See the
[modeled-weather contract](modeled-weather.md) and
[app verification](qa/weather-app-controls-2026-10-04.md).

The four manual weather overrides (`--cloud-cover`, `--cloud-base`, `--cloud-top`,
`--cloud-visibility`) are also rejected with replay, before conditions change.
For a live flight they still control presentation, but recording and F9 saving
are disabled with a persistent explanation. Restart the application without
those arguments to record. Cloud **quality** remains a render setting and does
not disable recording. Mapping those manual values into validated Custom weather
is still an explicit future decision; dropping them from a purportedly complete
recording is not permitted.

Regional-package replay remains blocked for every file version: v3 contains no
regional package ID/version/exact-manifest-hash block. Profile v2 must add an
identity schema covering each propulsion/aerodynamic variant, tag, table length,
coordinate and value without changing schema-1 ordering or propeller behavior.

See [app migration verification](qa/replay-app-v3-2026-10-04.md) for the actual
Light/Swift app acceptance, controls, clocks, bounded rewind and export matrix,
and [codec verification](qa/replay-v3-codec-2026-10-04.md) for independent golden
encoders and hostile inputs. The Windows candidate now uses a separate
[reviewed source contract](qa/replay-candidate-contract-2026-10-04.md), preserving
historical hashes and independent goldens while binding the expanded source
boundary. Pin migration does not qualify a Windows binary; the exact integrated
source still needs the complete candidate run. The earlier
[frozen-source audit](qa/replay-candidate-pin-audit-2026-10-04.md) records the reason
for that deliberate migration.

## Historical identity-only foundation verification, 2026-10-04

Using Rust 1.93 and the coordinated shared build target on Linux, these checks
passed on the isolated identity worktree based on `0581a4d`, before the codec
migration. These source-body claims describe that foundation only; the additive
codec subsequently extracts shared helpers while preserving behavior. Current
codec evidence is in [v3 verification](qa/replay-v3-codec-2026-10-04.md):

- `cargo test -j 2 -p flightsim-sim --all-targets`: all passed, including 47 new
  identity integration tests and one new model-revision unit test; the existing
  fidelity, hostile-byte, numeric-boundary, fixed-step, climate, turbulence and
  hands-off suites were retained
- `cargo clippy -j 2 -p flightsim-sim --all-targets -- -D warnings`
- `cargo test -j 2 -p flightsim-sim --doc`: both doctests passed
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-sim --no-deps --document-private-items`
- Independent Python reference encoder, changed-file rustfmt and `git diff --check`
- Direct source comparison against the base: the bodies of `aircraft_fingerprint`,
  `Recording::read_from` and `Recording::write_to` are byte-for-byte unchanged

No dependencies were added. Full workspace/Windows/app/native/GPU checks were
not run in this bounded sim-only task. The commercial candidate script pins the
entire `replay.rs` source, so the additive API changes invalidate that pin. Those
pins were deliberately left unchanged; candidate requalification requires
separate review and evidence rather than a blind hash refresh.
