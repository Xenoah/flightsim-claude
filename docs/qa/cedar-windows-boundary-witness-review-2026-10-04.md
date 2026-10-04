# Cedar cross-platform boundary witness: independent review and source binding

Date: 2026-10-04 UTC. Reviewed correction:
`51e03b7f16ab769b493617d4284fb9b641c6bb96`, directly on
`8dd34f77ea13f799dd9c268fcbbbe4f69307e494`.

## Review decision

The correction is accepted as a test/qualification portability correction.
Windows requalification is still required. The observed failure is recorded in
[Windows core run 37234646790, job 111531224096](https://github.com/Xenoah/flightsim-claude/actions/runs/37234646790/job/111531224096):
130 of 131 FDM unit tests passed. The original exact fixture/component inventory,
K2/substep 2 and negative-domain assertions succeeded before the Linux-derived
J-word assertion failed. Windows produced `bed475ed05f3e017`; the historical
Linux GNU value is `bed475ed0aff54c1`.

The new helper independently reconstructs two RK4 endpoints and third-substep
K2 using public, unchanged law-1 derivative queries. Its explicit state updates
use the ADR-0018 held governor sample, analytic turbine response and normalized
quaternion, with six subdivisions of the same 1/120-second attempt. It evaluates
both accepted endpoints and passes the caller's exact environment throughout.
The bare-FDM moving ground and host held ground plane remain distinct.

It never calls the production step/private integrator, nor constructs its
reference from law-2 results. It separately computes signed J from reconstructed
body-relative velocity, absolute spin and diameter. It then requires the exact
production J word, negative finite J, domain reason and all nine optional
force-diagnostic words. This is an independent integration/diagnostic witness
using shared force laws, not an independent validation of those force laws.
All 16 rollback words, exact component inputs and physical identity remain
checked. Law 2's identical-input acceptance is strengthened to exactly six
substeps. Historical J/geodetic output words remain additional x86_64 Linux GNU
regressions, with exact same-runtime comparisons on every platform.

A separate disposable reviewer probe accepts the authentic witness and rejects
13 false witnesses: changes to each of the nine diagnostics (including a
one-bit J change), stage, substep, reason and ground elevation. These fail before
the historical Linux-only assertion. The probe is review evidence, not a new
production source or fixture. Its first compilation exposed an unused import
in the temporary harness; the corrected harness passes with warnings denied.

The complete base-to-correction blob audit finds exactly three existing Rust
test/qualification changes, four documentation changes and the new test helper;
2,299 prior regular blobs are unchanged. No production runtime, fixture, profile,
codec, physical bound, Cargo file, app or UI change is included.

## Minimal strict source binding

The binding grows from 114 to **115 whole-file sources** by adding
`crates/flightsim-fdm/tests/support/cedar_boundary_witness.rs` to the fixed
`NEAR_STATIC_FOUNDATION_PATHS` set and reviewed manifest. Exactly three existing
pins advance: the near-static FDM tests, Cedar qualification support and v6
replay tests. All 111 other source hashes and all **102 independent anchors**
are unchanged. The helper belongs in reviewed test source, not in the frozen
independent fixture/reference inventory.

The manifest identifies reviewed source `51e03b7` and has SHA-256
`c6e940e881760daa8baee2d6b1b6555c8a7517db2bbf6caf2e442c6a6e5b6fcf`.
All 36 checker functions, historical hashes, frozen FDM-root guard, rights and
release gates, app admission, build flags and all existing Python tests remain
unchanged. The existing dynamic checks automatically cover the helper: committed
source drift rejects across all 217 sources/anchors, and omission/record removal/
digest change/contract-row removal reject across all 149 near-static sources and
anchors (596 exported-evidence attacks). No normalization, tolerance, reduced
source inventory, or failure-evidence exception is introduced.

## Validation and limits

Independent local Linux GNU verification uses Rust 1.93.0, the existing shared
target, offline dependencies, at most two Cargo jobs and warnings denied:

- Original FDM Cedar boundary test passes
- Reviewer sensitivity probe passes, rejecting all 13 deliberate mismatches
- Three Cedar replay tests and the qualification boundary test pass
- All 183 existing Python candidate/stager/readiness/authorization/workflow
  tests pass, including all 69 candidate tests
- Formatting, architecture and diff checks pass

The correction author's supplied logs additionally record all 236 FDM tests,
3 Cedar qualification tests, 28 v6 replay tests, 14 profile tests, 11 codec tests,
8 identity tests and strict FDM/sim all-target Clippy passing. Those complete
Rust suites are supplied evidence; the reviewer reran only the focused cases
above. No Windows execution, native rendering acceptance, new app admission,
release authorization or external publication is claimed by this review.
