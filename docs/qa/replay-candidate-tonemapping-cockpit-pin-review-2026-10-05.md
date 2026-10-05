# Candidate source binding for analytical routing and cockpit replay help

Reviewed combined source: `b1a62afc18407859be7677999077b1b0ea3fc577`, tree
`79bea00dbac5663564ea13aed404c75803b36873`. This isolated integration combines
analytical feature commit `c36100a08128076cb7d266356e1b8106c601849f`, its
script-only hardening follow-up `98de359aae533dad4aa914bc4caabcd5239d82f1`,
and UI commit `332dcce05bb269c2e00085246d1b2f31a0cc1db0`, against accepted
local source `5530f91bb86bde2d4538a94d66d8b584e8e0f2b7` (public equivalent
`e643748b471e2ece0623ffd6481f776539b5f9e9`). Each implementation delta received
independent source review before this binding migration. No blocking source
defect was found; full integrated build/native acceptance remains separate.

## Reviewed behavior and whole-file boundary

The [explicit analytical mode](../analytic-tonemapping.md) preserves the default
ordinary LUT route, Bevy version and lockfile. Main's analytical-only cfg statements
and compile guards change its complete source hash even when ordinary runtime
behavior is unchanged. The hardened build auditor is engineering evidence, not
source authentication, legal clearance or candidate qualification.

The [cockpit replay-help correction](cockpit-replay-help-2026-10-05.md) wraps help
in the space beside the fixed six-pack, using its actual shared width before UI
layout. The complete `instruments.rs` file now joins the boundary because that
geometry directly supplies the bound UI layout and its actual-instruments
regressions. No selected-function projection or normalized hash is introduced.
The documented 640x480 long-notice/HUD limitation remains; this binding grants
no broader visual acceptance.

Three existing whole-file digests advance:

- `crates/flightsim-app/src/main.rs`:
  `40f6ba06a9d2df87546a9cb9b81e6889eba9cfaa6ea9d88f1077bfca6c1f338a`
- `crates/flightsim-ui/src/lib.rs`:
  `407dcde6c93239ec7ef85e293fbb7b88244756b905d93fb66e4b4ec2b6927855`
- `crates/flightsim-ui/src/top_layout_tests.rs`:
  `76bca4fb6db1747775076dafdb9e20cdd1f0e7b25376b43a3e4519c22c6d7199`

The added complete `crates/flightsim-ui/src/instruments.rs` digest is
`9295167b8c1c4089b441f92c36510884fc863a993d5f8882267ef32e9b1e2ab4`.
This produces **130 whole-file pins**, preserving the other **126 previous pins**
and all **102 independent anchors**. `reviewed_source` names the combined source
above. The contract schema/identity and raw canonical Git/checkout byte rules
are unchanged. Independent encoders and fixture bytes are never regenerated.

## Preserved acceptance and additive rejection coverage

AST comparison with the accepted source preserves all **36 candidate checker
functions**, all **92 existing candidate test/helper functions and methods**, and
all **36 other checker constants**. Only the required whole-file path set grows.
The ordinary MSVC/Rust 1.93 release recipe still requires app defaults plus
`commercial-staging`, upstream LUTs, Swift-only distribution, the original four
case outcomes, and the 180-second capture watchdog. Its commands, evidence
validators, stager, readiness/release authorization scripts and candidate/release
workflows are unchanged.

The frozen app acceptance file `crates/flightsim-app/src/replay_migration_tests.rs`
retains SHA-256 `05b3b6ca71e696e6c88e5aae6da300fc7c7eec07f98e23d122a5805adb59cef6`.
In particular, `legacy_layout_test` still selects
`replay_migration_tests::real_replay_notices_fit_narrow_resizes_and_keep_live_tutorial_clear`
with its exact release/MSVC invocation and one-passing-test requirement. The new
actual-instruments regressions supplement that witness; they do not rewrite it.

Two additive Python methods check three committed instruments mutations
(dial gap, shared width formula and actual panel centering), plus five exported
omission/rehash mutations (missing file, missing reviewed record, changed
canonical digest, coordinated checkout digests and removed contract row).
The removed-row case reseals both the manifest and report; it still fails the
exact required-path boundary. Existing full-path drift coverage also automatically
includes the new file. Both new methods passed in the isolated integration.

Required source-only verification commands:

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow scripts.tests.test_tonemapping_build
git diff --check
```

Run `source_inputs()` on the clean committed migration, then validate its exported
source evidence; uncommitted source or a merely refreshed checkout hash does not
satisfy the canonical contract. Final command output belongs to the exact
integration's QA record.

This migration ran no Cargo build, GPU/native or Windows process, created no
rights receipt, and published nothing. It adds no analytical binary/candidate
qualification. Any future ordinary candidate needs new exact-source metadata,
notices, executable and capture evidence under the unchanged recipe. The failed
earlier e643 Windows capture stays failed; it was not reopened or replaced.
Rights, dependency review and release authorization remain independent blockers.
