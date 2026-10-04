# Swift candidate binding after the full-state turboprop host/replay foundation

Reviewed source: `28cd909f576ce0a11e4a00a3beaaed67788e19b5`, tree
`512d22e2634b6856934eaabe1ac126dfad6fb019`. Baseline: the accepted foundation
binding `9b7caf1`, tree `0ed7c1e0f1486f913ec4c916c21bd36d4f8aeec0`.
The [previous binding review](replay-candidate-turboprop-pin-review-2026-10-04.md)
retains the 47-file boundary, 13 independent anchors and five historical hashes.
Its manifest SHA-256 is
`ccabbe642cb6d02de8c7223a426b6cba84dc74cb2a514fb3b6cdbc3606053532`.

This review owns only the candidate manifest, one new Python test containing
four deliberate source mutations, and this record. It changes no runtime,
checker implementation, payload, dependencies, rights policy or workflow.
No Cargo, native app, GPU, Windows candidate, publication or distribution
operation was performed by this reviewer.

## Exact accepted input and integration delta

The independent pure-host/replay review accepted
`47915f46cf0b9ec3b1427aa3ca5f97f0daf25602`; its runtime/test source is
`e416fb6b6d42aeffe4021c4e167ab75b002e3239`. The final accepted follow-up changes
only QA wording. Integration consumes the three-commit implementation/review
sequence as `ebbf86e`, `374bbde`, and `d9c26ba`, then records acceptance in
`91f4b26`; `28cd909` aligns the ADR status with that accepted scope.

All **32 paths** changed by that accepted sequence were compared using complete
committed blobs. **29 match exactly**, including every production module, Rust
test, fixture and independent encoder. The three differences are the QA report,
replay guide and ADR's reviewed acceptance wording; none changes code or wire
allocations. The eight file hashes in the independent final-acceptance
receipt match the accepted source, and each non-documentation input matches the
integrated source. The separate prior Cedar/model-comment integration is already
covered by the previous foundation review; it is not a new replay change.

The complete baseline-to-integration delta contains **34 paths**, including 31
new paths. Only three pre-existing paths change: `ARCHITECTURE.md`, the profile-v3
guide and the sim module root. Removing precisely `pub mod replay_v5;` and
`pub mod turboprop_simulation;` from the new raw sim root recovers its complete
prior bytes. All other pre-existing tracked files are byte-identical. This
includes all old FDM/host implementations, replay v1-v4 codecs/tests, app/input/
UI/audio/renderer paths, Cargo manifests and lockfile, commercial stager,
rights/release inputs and workflow definitions. No dependency, feature or
existing arithmetic change is concealed by the new exports.

## Actual default and replay reachability

`SelectedAircraftProfile` still contains only Legacy and Jet. Its bounded
version probe dispatches profile 1 to the existing legacy decoder and profile 2
to the existing jet decoder; profile 3 reaches the unsupported-version error.
Built-ins remain Legacy. Ordinary startup selects Light Single; the unchanged
commercial-staging default and payload remain Swift Sport only.

App startup branches on the existing `is_jet()` selection. Both its legacy
`resolve_flight_sources` and jet `resolve_jet_sources` call the unchanged
`replay_v4::ModelReplayFile::read_from`. That enum contains only Existing and V4.
It explicitly admits version 4 to the jet reader and versions 1-3 to the old
reader; version 5 returns UnsupportedVersion. `FlightSession` still contains
only Legacy, JetLive and JetReplay. No app or existing headless path calls the
new `replay_v5` or `turboprop_simulation` modules.

`ModelIdentity::supported()` remains the algorithm-1/schema-2/kind-2/law-1 jet
gate. Both v4 admission sites still use it. The separately named schema-3
identity and full-state v5 codec do not widen that predicate or the old format
dispatchers. Old complete/partial identity, explicit legacy opt-in, persistent
missing-yaw notice, rewind and wind/visual-clock ownership remain unchanged.
Cedar still has no authored runtime profile, picker entry or turboprop app
session. Generic model override availability does not qualify a new aircraft.

## Receipt and independent byte verification

All eight raw receipt hashes match the prior independent review: pure-final,
Clippy, rustdoc, formatting, architecture, and Python v3/v4/v5. The pure log was
independently recounted: **1,220 passing tests/doctests in 91 summaries, zero
failed and zero ignored**. It contains the reviewed provenance, visual-time,
exact-stage rollback and prospective-weather regressions. The sim suite includes
66 unit tests (three v5 cases), five v5 codec tests and 19 v5 numerical tests.
These are the accepted author's executed receipts, not new runs on this binding
commit. Empty formatting output is supported by that explicit success record;
it is not independently treated as an exit-status proof.

All **21 existing `.fsreplay` fixtures** remain byte-identical to the accepted
foundation. All **21 new v5 witnesses** match the independent Python encoder,
which was executed without its write option. The 6,398-byte ordinary witness
contains 121 effective-control entries and distinct checkpoint states at 120
and 121; it is structural byte evidence, not an authored physical trajectory.
The 49,100,903-byte maximum was independently recomputed. Existing v1-v3/v4
reference commands and the legacy/complete identity reference pass unchanged;
no golden or oracle was regenerated.

The scoped runtime acceptance covers complete state/provenance, authentic prior
recording prefixes, rollback, exact terminal reproduction and reconstruction.
It does not claim separate per-engine-scalar terminal-only mutations: the
terminal-only continuity regression changes shaft speed, while the successful
report tests independently exercise all three engine scalars. Nor does it claim
an added Ground-only hostile stage test; that stage/mask correction was reviewed
directly in the accepted predicate. These limitations are retained from the
independent review rather than inflated into new coverage.

## Exact pin change and rejection witnesses

The required set remains exactly **47** files. Only the complete raw sim-root
SHA-256 changes:

- Previous: `720e183b52120966c4f75ec59058b3b3ea6e9024413bc2b240423a7e4d8e501b`
- Reviewed: `11b56a2b89154643b3c0c5fe9d20f580dc7187ab02e2b868f9ddbda122eda69c`

The other 46 digests, all 13 independent anchors, all five historical hashes,
legacy baseline and partial fingerprint remain fixed. The entire checker is
byte-identical, including the strict FDM-root guard, canonical/checkout checks,
source-evidence validation, supported MSVC commands, default features, Swift-only
identity/payload checks, literal false release authorization, 180-second capture
and default-disabled diagnostic. Whole-file reconstruction above is review
analysis only; no projection or normalization replaces raw hashing.

Before the pin update, the existing exact-source test failed on precisely the
new sim-root digest. After review, the added test first admits the unchanged
fixture and then independently commits four source mutations in a disposable
repository: remove either new export, route version 5 into the v4 jet decoder,
or route version 5 into the existing v3 decoder. Each must fail canonical
source collection with the changed path before building. These witnesses do
not execute modified Rust or add app support. No new checker guard is needed.

All **176 existing test functions** are byte-preserved, including their complete
assertions. All **177 Python tests pass**: candidate 63, stager 21, readiness 38,
authorization 43, release workflow eight and source-CI workflow four. Existing
per-file drift, semantic mutation, CRLF, byte provenance, release policy and
failure-evidence checks remain active. The new manifest SHA-256 is
`2385130bf3735efc0c43c007fd9f1388f26a648cca2cf2381a5457645a7150ec`.
`git diff --check` passes.

## Complete source inventory and limits

At the clean reviewed source `28cd909`, all **2,207** regular tracked Git blobs,
**46,218,774** raw bytes, were compared byte-for-byte with checkout files. Every
Git blob ID was independently recomputed. The sorted inventory is encoded as
compact JSON with sorted keys and a final LF; fields are path, mode, Git blob
ID, byte count and raw SHA-256. Its SHA-256 is
`dbc5e2117ffe431cbc831d7e993d78bc6f29ed7b101dc9f13dac7e4555f90435`.
It includes all new unexposed runtime modules and tests, beyond the retained
Swift boundary and frozen independent anchors. The final clean binding commit
receives a separate complete inventory and checker source-input/export validation;
this earlier inventory is not relabeled as that later commit.

The initial ADR status mismatch was reported and corrected by the integration
owner in reviewed `28cd909`; its two wording changes alter no allocation or
accepted runtime byte. App dispatch, an authored flight preset,
new native handling/presentation, Windows full-scene capture and dependency/
rights authorization remain separate gates. No source-binding blocker remains;
this milestone grants no binary release or aircraft-performance approval.

Executed commands:

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
python3 docs/qa/replay_v5_reference.py
python3 docs/qa/replay_v4_reference.py
python3 docs/qa/replay_v3_reference.py
python3 docs/qa/replay_identity_reference.py
git diff --check
```
