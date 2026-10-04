# Reviewed replay trim presentation source binding

Date: 2026-10-04 UTC. Independently reviewed authored source:
`0c6fe8c7b351ebf9435eb9931e8661617f45c339`, based on the accepted
near-static app binding `47b8a2f26385c3c8780f5c311032299d7d5e917d`.

## Independent review

The eight-file authored correction is accepted with no findings. Recorded
effective elevator combines stick input and pilot trim, so the shared HUD now
represents unavailable separate trim as `None` and displays `TRM N/A`. Its
existing replay-owner predicate covers the legacy `ReplayPlayback` resource and
the v4 jet, v5 forward turboprop and v6 near-static session owners. Live trim
still comes from the local pilot setting with the same number, width and
nose-up/down hint. No recorded elevator is treated as a reconstructed trim.

The repeated live/replay transition witness checks the HUD resource and rendered
text in the same update for both existing propeller aircraft while retaining
local trim. Existing v4/v5/v6 presentation witnesses now require unavailable
trim, and the UI witness covers unavailable/live transitions through positive,
zero and negative values. Tutorial edits only adapt test setup to the optional
value; tutorial behavior is unchanged.

All 2,315 other pre-existing tracked files are identical to the accepted base.
In particular, simulation, physics, codecs, identities, input, schemas, fixtures,
dependencies, assets, release policies and the complete candidate validator are
unchanged. This is a presentation correction and grants no additional aircraft
or release admission.

## Exact source binding

The contract retains exactly 122 required whole-file paths and all 102
independent anchors. Only these seven pre-existing hashes advance to the
reviewed authored bytes:

- `crates/flightsim-app/src/controls_runtime_tests.rs`
- `crates/flightsim-app/src/jet_runtime_tests.rs`
- `crates/flightsim-app/src/main.rs`
- `crates/flightsim-app/src/nearstatic_runtime_tests.rs`
- `crates/flightsim-app/src/turboprop_lifecycle_tests.rs`
- `crates/flightsim-ui/src/lib.rs`
- `crates/flightsim-ui/src/tutorial.rs`

The remaining 115 pins, contract identifier/schema, five historical hashes,
legacy fingerprint/baseline and additive FDM-root guard remain exact. The checker
is byte-identical. All 187 previous Python test function bodies and assertions
are retained without modification.

One additive source-gate regression commits nine semantic mutations to
disposable fixture repositories. It rejects omission of delegated replay owners,
leaked local replay trim, fabricated live or unavailable zero values, a removed
live nose-up hint, and weakened transition/v4/v5/v6 assertions. Every failure
must identify the changed path and raw checkout hash. These mutations are never
compiled or executed as Rust and are not behavioral qualification. Existing
all-path drift and exported-source completeness checks continue to cover every
required path.

## Verification and limits

The six focused Python suites pass all 188 tests: candidate 74, stager 21,
readiness 38, authorization 43, release workflow 8 and source-CI workflow 4.
The independent identity/wire reference scripts run without regeneration.
`git diff --check` passes. The accompanying clean-source receipt records the
complete tracked-file inventory, canonical/checkout byte equality and validation
of the exported source evidence through the unchanged checker.

The author supplied app/UI logs were independently read and hashed: 363 app
tests passed with one pre-existing ignored test; all 246 UI tests passed.
Ordinary and combined `commercial-staging,region-downloads` app/UI all-targets
strict Clippy, formatting, architecture and diff checks passed. Those logs are
supplied evidence, not fresh Cargo runs by this binding reviewer.

This review used only Git, Python and source/log inspection. Native HUD visuals,
integrated-source CI, Windows qualification and publication remain separate
lead-owned gates. No Cargo, native process, GPU, Windows candidate, publication
or release action was performed by the binding reviewer.
