# Reviewed capture-backpressure source bindings, 2026-10-08

This is the source-admission migration for
[ADR-0029](../adr/0029-capture-render-backpressure.md), following the existing
[candidate maintenance procedure](../release/swift-windows-candidate.md#reviewed-source-contract-maintenance).
It does not establish successful Windows capture, visual acceptance or release
permission. The measured problem is growing submitted work ahead of completion;
a callback defect was not established by the diagnostic.

## Reviewed implementation and source closure

The independently reviewed runtime preserves Bevy's extraction function and runs
its entire original Render schedule after obtaining a frame credit. Only an
explicit native screenshot session installs the outer schedule. Two outstanding
render-frame batches are allowed; completion callbacks own isolated atomic flags
and release only a completed prefix. A 100 ms polling timeout leaves credit debt
intact; other poll errors fail before another Render. Cancellation drops only CPU
bookkeeping, and late callbacks cannot complete a new session. Screenshot delivery
ends the session before save success, save failure or a removed request is handled.

A stalled capture can block render/main progress and later UI input. This is a
batch-count bound, not a bound on all internal submissions or GPU bytes, a time
or speedup guarantee, or GPU readiness. The independent process watchdog remains
necessary. No extraction, render preparation, graph work, collection or cleanup
step is skipped. Original CPU readiness, the previous committed-scene observation,
screenshot delay, 30-frame floor, PNG persistence and exit rules are retained.
Normal launches register no gate; wasm does not install native blocking polling.

Exactly two of the previous 160 replay source pins advance: app `main.rs` registers
the native module, and `screen_capture.rs` invokes it and ends its shared session.
The remaining 158 previous pins and all 102 independent anchors remain unchanged.
Four added whole-file paths bring the frozen boundary to 164:

- `crates/flightsim-app/src/capture_backpressure.rs`, including the bounded credit
  state machine, actual schedule integration and retained Rust witnesses
- `crates/flightsim-app/Cargo.toml`, which explicitly selects the already-locked
  `wgpu-types =27.0.1` with default features disabled
- `Cargo.toml`, which owns the inherited Bevy version and feature declaration
- `Cargo.lock`, which supplies the complete reviewed engine/type dependency bytes

Those build inputs previously belonged only to the analytical source boundary.
The ordinary candidate now protects them too because the new helper relies on
the exact reviewed Bevy/wgpu extraction, submission, timeout and callback lifecycle.
Complete files are hashed without projections or normalization. No helper, build
input or source path is wildcarded or silently omitted.

The lockfile changes only the app's edge to the existing wgpu-types package.
Independent comparison preserves all 603 package identities/checksums and all
589 registry-node feature sets. No engine patch, version change or new backend
feature is introduced. This comparison is dependency identity evidence, not a
license or platform review.

## Retained acceptance and adversarial witnesses

All 39 existing candidate checker functions, all 100 existing candidate test/helper
methods and all 14 analytical checker functions are retained. The checker change
is only the explicit added path set. All legacy provenance, fingerprints, codecs,
FDM laws, independent fixture bytes, replay refusal, partial-identity disclosure,
ordinary quality/backend/adapter selection and capture watchdogs remain intact.
No retained fixture or golden is regenerated.

Three additive Python tests protect the new boundary:

- Independent literal lifecycle and engine anchors check the two-credit bound,
  finite service wait, cancellation/ownership, wait-before-Render-before-callback
  ordering, intact Render execution, native capture-only registration, unchanged
  readiness/delay/30-frame floor and ending the session before every save outcome.
  They also retain the five independently reviewed upstream package checksums and
  the default-feature-disabled direct type dependency
- Thirty committed bad-source mutations cover bounded credit, finite waits,
  timeout debt, cancellation, callback ownership, ignored errors, original Render
  execution, ordinary/native scope, preserved readiness and engine/build inputs
- Five exported-evidence attacks for each of the six affected owner/build files
  reject omissions, missing reviewed records, canonical substitution, coordinated
  checkout substitution and a resealed removed contract row, totaling 30 cases

These tests check source admission, not Rust execution, actual GPU progress or a
native pipelined renderer. Existing all-file drift tests also cover every added
path and every retained independent anchor.

## Minimal dependent bindings

The analytical source path set stays unchanged. Only the actually changed main,
app manifest, lockfile, candidate checker, replay contract, analytical count comment
and analytical count assertion receive updated hashes. Its reviewed-source pointer
matches the replay contract's verified runtime identity. The executable analytical
checker is unchanged; only its source-count comment changes from 160 to 164.

The capture runner changes only its two inherited contract hash literals. The
capture contract repeats those values and updates the runner's whole-file digest.
Its original provenance `75753f5e1dfe8d56d321775cb800b29bf058b3e3`, four-file boundary,
other three hashes, modes, commands, Windows restriction and non-qualification
status remain unchanged. No workflow, stager copy set, right, dependency approval,
publication receipt, reduced release choice or release-authorization gate changes.

## Verified source and checks

The verified remote runtime commit is
`f54c5d3060e640f8cd2d01450da112472baff832`, tree
`755064120baf917e4179e1e5f976533003f7dc5a`, sole parent
`afff4a7ccb35ce2f075ca6f404ffc2e664fceb4f`. Each of the six affected source/build
paths was independently fetched at that actual remote SHA and matched to the
reviewed runtime's exact Git blob before any contract was updated.

The implementer's Linux Rust 1.93 runtime checks passed 1300 rendering-group tests
and nine bench smoke cases (three existing fixture/manual tests ignored), 1320
pure-Rust tests and 49 bench smoke cases, six doc tests, workspace all-target Clippy,
private-item rustdoc, ordinary and analytical/commercial combined-mode compilation,
formatting and architecture checks. Test code generation used explicit local
unoptimized/no-debug profiles; these are correctness checks, not release builds,
performance measurements or actual Windows execution.

Before migration, the 318-test Python run reported seven failures, 21 errors and
one existing Windows-only skip because the source boundary still held the old
reviewed bytes. Those results remain failures; they are not relabeled as a pass.
The complete migrated `scripts/tests` suite ran 321 tests in 128.775 seconds:
320 passed and the existing Windows-only junction test was skipped. The three new
methods also passed separately, including all 30 committed source mutations and
30 export attacks. The retained baseline tests and independent fixtures were not
weakened to obtain these results.

Canonical replay/export, analytical/capture and release-preflight receipts are
produced separately against the final clean commit so recording them cannot
change that source identity. The additional independent migration review inspects
that same final boundary. The clean release preflight reports **ten blockers** and
`authorized=false`: all nine previous blockers remain, plus
`DEPENDENCY_LOCK_CHANGED`. The reviewed app dependency edge changes the exact
Cargo.lock bytes, while the committed dependency notice inventory still binds the
previous lockfile. Unchanged package versions/checksums and registry features do
not waive that exact inventory-binding check. This migration does not alter the
inventory, select licenses or issue a positive review/authorization to hide it.
Native capture remains a separate requirement; no successful GPU or Windows result
is inferred.
