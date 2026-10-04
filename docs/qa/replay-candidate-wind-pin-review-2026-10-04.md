# Swift candidate source binding for explicit new-flight wind

Reviewed source: `e05cf711ef58b09536e8e65154baef3427827b08`, tree
`ef7b3c50c5b22ea33eba8844bd0e0722348128ed`, against local baseline `ed83116`
(public main `7e21c23`). The baseline includes the accepted
[picker binding](replay-candidate-picker-pin-review-2026-10-04.md) `e392e046`:
41 whole-file pins and 13 independent encoder/golden anchors. Its manifest
SHA-256 is `8a436572a33b57d224cbffe2bbd93e1d6a53a4f68a4765bbe1fcbf4955c0f51f`.
The complete 17-path implementation delta at `9f463d241ad2c67415a1aad119975b0dd0d449b7`
was inspected before any digest moved. The final source only formats the included
`wind_settings_tests.rs`: assertion expressions, input values and runtime bytes
are unchanged. That complete test file is pinned in its final formatted form.

This review changes only the candidate's required source set, manifest, Python
regressions and this record. It does not change runtime source, qualification
commands, payload membership, rights records or release authorization. The
[wind guide](../new-flight-wind.md) and
[ADR-0017](../adr/0017-explicit-new-flight-forces.md) describe the feature; native
and aggregate acceptance remain separate from this source-binding review.

## Reviewed environment and transaction boundary

- App `PhysicalConditions` keeps exact wind direction/speed, turbulence amplitude
  and seed, and both explicit-override flags. Equality compares the floating
  values by bits, including signed zero, and also compares seed and provenance.
  `edited` validates a copy; a bad later field cannot partly publish earlier
  fields. Only explicitly supplied fields are parsed, using the existing CLI
  wind parser and unit conversions. Applying no changes preserves all values
- The UI begins with both numeric fields clean and no turbulence choice. Rounded
  display strings are never parsed back into untouched physical values. An
  explicit strength choice sets only intensity; the original deterministic seed
  survives every level, including Calm. Explicit field edits set the corresponding
  existing override flag, so later difficulty resolution cannot erase the choice
- Startup replay, the legacy replay resource and the typed jet replay owner
  disable pending physical selection. Host and join configurations disable it
  independently. App admission rechecks those locks and modal readiness even
  for injected actions. Existing manual-cloud flags keep their recording block
  while allowing the independent physical editor
- Opening, editing, applying and canceling the child invalidate Start generation.
  Its ordered input owner consumes the opening/closing frame, repeated Enter,
  underlying map actions and cloud/aircraft shortcuts. Focus loss discards input;
  a session lock closes the draft. Map close/reopen restores committed values.
  The added child panel has its own input and real-font layout witnesses
- `PendingFlight` includes exact physical conditions beside aircraft, destination,
  month, visual weather and region. Matching requires the exact snapshot and the
  existing request generation throughout regional and scene preparation. Editing
  then restoring values cannot revive admitted work. Failed preparation retains
  active conditions, scene and recorder; another explicit Start is required
- Pending conditions are applied to the copied startup before wind-aware airborne
  state, simulation and recorder construction. Final publication uses the existing
  complete-aircraft transaction. The added loader/ECS tests inspect initial
  velocity, recorded environment and replayed physical state, rather than only
  labels. Restart keeps committed values and resets the existing gust clock
- Visual Clear/Rain selection remains independent of forces. The new pure tests
  check compass-from/unit vectors, force and motion response, deterministic seeds,
  exact environment round trips and replay/rewind across render cadences. They
  change no physical implementation, identity, profile or v3/v4 bytes. Jet branches
  remain explicit support and do not become Swift package qualification

All FDM and sim production files, model/profile assets, manifests/lockfiles,
staging/rights/authorization files and workflows are byte-identical to the
baseline. Existing CLI defaults, complete/legacy identity admission and explicit
Light/no-model inspection exceptions remain unchanged. No fixture or oracle was
regenerated, and no historical evidence is relabeled.

## Exact source pins and rejection coverage

Six existing whole-file digests advance: app `aircraft_picker_runtime.rs`,
`aircraft_picker_tests.rs`, `main.rs`, `world_runtime.rs`, and UI `lib.rs` /
`world_map.rs`. The other 35 existing digests remain unchanged. Six complete files
are added, bringing the required set to 47:

- App `conditions_runtime.rs`: exact environment, provenance, validation, locking,
  pending selection and scheduling ownership
- App `conditions_runtime_tests.rs`: untouched values, individual fields, seed,
  override, atomic failure, signed zero, replay/LAN and close/reopen witnesses
- UI `wind_settings.rs`: clean/dirty draft ownership, bounded typed edits, default
  preservation and explicit Apply boundary
- UI `wind_settings_tests.rs`: real ordered input, cancellation/generation,
  repeated shortcuts, disabled sessions and bounded real-font layout
- Sim `wind_force_contract.rs`: independent unit/direction vectors and actual
  force/trajectory response to the exact physical inputs
- Sim `wind_replay_rewind.rs`: exact recorded environment, retained formats,
  deterministic cadence/rewind and visual/physical independence

All 13 independent anchors, five historical hashes, historical baseline, frozen
FDM guards and legacy partial fingerprint remain unchanged. Whole raw files are
pinned; no function extraction, newline normalization or hash projection is used.
The complete tracked source inventory still binds the exact clean run HEAD/tree.

Every old candidate test function and semantic mutation remains unchanged.
No assertion adaptation or removal was needed. Two new groups add 23 deliberate
mutations: dirty-by-default display fields, implicit Calm, removed bounds, seed
reset, non-bitwise comparison, omitted seed/provenance, lost override flags,
bypassed replay/LAN/modal locks, lost generation invalidation, incorrect schedule,
ignored Cancel, stale/missing snapshot matching and omitted application before
initial-state construction. Each altered file is committed in a disposable source
fixture and must fail exact-source collection before a candidate could build.
These checks exercise immutable source acceptance, not execution of a mutated
Rust program; the separately pinned Rust tests own behavioral evidence.

An AST comparison confirms that the checker's required path set is its sole code
change. Every other constant/function remains identical: Swift-only identity,
Windows/MSVC target, ordinary default features plus commercial-staging, payload
and literal false-authorization validation, acceptance commands, the ordinary
180-second full-scene capture and the default-disabled diagnostic protocol.

## Checks and limits

All **174 Python tests pass**: candidate 60, stager 21, readiness 38,
authorization 43, release workflow 8 and source-CI workflow 4. This includes the
unchanged independent encoders, old/new semantic mutations, expanded per-file
drift, CRLF checkout rejection, source/export binding, and diagnostic opt-in and
nonqualification checks. `git diff --check` also passes.

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
```

This review runs no Cargo, GPU/native/UI process, Windows candidate/diagnostic,
workflow trigger or publication. Reported app/UI/pure-Rust implementation results
belong to their separate owners; this record does not claim remaining aggregate
gates. Windows full-scene capture acceptance and dependency/rights review remain
unresolved. Source acceptance cannot authorize a binary or substitute a
diagnostic readback for the required full-scene Swift image.

## Follow-up: complete wind-independence note

Final reviewed source: `bf79f846bdfd8da768306415082960a85616b329`. The intervening
`fe6f0aef233e65287c58a8403e33cba5268d9d34` only adds the implementation owner's
[automated QA record](new-flight-wind-controls-2026-10-04.md); it changes no pin.
The subsequent two-file delta corrects a note clipped by the UI's existing
bounded text formatter:

- Old: `Authored forces for next Start; visual presets do not change wind`
- New: `Visual presets do not change wind or turbulence.`

The new note is 48 ASCII characters. Existing panel text still explains that
conditions apply on Start. Physical values, defaults, validation, snapshots,
transactions, recording, geometry and input handling are byte-unchanged.
The new `physical_settings_note_is_complete_after_ui_formatting` app regression
opens the real wind editor and passes its note through `format_world_map_text`.
It requires the exact complete new string and separately rejects an ellipsis.
No old assertion, fixture or semantic mutation is removed or adapted.

Only app `conditions_runtime.rs` and `conditions_runtime_tests.rs` hashes advance,
along with the reviewed-source pointer. The other 45 digests, exact 47-file set,
all independent/historical anchors, checker implementation and acceptance policy
stay unchanged. The same 174 Python tests and `git diff --check` pass after this
focused update. Native evidence and final integrated source inventory remain the
integration owner's gate; no native/Windows acceptance is inferred here.
