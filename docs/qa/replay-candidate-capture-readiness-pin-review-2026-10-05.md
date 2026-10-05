# Candidate binding for screenshot CPU world readiness

Reviewed runtime source: `d00bfced040e86809d3e1057518fe2c9cf07adeb`, against
`edbe6a75cd496845ad59bfb1f5d902b6ed8e5685`. The complete five-file runtime delta
was inspected before rebinding. It contains three implementation/test files and
the corresponding README/architecture documentation.

## Reviewed behavior and boundary

- App `main.rs` moves screenshot registration to `Last`, marks the existing
  runway surface and passes capture-only selector observations. The capture
  gate runs after deferred updates and propagated visibility/transforms
- `screen_capture.rs` retains the minimum delay and 30-frame floor. It requires
  exact displayed/live tile IDs, completed bridge/overlay transactions, model
  fit and a visible nearby runway. Consecutive snapshots bind surface entities,
  overlay revision, runway mesh, model entities, startup revision and origin.
  Valid coarse/empty cuts require examined availability dependencies when raw
  desired IDs do not converge. Distant hidden runways and optional omitted
  scenery remain valid. The PNG save, flush, sync and batch-exit function bodies
  are byte-identical to the baseline
- `terrain_selection.rs` adds opt-in observation of existing dependency state
  and actual fallback outcomes. Unknown or evicted successful dependencies
  cannot count as missing; known missing retries need not become idle. Observer
  history follows the active-tree/wrap lifecycle. Existing read/mesh loops,
  selection priorities, retry intervals and budgets retain their original
  operations. Ordinary callers retain no observer allocation or extra traversal

Only the existing `main.rs` digest advances. The other **126 previous pins** are
unchanged. The complete `screen_capture.rs` and `terrain_selection.rs`, including
their regression witnesses, join the exact required set: **129 whole-file pins**
plus **102 unchanged independent anchors**. `reviewed_source` names the reviewed
runtime commit above; hashes cover raw canonical files and exact checkout bytes.

Python AST comparison confirms that all **36 checker function bodies**, all
**86 pre-existing test/helper methods** and all existing module-level test
functions are unchanged. Every pre-existing checker constant except the expanded
required path set is equal to the baseline, including historical hashes, model
identities, acceptance commands and the **180-second capture watchdog**. No
physical profile, codec, independent fixture, rights or release gate changes.

## Checks and limits

The six focused Python suites passed **193 tests**: candidate 79, stager 21,
readiness 38, authorization 43, release workflow 8 and source-CI workflow 4.

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
git diff --check
```

Two added tests exercise 12 committed semantic mutations of scheduling,
stability, discovery, model/runway/overlay guards and the unchanged-selection
witness, plus 15 exported source omission/rehash cases. Existing per-path drift,
exact canonical/checkout equality, independent encoders and acceptance failures
remain required. No golden bytes were regenerated.

This migration ran no Cargo, native/GPU or Windows process and published
nothing. It strengthens source binding; CPU readiness is not GPU completion or
renderer qualification. The earlier-source Windows engineering result cannot
qualify these runtime bytes. Exact-source native/Windows capture and visual
inspection remain the integration owner's separate checks.
