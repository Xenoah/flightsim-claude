# Swift candidate binding after the pure turboprop foundation

Reviewed implementation: `e0a33318507dc4e3a8331617ab1a3ba030d24a3e`, tree
`bc5c150a542cc4657755d95f46c765d9b13c1482`, against local baseline
`b38f41e94b6231037c54c538a5b99c33b19ca7dc` (public main `0713227`). The
baseline retains the accepted [wind binding](replay-candidate-wind-pin-review-2026-10-04.md):
47 whole-file pins, 13 independent anchors, reviewed source
`bf79f846bdfd8da768306415082960a85616b329`, and manifest SHA-256
`4b50103a75a87de1142126a2e69b48fa343c4dede5d7f6a346769d7b89b7910e`.
The entire 51-path implementation delta was inspected before changing a digest.

The later `49092fa46c3bf16b5fcc833d8c0c417b3c4ca3c2` adds only seven lines of
combined-check evidence to the [integration report](turboprop-foundation-integration-2026-10-04.md).
It is included in this binding branch and final source inventory. It changes no
production file or pin. This review owns only the candidate manifest, its strict
FDM-root guard, two added Python tests and this record. It changes no runtime,
payload membership, dependency/rights review, workflow or release authorization.

## Actual reachability and unchanged defaults

- The FDM root adds exactly `pub mod turboprop;`. Removing that line recovers the
  complete prior raw file, including legacy revision 2, defaults, arithmetic,
  integration, limits and tests. The sim root adds only `aircraft_profile_v3`
  and `turboprop_identity`; removing those two declarations recovers its complete
  prior raw file. Its existing simulation and recorder/player exports remain
- Core adds five named SI unit wrappers and their exports. Removing precisely
  those declarations and replacing the reviewed export list recovers both entire
  prior files; existing unit definitions, macro behavior and conversions are
  exact. The FDM manifest adds only the separate benchmark target. Dependencies,
  features, default commands and all lockfiles are unchanged
- The four existing profile-v2 files only widen specified helper/DTO visibility
  to `pub(crate)` and generalize one error-type comment. Applying those explicit
  substitutions to each prior raw blob reconstructs the complete current file.
  Exact-number parsing, depth/resource bounds, metadata validation, physical
  conversion and original tests are unchanged. No serde feature was added
- App `SelectedAircraftProfile` and `FlightSession` still contain only Legacy
  and Jet. The unchanged version probe routes only 1 and 2; version 3 reaches
  the unsupported-version error. Built-ins remain Legacy, the ordinary default
  remains Light and commercial-staging remains Swift. The picker, session
  transaction, controls, clocks, wind/turbulence, audio and recording paths are
  byte-identical. The new modules have no caller in those existing app paths
- `ModelIdentity::supported()` and the complete jet encoder are unchanged. New
  methods `for_turboprop` and `supported_turboprop` are separately named. Both
  replay-v4 admission sites still call the original schema-2/kind-2/law-1 gate.
  Legacy complete/partial identity, opt-in, missing-yaw notice, old physics,
  replay v1-v4 byte layouts and all 22 pre-existing sim fixtures remain exact
- Renderer `ModelFit` changes exactly two comments. Explicit reconstruction from
  the prior blob verifies unchanged -Z forward, +Y up, 8.3 m default length,
  rotation, scale and zero translation. Cedar's separate metadata specifies +Z
  forward/+Y up and 9.6 m; no old model is silently reoriented
- Cedar has no authored runtime profile, built-in/picker entry or turboprop
  dynamics route. The pre-existing generic `--model` override can name a developer
  GLB, which is a visual override using the selected existing dynamics. This
  source review neither runs nor qualifies Cedar through that generic route.
  The unchanged Swift candidate stager does not copy the experimental asset

In the reviewed implementation delta, all previously tracked files outside these
ten implementation files and the architecture note are byte-identical to the
baseline. Whole-file reconstruction is review evidence only: the candidate
continues to hash complete raw files,
without removing comments, selecting functions or normalizing newlines.

## Accepted inputs and receipt verification

All 15 paths in the accepted FDM delta at
`7fdf76ad138431050a1daf9136ae0412754dd3dd` match exactly. The accepted profile
source `07febca75a6c90f0aab56bfb9bf8b0405008cb70` has 34 paths changed from the
baseline; 33 match exactly, including every production source, test, schema,
fixture and QA report. Its only further edit is two passages in the profile
guide marking the already reviewed allocation accepted and fixed. No version
number, component meaning or canonical byte order changes. All 14 asset paths
from accepted `60785f7f0e7e6c8bb7cb4cd02fe1311655ffa185` match exactly.

The raw FDM regression log independently recounts 1,175 passing tests/doctests
in 87 result blocks, including 27 turboprop cases. The raw profile logs recount
18 focused tests and 1,152 passing tests/doctests in 82 broad result blocks.
All have zero failures/ignores; the broad totals overlap and are not summed.
The five FDM log hashes, seven profile log hashes and four combined integration
log hashes match their reviewed receipts. The combined log records 36 passing
assetgen tests and completion of workspace/all-target Clippy with both optional
download features. Empty fmt output is supported by the owner's success receipt,
not treated by itself as an exit-code proof. This review runs no Cargo command.

The Cedar validator was rerun and its parsed output equals the entire committed
asset-validation receipt. All eight provenance entries match exact sizes/hashes;
the rebuild source/generator, Blender audit and old/new clearance-revision hashes
also resolve to the recorded blobs. The GLB remains 97,404 bytes, 2,422 vertices,
4,380 triangles, eight materials and no external resources. Nominal 0.40 m
level/uncompressed disk clearance remains a visual design claim. Blender rebuild,
reimport and preview inspection belong to the accepted asset review and were
not rerun here; no native rendering or dynamic clearance claim is inferred.

Independent Python checks also reproduce the 1,379-byte schema-3 golden and
`a302a97f8e778c26`, the unchanged 1,095-byte jet golden and `9493dfe3f9ba6f76`,
the 120-case v3 schema corpus, old v1/v2 schema tests, and all 1,156 numerical
fixture samples. The minimum sampled power margin is `0.03921045903391185`.
No oracle or golden is regenerated; sampling is not a continuous-bound or
governor-stability certificate. The accepted Rust receipts own loader execution,
152 physical-scalar mutations and actual schema-3 rejection by the v4 reader.

## Exact pin changes and independent rejection witnesses

The required set remains exactly **47** files. Only two digests advance:

| Complete file | Previous SHA-256 | Reviewed SHA-256 |
| --- | --- | --- |
| `crates/flightsim-fdm/src/lib.rs` | `4d51cf4b5d62ce0f6bcc031df445225ac07c2bad0ed590c4b422ad6cca579462` | `2328631f895921e4152de6f8107c4d6f07ba764b6f6225189a4b826c6f9f013c` |
| `crates/flightsim-sim/src/lib.rs` | `09aab74c1d1a936e7c91ea3596117cb42fc68ac17a66f83f8157c5daffe5815e` | `720e183b52120966c4f75ec59058b3b3ea6e9024413bc2b240423a7e4d8e501b` |

The other 45 digests, all 13 independent encoder/golden anchors, all five
historical hashes, historical baseline and partial fingerprint remain unchanged.
The strict literal `REVIEWED_ADDITIVE_FDM_LIB_SHA256` guard advances to the same
reviewed complete FDM-root digest; it is not weakened or replaced with dynamic
self-acceptance. AST comparison confirms this literal is the checker's only
executable change. Its path set, functions, policy, commands, default features,
Windows/MSVC target, Swift-only payload identity, literal false authorization,
180-second full-scene capture and default-disabled diagnostic remain unchanged.

All 174 existing Python test functions are byte-preserved. One added test commits
six deliberate mutations in disposable repositories: removing any of the three
new exports, routing profile 3 into the app's v2 decoder, or broadening either
replay-v4 identity gate to accept the turboprop tuple. Each must fail canonical
source collection before building. The second test rejects replacement of the
new strict FDM guard with its previously accepted jet-only digest. These are
source-admission witnesses, not execution of modified Rust or new app support.

## Full source inventory and executed checks

The clean reviewed `e0a3331` checkout contains **2,175** regular tracked Git blobs,
**46,038,432** raw bytes. Every file was compared byte-for-byte with its canonical
blob, and each blob's Git object ID was independently recomputed. The sorted raw
inventory SHA-256 is
`e04d1c7865d7fa4c85fba4f736a7773cf29d8a02bb210c4493a213d94102f5e2`.
This includes the new unexposed implementations/assets and all policy/dependency
inputs, beyond the 47 candidate pins and 13 independent anchors. The final clean
binding commit gets its own exact HEAD/tree source collection and exported-source
validation; historical inventories are not relabeled as that later commit.

All **176 Python tests pass**: candidate 62, stager 21, readiness 38,
authorization 43, release workflow 8 and source-CI workflow 4. Existing CRLF,
canonical/checkout, per-file drift, old semantic mutation and source/evidence
checks remain active. The reviewed manifest SHA-256 is
`ccabbe642cb6d02de8c7223a426b6cba84dc74cb2a514fb3b6cdbc3606053532`.
`git diff --check` passes.

```sh
python3 -m unittest scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
python3 tools/blender/validate_cedar_turboprop.py
python3 docs/qa/turboprop_identity_reference.py
python3 docs/qa/jet_identity_reference.py
python3 docs/qa/turboprop_profile_reference.py
python3 schemas/tests/test_aircraft_profile_v3_schema.py
python3 schemas/tests/test_aircraft_profile_v2_schema.py
python3 schemas/tests/test_aircraft_profile_schema.py
git diff --check
```

No Cargo, native/Bevy/GPU process, Windows candidate or diagnostic run, workflow
trigger, distribution or publication is performed here. No binding-review
blocker remains. Complete host state/replay ownership, an authored flight preset,
native handling/presentation, Windows full-scene capture, and dependency/rights
authorization retain their separate gates. This source milestone does not
authorize a binary release or qualify a new app aircraft.
