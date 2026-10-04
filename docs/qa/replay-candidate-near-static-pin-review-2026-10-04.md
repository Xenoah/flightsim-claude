# Strict candidate binding for the combined near-static pure foundation

Date: 2026-10-04 UTC. Reviewed pure integration:
`93bad37f27403ee566a59290bc25fe6dc4bec48e`. Compatibility baseline:
`7f5663c064d31b0361530777a058e763ac7f0e85`.

This review joins the separately accepted law 2, profile/identity 4, host/replay 6
and Cedar numerical qualification under one source boundary. It changes only
candidate source pins, additive rejection tests and this report. No production
Rust, old fixture, app selection, commercial payload or rights policy changes.
It does not accept the separately developed near-static app integration.

## Exact integration and retained behavior

The complete base-to-reviewed tree comparison contains 2,223 baseline regular
files: 2,217 are unchanged, six have precisely reviewed edits, 82 are added and
none are deleted. The six existing-file changes are:

- `ARCHITECTURE.md`: additive descriptions of the new explicit pure APIs
- FDM `Cargo.toml`: test-only serde_json `raw_value`, without changing old float
  decoding features, runtime dependencies or Cargo.lock
- FDM `turboprop/mod.rs`: the documented `near_static` module export
- Sim `lib.rs`: four explicit profile, identity, host and replay module exports
- Profile-v3 `mod.rs` and `wire.rs`: only crate-private visibility for shared DTOs

Removing exactly the additive exports and restoring only the DTO visibility
recovers the complete old production bytes. The original law-1 configuration,
FDM runtime, physical identity, full-state host, replay-v5 codec and existing
profile decoders are unchanged. The existing public profile-v3 types and their
accessor meaning remain intact; visibility changes create no public conversion
or old-format admission. All 51 prior fixture/golden files, including all 42
old `.fsreplay` witnesses, retain their exact bytes.

All app, picker, renderer, input, UI, audio and source-selection files remain
byte-identical to the baseline. Development selection still dispatches profile
1/2/3 only to Legacy/Jet/Turboprop, with profile 3 selecting law 1 and replay 5.
Profile 4 reaches its existing unsupported-version error. No existing app
session or old reader admits replay 6. The new pure APIs are not app admission.
The commercial default, payload and candidate evidence remain Swift Sport only;
`release_authorized` remains literal false. Existing native presentation and
readback source identity are retained, not newly qualified by this pure run.

The upstream independent acceptance sources are:

- Law 2: `d18d953dcaa9775783b0a4adb16a540698f09e5a`
- Profile/identity 4: `6fa7aee254a43943928e4c26f16c99e64eecaaef`
- Host/replay 6: `5c547798d61376c79ff96ef48bb48e54a7806606`
- Cedar qualification: `b9c8f143ebf2c0d317e4003cb565044e578082b3`, followed
  by the exact per-case parity assertion strengthening integrated at `93bad37`

Accepted additions were compared against complete committed upstream blobs.
Historical checkpoint documents saying “review pending” retain their original
meaning; the independent receipts above establish their subsequent scoped
acceptance. The profile reference's previously reported integer-spelled `-0`
limitation remains a fixture-oracle limitation, not a production-parser change.
No reference or golden was regenerated to satisfy a test.

## Additive strict boundary

The required source set grows from 55 to **114 whole files**. Every former path
remains. The 59 additions bind all FDM source, retained original-token/profile
and forward-identity dependencies, old full-state host/replay inputs, the new
law/profile/identity/host/codec modules and their qualification/schema witnesses.
The explicit set is `NEAR_STATIC_FOUNDATION_PATHS` in the candidate checker.
This is a fixed reviewed set, not a filesystem glob or normalized projection.

Only one old pin changes, the reviewed sim module root:

- Previous: `11b56a2b89154643b3c0c5fe9d20f580dc7187ab02e2b868f9ddbda122eda69c`
- Reviewed: `343bac408d23a0acc0ddfda8670687bf4e4e221bc24a8a757e97662ecfd5025c`

The other 54 original pins, strict FDM-root guard, five historical hashes,
legacy baseline and partial fingerprint are unchanged. The 13 original
independent anchors remain exact, with 89 additions for **102 anchors** total.
The additions pin all old/new replay fixtures, complete physical-identity
references and golden inputs, and original Cedar component/boundary inputs.
Independent anchors stay separate from the moving source manifest.

The manifest SHA-256 is
`28b3283d330e10d67fbadee8ee3d6cb94734f08be905484254b2149d03afd41b`.
All 36 existing checker functions are byte-preserved. Source and exported-evidence
validation still require exact canonical Git and checkout hashes; no newline
normalization, relaxed path boundary or failure-evidence exception was added.
MSVC toolchain, default features, offline/Swift-only identity, literal false
release authorization, 180-second capture, optional diagnostic and all rights/
release/admission rules retain their exact prior implementations.

## Rejection and independent-reference evidence

All **180 existing Python test functions** in the six candidate/release suites
retain their complete original source and assertions. Three additive tests pass:

- Fourteen independently committed source mutations reject before compilation:
  remove the new export, widen either 0.10 flow bound, change a negative knot,
  lose signed original-token parsing, skip fixed domain commitments, replace
  full retained identity or origin bytes, alter prospective time, use old model
  support, bypass report provenance, weaken terminal diagnostics, route profile
  4 through the old loader, or weaken the exact Cedar matrix parity assertion
- All retained physical-identity/profile and replay-v4/v5/v6 independent Python
  references execute in verification mode; existing v1-v3 references remain
  covered by the unchanged test
- Every one of the 148 added sources/anchors rejects four exported-evidence
  attacks: omitted file, omitted reviewed record, changed canonical digest and
  removed contract/anchor row, including recomputed outer hashes: **592 cases**

The unchanged committed-drift test now checks every one of the 216 source paths
and independent anchors. Existing clean-CRLF checkout rejection, raw-notice
preservation, frozen historical hashes, supported build flags, all app/replay
admission mutations and synthetic success/failure export checks still pass.
All **183 candidate/stager/readiness/authorization/workflow tests** pass, plus
all **five profile schema suites** and the negative-row rebuild check.
These Python evidence fixtures are synthetic validator tests, not executed
Windows candidates or actual rendering acceptance.

Both preserved Cedar compressed archives match their committed compressed and
decompressed hashes and byte counts. Recomputing the compact summary in memory
matches its complete committed JSON. The 21 original matrix cases retain 76,080
exact law-1 parity steps and no negative-stage records. The four requested
120/240/480/960 Hz critical runs and reported braked-idle creep remain bounded
numerical evidence. Their effective internal resolutions and original limits
are unchanged. Stationary parking remains false; no ground-force changes follow.

## Combined execution and limits

The complete combined core/FDM/world/sim/tilegen run passes **1,260 tests and
doctests in 89 result groups**, zero failures and zero ignored. This includes
14 profile-v4 tests, eight complete-identity-v4 tests, 11 replay-v6 codec tests,
28 replay-v6 host/runtime tests and all three Cedar qualification tests, plus
the new FDM and private identity unit tests in their complete package suites.
The retained old tests execute alongside them on the same source/configuration.

The prior replay-v6 author's 1,298 tests/95 groups also included `flightsim-content`
and `flightsim-net`. Direct log comparison retains all 88 common groups and
exactly 1,257 passes. Those two additional packages supplied seven groups and
41 passes: content packages 23, net unit two, local sessions nine and protocol
traffic seven, plus three empty unit/doc groups. This run adds the Cedar group
with three tests. Thus 1,298 - 41 + 3 = 1,260, and 95 - 7 + 1 = 89. No common
target is missing or reduced; content/net were not newly rerun. Counts are not
summed across overlapping runs.

Strict FDM/sim all-target Clippy, full-workspace formatting, architecture and
whitespace checks all exit 0. The exact commands are:

```sh
cargo test --offline --locked -j2 -p flightsim-core -p flightsim-fdm \
  -p flightsim-world -p flightsim-sim -p flightsim-tilegen
cargo clippy --offline --locked -j2 -p flightsim-fdm -p flightsim-sim \
  --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/check-architecture.sh
git diff --check
```

The separate compatibility reviewer also verified 59 immutable prior QA
artifacts by hash/length and matched the full new source to accepted upstream
commits. Its acceptance receipt SHA-256 is
`ff64083fb52be9b2fa23574f7a12ee06d2c42b611472d5d78c9b514569cd7fe6`.
Those prior executions remain distinct from the newly executed 1,260-test run.

The shared prescribed target is used with the existing 1.93.0 toolchain,
`RUSTFLAGS=-D warnings`, debug information/incremental disabled and `-j2`.
Relevant package source roots are touched before Cargo because cross-worktree
mtime reuse can otherwise select stale shared artifacts. The exact pure source
inventory and SHA-256 fingerprints of all 84 executed test binaries are recorded
with the test receipt.
No second Cargo cache, native/GPU process, Windows run or publication is used.

The final clean binding commit is additionally checked through the unchanged
`source_inputs` collector and synthetic export validator, with a complete
canonical/checkout inventory in the external QA receipt. A final commit receipt
is distinct from the reviewed runtime base and does not relabel prior native
or independent tests as new runs.

Acceptance is confined to combined pure compatibility, qualification and strict
source/evidence binding. The finite-domain authored approximation is not measured
reverse-flow data, a new flight preset, stationary parking, cross-platform libm
trajectory identity, native app acceptance or distribution permission.
