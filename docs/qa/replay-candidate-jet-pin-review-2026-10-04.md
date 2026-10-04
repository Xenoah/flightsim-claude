# Swift candidate binding after the additive headless jet milestone

This records the headless milestone at the commits below. The later explicit
native v2/v4 dispatch has its own [source-binding review](replay-candidate-native-jet-pin-review-2026-10-04.md);
the historical statements here about app support do not describe that later source.

Independent source-boundary review on 2026-10-04. Reviewed integration:
`b8ff801ea3a36a1934d01d47b24da6851959e1ed`. Previous contract source:
`c80596c0f73a03d2dff45efe75ddd15265dfc9ca`. This change owns only the reviewed
manifest, its checker guards, Python regressions and this record. It changes no
runtime, staging list, rights record, dependency review or publication authority.

## Exact source delta and retained behavior

Before rebinding, the committed-byte candidate test failed on the stale FDM
module-root pin. The full comparison found exactly four changed entries among
the previous 26 pins; the other 22 remain byte-identical:

- FDM `lib.rs` adds only `pub mod subsonic;`
- Sim `lib.rs` adds five module exports: profile v2, jet scenarios, model identity,
  model simulation and replay v4
- Sim `replay.rs` exposes its current module and nine existing helpers with
  `pub(crate)`; no body, numeric limit or codec byte order changes
- Sim `replay/current.rs` exposes seven existing helpers within the crate and
  wraps three signatures; all existing bodies and dispatch remain unchanged

The review reconstructed each entire current file from the prior Git blob using
only those explicit additions/visibility edits and compared exact bytes. This
is review evidence, not the hash algorithm: candidate pins always hash complete,
unmodified files, independently checking canonical Git and checkout bytes.

The unchanged app still loads its own bounded profile-v1 parser and constructs
the old FDM/simulation. It does not call `AircraftProfileV2`, `JetFlightDynamics`,
`JetSimulation` or `ModelReplayFile`. New flights still use `CurrentRecorder`
and v3's complete algorithm-1/schema-1/FDM-revision-2 identity. Complete replay
admission still compares the entire physical identity, including `yaw_rate_p`.
Legacy v1/v2 admission still needs explicit opt-in, a supported complete baseline
and the persistent missing-yaw notice. Default Swift still rejects the Light
partial fingerprint `0505e6644bb29a53`; the positive case still requires explicit
Light/no-model selection. App main/profile/distribution/policy, old identity and
player, both bundled profile JSON files and legacy force arithmetic are unchanged.

**The Swift candidate still rejects replay v4.** App startup calls the old
`ReplayFile::read_from`, whose variants remain V1/V2/V3 and whose unmatched
version arm returns `UnsupportedVersion` (expected 3). Profile version 2 likewise
remains unsupported in the app. The separate model wrapper/headless command is
the only new v4 entry point. Existing Rust coverage rejects all ten v4 fixtures
through the old wrapper; that prior result is recorded in
[headless QA](jet-headless-v4-2026-10-04.md), not rerun by this binding review.

## Bounded additions to the reviewed path set

The manifest grows from 26 to 28 complete files:

- `crates/flightsim-sim/Cargo.toml` now participates in legacy app feature
  unification. It adds `serde`, `serde_json/raw_value` and the separate headless
  binary. Pinning this changed declaration prevents silently enabling
  `float_roundtrip` at that boundary. The accepted
  [profile review](aircraft-profile-v2-2026-10-04.md) records the independent
  default-decoder bit comparison and twelve actual app v1 tests. This review
  does not rerun Cargo or assert that one manifest pins the entire feature graph
- `crates/flightsim-fdm/src/landing_gear.rs` is shared with the old force path.
  Its only additions are the `Meters` import and a 1,268-byte documented signed
  clearance helper, called by the separate jet runtime. Removing just those
  additions recovers the entire prior file exactly, including normal-force,
  loads and imminent-contact arithmetic. The whole file is now guarded

The FDM module root retains a strict literal full-file guard at
`REVIEWED_ADDITIVE_FDM_LIB_SHA256`. Its historical hash remains unchanged in
`LEGACY_SOURCE_HASHES`; neither the historical nor an arbitrary replacement hash
can substitute for this reviewed additive file. The Light JSON and aircraft
configuration guards remain tied to their original historical hashes.

All 13 independent identity/codec/golden anchors, all five historical hashes,
the historical baseline SHA, and the legacy partial fingerprint are unchanged.
No v1/v2/v3 golden, oracle script or digest was regenerated. New isolated jet
implementations and the separate pre-scene readback diagnostic are not described
as old replay compatibility evidence. They remain covered by the clean exact-HEAD,
complete-tree, every-tracked-file source inventory before and after qualification.
The diagnostic protocol-v2 parser, all 51 existing candidate tests and the
nonqualifying capture boundary are retained.

## Reproduction and checks

Inspect the complete boundary delta with:

```sh
git diff c80596c0f73a03d2dff45efe75ddd15265dfc9ca b8ff801ea3a36a1934d01d47b24da6851959e1ed -- crates/flightsim-fdm/src/lib.rs crates/flightsim-fdm/src/landing_gear.rs crates/flightsim-sim/Cargo.toml crates/flightsim-sim/src/lib.rs crates/flightsim-sim/src/replay.rs crates/flightsim-sim/src/replay/current.rs
python -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
```

All **166 Python tests passed**: candidate 52, stager 21, readiness 38, release
authorization 43, release workflow 8, source-CI workflow 4. Both frozen independent
Python encoders run within that suite without write/regeneration options.
New committed semantic mutations verify refusal of `float_roundtrip`, a changed
old gear-force coefficient, v4 admission in the old dispatcher, profile-v2 app
admission, bypassed legacy opt-in and bypassed complete-identity comparison.
The existing per-path drift tests now also cover both added pins. Manifest row
removal and repinning the reviewed FDM root to its historical hash fail closed.
Canonical/checkout mismatch, exact-source evidence, no-model/notice requirements,
Swift-only staging, readiness blockers and release authorization remain covered.
`git diff --check` passes. Clean post-commit source collection and its exported
source-evidence validator are checked separately against the final binding commit.

No Cargo, native app launch, Windows candidate run, binary distribution or rights
approval is performed here. The prior public source `beb71cb` / public commit
`dfd7754` had green source CI but failed actual Windows capture; those outcomes
do not qualify this integration. Its new native probe and exact-source Windows
run remain separate work. Binary rights/dependency review and publication remain
blocked until their actual requirements are met.

## Follow-up: preserve implicit legacy Cargo command selection

Reviewed source `bc5983ed5cd454fedafad648c185b80dd8139ad9` adds exactly
`default-run = "flightsim-headless"` to sim's package table. With two binary
targets, this restores the formerly unambiguous `cargo run -p flightsim-sim -- ...`
selection. The explicit jet command remains separate. Both binary names/paths,
dependencies/features and all runtime files are unchanged. Exact-byte comparison
confirms this review applies the identical one-line source change.

Only the sim Cargo pin and reviewed-source pointer advance; the other 27 pins,
all 13 independent anchors and every historical hash stay unchanged. The semantic
mutation test now also rejects removal of `default-run`. The targeted candidate
mutation/path/frozen-input checks plus all stager tests passed (24 tests). The full
candidate/stager suite passed all 73 tests; clean exact-HEAD source collection and
exported-evidence validation passed after committing. Actual legacy/explicit-jet command execution
and refreshed metadata-bound inventories belong to integration verification;
this review performs no Cargo command and makes no new native/Windows claim.
