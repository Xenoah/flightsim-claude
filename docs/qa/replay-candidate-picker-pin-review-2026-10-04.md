# Swift candidate source binding for the staged aircraft picker

Reviewed source: `7a955b75f2845e9da5c818be4b2113b476a3f2b1`, against previous
binding `965b24ff20cfc351ec0774d14d2fd04da41b03c0`. Review covers the initial
implementation `1ce3fa723d4a0b69737e372df49414adad480a78`, its automated record
`337cf6b006682c013d3780f8fbce00862e06cfca`, and the final bounded polish.
The [picker guide](../aircraft-picker.md),
[ADR-0016](../adr/0016-transactional-new-flight-aircraft.md) and
[automated verification](aircraft-picker-automated-2026-10-04.md) describe the
transaction and its independent source/automated review. This document records
the separate candidate-boundary review; it confers no native, Windows, speaker,
distribution or rights acceptance.

## Reviewed behavior and preserved boundaries

- The picker snapshots validated profile/model/fit/sound choices and target
  departure/month/weather/region. Preview alone changes no active flight. Request
  generation and all selected values must still match before a staged result
  can commit. Cancellation and edits retire pending work; returning to previous
  values does not revive it. Failures retain the old flight and require another
  explicit Start
- The commercial catalog has two independent source gates: it omits the Launch
  row and skips every named preset except Swift. Named presets must match the
  expected ID, model path and physical family and pass the existing distribution
  model validator. The existing explicit CLI inspection exceptions remain;
  they are not new picker choices. No model or profile is downloaded or added to
  packaging, and the complete-source asset set is not the candidate copy set
- Replay locking checks the old replay resource, startup replay selection and
  the jet player's ownership. Legacy target flights still construct the original
  `Simulation`/`CurrentRecorder` with complete v3 identity; jets still own their
  separate v4 session. No profile, force law, simulation revision, codec,
  identity check or independent replay fixture is changed
- The aircraft scene helper now owns initial scene construction as well as
  hidden replacement preparation. Startup preserves the selected model, fit,
  profile eye, materials and implicit development fallback. The exact Swift
  model-path and fit logs used by candidate acceptance remain. Staged models
  must pass parent-document/dependency checks, scene readiness, an additional
  transform/bounds pass and fitting before activation. Embedded cameras/lights
  are rejected before instantiation. Failed or canceled candidates cannot replace
  the active hierarchy; only generated candidate assets are explicitly removed
- World preparation is extracted from the existing map-start owner and retains
  terrain, weather, control and recorder construction. The target family, rather
  than the old active family, controls terrain admission. Jet source restrictions
  run before consuming raw/active/selected/pending regional state. Regional
  results also carry aircraft index and request generation
- Commit replaces physical/session/recorder and presentation owners in one
  exclusive operation. Both-family HUD reseeding, warning reset, profile camera
  eye, cleared crash/replay notices and updated guidance are intentional new
  presentation semantics. Final UI readers run after commit and state publication.
  This is not a claim that all legacy presentation timing is unchanged; legacy
  physics and ordinary smoothing arithmetic remain unchanged
- Audio replacement retires the old tagged source, asset and shared bridge,
  immediately mutes retained decoders, then creates the selected synth family.
  Enabled/master settings and unrelated audio remain. Startup synthesis and
  sample-generation implementations retain their prior behavior; no speaker
  listening evidence is inferred from ownership/decoder tests
- The bounded UI catalog, keyboard/button ownership, modal cancellation and
  narrowed map column are reviewed presentation changes. The Light Single
  weather test now explicitly selects the independently encoded fixtures' actual
  aircraft under commercial-staging; it does not rewrite those fixtures or
  weaken mismatch rejection. Other small existing-test changes supply the new
  aircraft-index/generation request fields

## Exact source boundary and negative checks

Nine existing pins advance: app `main.rs`, `jet_runtime_tests.rs`,
`region_runtime.rs`, `replay_migration_tests.rs`, `weather_runtime.rs`,
`world_runtime.rs`, and UI `lib.rs`, `replay.rs`, `world_map.rs`.
The other 27 existing digests remain unchanged. Five complete files are added,
bringing the exact required set from 36 to 41:

- App `aircraft_picker_runtime.rs`: commercial catalog, replay/source admission,
  snapshot, generation, stage and commit ownership
- App `aircraft_scene.rs`: shared initial scene construction, readiness and
  replacement hierarchy ownership
- App `aircraft_picker_tests.rs`: actual loader/ECS assertions for transaction,
  commercial policy, identity, cancellation and publication of fresh HUD state
- Audio `lib.rs`: actual old/new synth, bridge, entity and asset ownership
- Audio `lifecycle_tests.rs`: retained decoder silence, selected synth, repeated
  replacement and enabled/master/unrelated-source expectations

Whole-file hashes include tests and helpers; no normalization or function-only
hashing is introduced. All tracked files still participate in the clean exact-HEAD
source inventory. The contract identifier, acceptance commands and required
Swift model/PNG evidence do not change. The 13 independent anchors, five
historical hashes, frozen FDM guards, default profiles, dependency inputs,
stager/rights/authorization files and automatic workflow remain unchanged.

The previous negative mutation that forbade expanding a jet-only HUD reset is
superseded explicitly because a committed picker replacement now needs both
families reset. Its new mutations (1) restore the obsolete jet-only condition,
(2) force a pending reset false and (3) remove the actual `smoothing.reset(&hud)`
call. All unrelated old mutations remain. New mutations also bypass each
commercial catalog guard, profile-family check, generation/replay locking,
scene dependency/readiness/hidden-state requirements and old audio-entity
retirement. Each modified file is committed in a disposable source fixture and
must fail exact source collection before any candidate could be built.

Those Python mutation checks test the immutable source boundary, not Rust
execution of modified programs. Actual display assertions belong to the pinned
ECS tests; source acceptance and behavioral test execution are distinct evidence.
The CRLF regression now finds the first changed pinned file rather than assuming
the old profile sorts first; it still explicitly verifies that profile's bytes
differ and that explicit LF checkout restores every pin. The checker itself
continues to reject all checkout/canonical newline differences.

## Final presentation and behavioral regression review

The final six-file follow-up changes no transaction/admission predicate. The app
supplies the raw aircraft name; UI owns its single Current/Locked prefix and the
40-character original-name budget. Short preset notes keep their factual model
limits, while Preparing/regional rejection use two complete bounded lines.
Real-font tests cover original live/locked names at 1024x720, 1180x812 and 1280x720.

The HUD regression now seeds actual `HudSmoothing` with 5000 m and +26.4 m/s,
asserts those displayed values differ from the current HUD, and verifies they
persist during asynchronous preparation. Its same-commit observer then compares
HUD altitude/V/S to the authoritative simulation and displayed values to that
fresh HUD, with exactly one observed legacy commit. The fixture includes the
production capture -> Start -> advance -> sound -> HUD ordering at zero physical
time; the previous fixture lacked the advance system and therefore did not prove
that transitive edge. Restoring a jet-only reset now leaves explicit stale legacy
values, rather than accidentally comparing two equal 1000 m/zero-V/S starts.

## Verification limits

All **172 Python tests passed**: candidate 58, stager 21, readiness 38,
authorization 43, release workflow 8 and source-CI workflow 4. These include
the unchanged independent encoders, both supported readback evidence protocols,
default-disabled diagnostics, every old/new semantic source mutation and exact
canonical/checkout byte checks. `git diff --check` also passes.

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
```

This review runs only Python contract/regression checks and exact-source
collection. It runs no Cargo, native/GPU/UI process, Windows diagnostic,
workflow or publication action. Final native picker observations, any resulting
source fixes and fresh platform CI remain separately owned acceptance gates.
