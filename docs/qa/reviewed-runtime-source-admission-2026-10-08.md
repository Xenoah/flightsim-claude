# Reviewed runtime repairs: source admission boundary

This migration is separate from runtime source commit
`bda51fd222a339dbbf6bee52097b7a02e840798f`, whose tree is
`15160986689094a4395ca09d74d8c1b66358e5cd` and sole parent is
`fe7373e96396131ef4a1c2a9af9cfabdb8aab82b`. The runtime commit combines the
reviewed camera repair, bounded Filmic source-notice decision, two runtime-facts
files, and one accurate future-report sentence. The original component evidence
and historical native inventories remain unchanged.

The runtime commit deliberately leaves source admission closed: the ordinary
candidate and analytical entry points both reject the changed `main.rs` against
the preceding contract. The pre-migration candidate test run consequently had
68 passes, 5 failures and 29 errors, all caused by that unchanged `main.rs` pin.
These failed results are not reclassified as successful qualification.

## Exact boundary additions

The replay boundary grows from 402 to 404 whole-file pins:

- `crates/flightsim-input/src/camera.rs`: the new production helper transports
  existing smoothing history across both origin translation and tangent-basis
  rotation. Pinning only the app call would leave the helper outside admission.
- `crates/flightsim-app/src/runtime_tests.rs`: preserve the actual app witnesses
  for continuous rebases, a basis-only change, explicit reset, view changes,
  and seek precedence. This follows the existing controls, jet, weather and
  turboprop runtime-test conventions; it creates no independent golden anchor.

The analytical boundary grows from 22 to 24 whole-file pins:

- `scripts/collect-analytical-runtime-facts.py`: fresh native evidence relies on
  the bounded collector's toolchain, installed-linker and final-output discovery.
- `scripts/tests/test_analytical_runtime_facts.py`: preserve its exact-output,
  ambiguous-command, installed-origin, private/public and closed-schema witnesses,
  as the existing boundary already preserves analytical recipe/build tests.

Previously these four files were present in complete tracked-input inventories,
but were not frozen source pins. This is an explicit reviewed boundary expansion,
not a claim that the old pin sets already protected them. No path is removed.

## Affected bindings only

Both replay and analytical `reviewed_source` pointers identify the actual
remote runtime commit above. Of the 402 existing replay hashes, only `main.rs`
changes; the remaining 401 stay exact. The analytical contract changes only its
existing `main.rs`, asset manifest, ordinary checker, analytical checker,
analytical checker test, and replay-contract digests, besides the two additions.
The other 16 existing analytical hashes remain unchanged.

The capture runner's two inherited contract digests then bind the resulting
replay and analytical contracts. The capture contract updates those inherited
digests and the runner's complete-file digest. Its three other source pins,
four-path boundary, and historical `75753f5...` capture identity guard remain
unchanged. No partial-function hash or source normalization is introduced.

All 102 independent replay anchors, historical legacy/FDM hashes, original
fixture bytes, vendored-source boundaries, build recipes, feature selection,
workflow permissions, extraction checks, runtime checks and release gates are
retained. The migration changes no runtime Rust implementation, source evidence
packet, executable or LUT payload.

## Adversarial admission checks

The added camera admission test first accepts an unchanged committed fixture,
then commits separate mutations which erase transported smoothing state or
weaken the app's world-space tolerance. Each must fail the real ordinary source
entry point and identify the changed helper or witness path.

The added analytical admission test likewise accepts its committed baseline,
then separately commits a bypass of the native-Windows requirement and disables
the exact-output matching witness. Each must fail the actual analytical source
entry point on that precise path; restoring original bytes must restore
admission. Existing whole-boundary, independent-golden and export-tampering
adversaries remain in place.

Commit-specific validation must run both new adversaries, the complete ordinary
candidate and analytical recipe/capture suites, and all three source entry
points on a clean committed checkout. Passing source admission does not qualify
Windows execution, visual appearance, hardware, dependency/platform rights,
final bundle contents or publication. Fresh evidence must bind the eventual
integrated source; old evidence is not relabeled for it.
