# Swift candidate binding after explicit native jet integration

Reviewed source: `238e89bf6c51cad853811b13e602956b3df95840`, including app
`9842716` and its subsequent documentation update. The complete 41-path delta
was inspected against local baseline `ff409d322372b7af028a15fd2ae7517e4a65ad6a`
(the integration's equivalent of public baseline `f3850a2`). All 28 previous
manifest digests match that local baseline exactly. The old recorded review
pointer is `bc5983ed5cd454fedafad648c185b80dd8139ad9`.

This is a source-boundary and candidate-evidence review. It changes only the
candidate checker, reviewed manifest, Python regressions and binding documents.
It does not change app/FDM behavior, dependencies, staging membership, rights,
workflows, authorization receipts or publication authority.

## Semantic review before rebinding

- `SelectedAircraftProfile` explicitly separates ordinary v1 and exact-decimal
  v2 decoding. Built-in Swift and Light remain `Legacy`. External JSON first has
  its integer version probed without converting the physical numeric tokens;
  the same original bytes then reach the selected decoder. The outer read is
  bounded at 1 MiB; the v1 branch retains its 128 KiB limit. Exact reconstruction
  from the prior blob confirms the entire original v1 structs/implementation
  and original test module are byte-identical. Its version-2 rejection remains
  required; v2 admission belongs only to the separate typed branch
- `FlightSession::Legacy` retains the existing `Simulation`, fixed-step input
  loop, `CurrentRecorder`, replay player, ground classification and presentation
  calls. Legacy restart, controls, audio, HUD, camera, model fit and placeholder
  helpers delegate to those implementations. The former setup body is enclosed
  in the legacy branch; jets are prepared separately. No propeller configuration
  is fabricated for a jet. The migration/controls/runtime/scenery test edits
  adapt typed access without weakening their existing expectations
- App legacy replay startup now reads through `ModelReplayFile`. Its v1-v3
  branch reconstructs the identical header bytes and delegates the remaining
  reader to the unchanged `ReplayFile`. The app explicitly rejects `V4` for
  Swift/Light. The unsupported-version diagnostic now belongs to the model-aware
  wrapper; a valid v4 file is parsed before the model-selection rejection.
  This is not a claim that every error string or parsing cost is unchanged.
  Complete legacy identity comparison, supported-baseline opt-in, missing-yaw
  notice, old physics revisions and original replay bytes remain intact
- Explicit v2 jets use their matching complete model identity and v4 player.
  Their parked placement/presentation APIs and non-consuming exports are
  separate from the legacy simulation. The pure-sim delta adds slope/pole-aware
  placement, read-only diagnostics, bounded replay speed/interpolation, and
  export checkpoint sharing without changing the existing v4 wire layout.
  The native adapter owns jet parking/recording faults, atomic starts and
  authoritative replay weather/time. None of these are Swift acceptance claims
- World/weather/region helpers explicitly dispatch jet starts and playback.
  Legacy terrain levels, initial controls and airborne-speed arithmetic remain.
  Replay preview gains guards for the selected startup/player as well as the
  legacy replay resource; failed navigation now also exposes its error on the
  map. Sun-rate adjustment is explicitly ordered before flight controls and
  stepping. Jet recording closes its valid prefix on a rate change. These are
  app coordination changes, not changes to legacy FDM or replay arithmetic
- UI guidance defaults preserve the original live help/tutorial; only explicit
  jet selection disables the propeller cues. Jet replay still owns playback
  help. Traffic placeholder dispatch returns the same legacy meshes. Jet
  exterior/cockpit, warning and landing-report choices are conditional on jets
- The Kestrel additions are original asset/profile/generation/validation files
  plus four prior Blender previews and their documentation. They do not change
  an existing aircraft or packaging file. The pure-Python binary/schema validator
  reproduces the committed asset evidence exactly. This review neither renders
  those assets nor qualifies their native appearance or flight handling

The remaining documentation changes describe those explicit integration paths
and their limits. No Cargo manifest/lock, FDM file, legacy sim/codec/identity file,
existing aircraft byte, release allowlist or workflow changes in this delta.

## Exact boundary and model-specific evidence

Seven existing hashes advance: app `aircraft_profile.rs`, `distribution.rs`,
`main.rs`, `region_runtime.rs`, `replay_migration_tests.rs`, `weather_runtime.rs`
and `world_runtime.rs`. The other 21 hashes remain unchanged. The
`reviewed_source` pointer advances to the coherent source above.

The required whole-file set grows from 28 to 35:

- App `flight_session.rs` binds the typed legacy adapter
- App `traffic_runtime.rs` binds its placeholder delegation
- App `controls_runtime_tests.rs` and `jet_runtime_tests.rs` bind regression
  expectations at the new session/model boundary
- Sim `replay_v4.rs` binds the newly shared model-aware reader and its legacy
  delegation, as well as its explicit model-identity check
- UI `lib.rs` and `tutorial.rs` bind default legacy guidance and jet opt-in

All hashes cover complete raw files, not selected functions or normalized text.
Every tracked source, including jet-only helpers/assets not newly pinned here,
still participates in the exact clean-HEAD/tree inventory before and after a
candidate run. This selected review boundary is not an exhaustive dependency
hash graph, and pinning a shared file does not confer jet qualification.

All 13 independent Python/fixture anchors, all five historical source hashes,
the historical baseline, frozen Light/FDM guards and partial fingerprint
`0505e6644bb29a53` are unchanged. No golden, oracle or historical digest was
regenerated. Every previous negative mutation is retained, including v2 refusal
in the old v1 decoder and v4 refusal in the old v3 dispatcher.

The candidate and replay contract identities remain Swift-only/v3. The stager
already enforced Swift/default-model/bundled-aircraft. The candidate validator
now enforces those same properties in exported evidence too, including exact
Windows/x86_64/MSVC, commercial-staging, package/schema/version and literal false
authorization. Two equally wrong metadata objects no longer suffice. Explicit
external jet support never makes Kestrel part of the package or lets a jet
image/model identity stand in for Swift's required capture.

## Executed checks and limits

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
python3 tools/blender/validate_kestrel_jet_trainer.py
git diff --check
```

All **168 Python tests passed**: candidate 54, stager 21, readiness 38,
authorization 43, release workflow 8 and source-CI workflow 4. The original
independent encoders run without regeneration flags. Added semantic mutations
reject v1/v2 route mixing, built-in remapping, changed legacy adapter speed/ground
semantics, corrupted legacy header delegation, bypassed jet identity and changed
default tutorial policy. Added exported-evidence cases reject jet/default/model/
membership substitution, absent fields, wrong platforms and bool/int ambiguity.
All per-path drift, canonical/checkout byte equality, source evidence, legacy
notice/no-model and nonqualifying readback diagnostic regressions remain.

The asset validator output exactly matches its prior committed JSON report.
No Cargo, native/GPU process, Windows run, workflow trigger, binary distribution
or rights approval was performed by this review. Native QA and final source
revision remain the integration owner's gate; any subsequent source fix needs
its own exact-delta review before affected pins advance. Prior-source Windows
diagnostic results do not qualify this source or its explicit jet path.

## Follow-up: native HUD findings and explicit diagnostic opt-in

The complete source delta from binding commit
`8f36ece1d23a4b149cb7b5897552d400bf36cf48` through integration
`dd42b3dac78f4f9227e80c7e4b0f49041bcb5eba` was reviewed before further rebinding.
This includes the independently reviewed notice layout and jet HUD history fixes,
plus the explicit secondary-diagnostic opt-in and its completed Windows record.

- HUD text and the persistent notice now share a wrapping layout row measured in
  the same UI pass. The original HUD font and top-left origin remain; notices
  occupy the available column or stack below. Word-or-character wrapping keeps
  long notice tokens inside their node. A hidden banner uses `Display::None`,
  and unchanged visibility/display values no longer invalidate layout each tick.
  Standalone banner creation remains supported. The actual text-layout regression
  checks complete `STALL WARN`/`STALL WARN N/A` glyph bounds, long legacy/jet
  notices, replay phases, resizing, and retained legacy no-notice geometry
- `HudSmoothing::reset` discards the previous flight's displayed history while
  retaining refresh/smoothing settings. The normal update arithmetic is unchanged;
  its display construction is extracted into `refresh_display`. The app calls
  reset only for jet startup, successful R/map reconstruction and each bounded
  F8 seek batch, before the same update displays the completed physical state.
  Failed new flights do not mark a reset. Legacy presentation never takes this
  jet-gated reset path. Tests compare all displayed fields and rendered HUD text
  at zero elapsed time, and also require ordinary live motion to stay smoothed
- The jet help text adds the already-supported `=`/`-` thrust aliases. The input
  implementation is unchanged. No physical state, controls, identity, replay
  codec, terrain, asset or dependency bytes change in these HUD fixes
- The candidate checker defaults to one ordinary capture. Only explicit
  `--diagnose-readback` allows one additional nonqualifying attempt on failure;
  it never runs after primary success or replaces primary failure. Primary-only
  failed evidence retains exact command/launch/timeout/log binding and rejects
  orphan diagnostic files. Historical v1/v2 probe evidence still follows its
  original parser and qualification rules. The automatic workflow is unchanged
  and does not pass the opt-in. No new diagnostic was requested or executed here

The completed Windows observation is tied to public source `f3850a2`, not this
native integration. Its basic pre-scene 4x4 readback succeeded at 3 ms; both full
scene captures timed out and no PNG qualified. The
[diagnostic record](windows-readback-diagnostic-2026-10-04.md#completed-windows-v2-observation-and-opt-in-boundary)
retains that failure and its scope. It is not a production rendering fix, a
successful Windows candidate, or permission for another diagnostic run.

The six affected existing pins are app `flight_session.rs`, `main.rs`,
`world_runtime.rs`, `jet_runtime_tests.rs`, and UI `lib.rs`/`replay.rs`.
`replay.rs` was already in the reviewed set; the newly extracted
`top_layout_tests.rs` is added to prevent its expectations silently drifting.
The new negative mutations reject expanding HUD-reset admission to legacy
sessions, making hidden banners occupy layout and shrinking their readable
column. All old negative anchors remain, as do the exact Swift model/capture,
Swift-only distribution, false authorization and complete source-byte checks.

The subsequent test-only commit `f1ef2c7f47cb4d54336cdc621ba9cc345bf6376f` adds
real-GLB scene hierarchy coverage and its QA record. It checks the entire set of
spawned entities against the complete descendant tree, then requires transforms,
visibility, materials, bounds and propagation for every entity. No missing
component can disappear through a filtered acceptance query. It changes no
production or asset bytes and remains part of the complete source inventory.
Its documented Kestrel B0004 observation is not treated as proof that arbitrary
hierarchy warnings are harmless; see the [specific hierarchy review](aircraft-scene-hierarchy-2026-10-04.md).

Final reviewed source: `138e317e199b6bda8108b1ce1a7c7b48ab70461c`. Its final
seven-line production change separates jet visual stall-warning validity from
audio transport muting. Valid paused/completed/seeking snapshots retain their
warning; terminal and reproduction faults remain excluded, alongside existing
app-fault, nonfinite-state and low-speed guards. The legacy warning branch and
audio mute expression are unchanged. Five added ECS regressions check actual
HUD text and sound state for those boundaries. Additional candidate mutations
reject both restoring the audio-mute bug and bypassing terminal/fault validity.

Only the six reviewed existing digests advance; the other 29 stay unchanged.
Adding `top_layout_tests.rs` brings the exact required set to 36. All 13
independent anchors, historical hashes, frozen FDM guards, Swift-only identity
and release restrictions remain unchanged. No fixture or historical evidence is
rewritten. The final binding covers this production source; later QA-only
documentation remains subject to the run's fresh exact-HEAD/tree inventory.

The same six Python suites now pass **171 tests** (candidate 57, stager 21,
readiness 38, authorization 43, release workflow 8, source-CI workflow 4),
including original independent encoders, explicit/default diagnostic branches,
all added semantic mutations and complete per-path drift checks. `git diff
--check` also passes. This binding review executes no Cargo, native/GPU process,
Windows candidate or diagnostic; the integration owner retains final native QA.

## Follow-up: Windows test-fixture line endings

Reviewed source `c02fc80631cf5e70fccfb712bce70d717b807435` repairs three test
bodies after public source `2f30b519b362304f726377d393f57b1860b420ca` failed
[Windows render CI](https://github.com/Xenoah/flightsim-claude/actions/runs/37203963444/job/111441184921).
The original multiline LF-only replacements did not match CRLF fixture input:
the signed-zero/subnormal eye mutation and optional pitch-field removals stayed
unchanged, as did the intentionally narrowed pressure bound in the rollback test.
The failure log's observed values match those unmodified fixture values.

Only the three `#[cfg(test)]` bodies in app `aircraft_profile.rs` and
`world_runtime.rs` change. Each constructs both LF and CRLF fixture copies in
memory without parsing/reserializing numeric tokens, matches exactly one intended
fragment, proves replacement/removal and retains the original exact-number,
default-control, approach and atomic-failure assertions. This is test setup,
not runtime input normalization. Full production prefixes are byte-identical to
the previous source. The numerical JSON fixture remains byte-identical at
SHA-256 `8339b68183ea31c228082e56984a39775f7f23df5633dd8a333fe06a65be7c74`.

Both files were already pinned: `aircraft_profile.rs` has been part of the
original legacy parser boundary throughout the native dispatch review.
Only those two whole-file digests and the reviewed-source pointer advance;
the other 34 digests, required 36-file set, all 13 independent anchors,
historical hashes, default models, rights/staging/authorization gates and
workflow newline policy are unchanged. The candidate still checks exact raw
canonical/checkout bytes and never accepts newline normalization as equality.

The fix author and integration owner report 34 focused jet tests plus strict
app all-targets Clippy, formatting and diff checks passed. This binding review
passes all 171 tests in the six Python contract suites, including the existing
LF/CRLF checkout-byte rejection and exact-source evidence tests. A fresh remote
Windows result remains separate. No Cargo, native/GPU, workflow trigger or
publication is performed by the binding review.
