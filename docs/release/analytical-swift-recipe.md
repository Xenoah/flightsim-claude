# Prepared analytical Swift build-evidence recipe

Status: **prepared, build/native/bundle qualification unexecuted**. This additive
recipe has no Windows workflow and supplies no usable binary. It does not repair
or relabel the ordinary candidate's failed PNG capture. No engine/dependency,
runtime, release/authorization, stager, readiness or acceptance-test change is
part of this preparation.

The separate identity is
`swift-only-analytical-windows-build-evidence-v1`. It selects exactly one app
root, Rust 1.93.0, `x86_64-pc-windows-msvc`, release, locked/offline, jobs 2,
`RUSTFLAGS=-D warnings`, `CARGO_INCREMENTAL=0`, `--no-default-features` and
`--features analytic-tonemapping,commercial-staging`. The root and selected
libraries must report release opt-level 3, debug info 0, assertions/overflow
checks/test false. Its ordinary positive control selects a separate target tree
with the ordinary app defaults and no commercial feature. That control proves
sensitivity to all three known LUT payloads; it is not the ordinary commercial
candidate, whose existing command and gates remain unchanged.

`python scripts/check-analytical-swift-recipe.py --describe` prints the exact
commands and unexecuted gates. This only describes preparation. A future
qualified operator/runner must record real outputs and statuses while executing
the commands; this change intentionally does not automate those executions.
The metadata/tree commands are safe locked/offline reads; the `build` command
is specified here but was not executed for this combined recipe.

The existing minimal `check-tonemapping-build.py` CLI and omitted Python recipe
argument still reject `commercial-staging`. Only the separately named wrapper
selects its exact optional recipe argument. Neither ordinary constants nor
existing negative fixtures are widened. The six-library payload/fingerprint/
dep-info implementation is shared, rather than bypassed or monkey-patched.

## Future private capture contract

Create separate `analytic` and `ordinary` directories under a private capture
root, plus disjoint fresh Cargo target directories. Use the same exact clean
LF source checkout. Never reuse or write either target tree after freezing it.
Never substitute another executable or an earlier minimal analytical artifact.
The auditor itself executes only Git reads, `rustc -vV`, locked/offline Cargo
metadata/tree reads and temporary notice recollection. It never builds, runs an
app, stages, invokes Windows acceptance, creates approval or uploads anything.
Run it on the same native Windows MSVC machine and source/target paths as capture.
Unexpected compiler wrappers/overrides are rejected.

Each mode directory contains:

- `rustc.txt`, `graph.txt`, `messages.jsonl`, `metadata.json`: exact stdout bytes
  of the commands emitted by `--describe`
- `rustc.stderr`, `graph.stderr`, `build.stderr`, `metadata.stderr`: exact stderr
- `notices/`: a fresh unchanged output tree from the sole existing
  `collect-dependency-notices.py`, including `dependency-inventory.json`

The root `capture.json` is schema 1, with `recipe`, `target`, `toolchain`,
`features`, `default_features`, `region_downloads`, `release_authorized` exactly
matching `--describe` (all three booleans are literal false), exact full
`source_sha` and `source_tree`, and `modes` containing only analytic and ordinary.
For each mode record:

- `cwd`, `source_sha`, `source_tree` and absolute `target_dir`
- `environment` exactly containing `RUSTFLAGS: "-D warnings"`, that
  `CARGO_TARGET_DIR`, and `CARGO_INCREMENTAL: "0"`; capture from a clean shell
  without extra profile/target/compiler configuration
- `commands` exactly matching the mode's described commands
- `results` containing rustc, graph, build and metadata, each with actual integer
  `exit_code: 0`, `stdout` and `stderr` records of `sha256` and byte count `bytes`
- `inventory`: the same hash/byte record for the complete inventory JSON
- `frozen_artifacts`: absolute path to hash/byte record for the executable,
  six selected `bevy`, `bevy_internal`, `bevy_core_pipeline`, `bevy_image`,
  `ktx2`, `ruzstd` rlibs and their six fingerprints, core-pipeline dep-info, and
  the compiled render-routing rlib, plus app and render fingerprints

The fixture factory in `scripts/tests/test_analytical_swift_recipe.py` illustrates
this structure with expressly synthetic data. It is test material, never a build
receipt. Future capture must retain actual command failures and cannot write a
success receipt around a failure. No capture receipt is supplied by this change.

Audit a completed real capture with:

```sh
python scripts/check-analytical-swift-recipe.py \
  --repo /exact/source --source-sha FULL_SOURCE_SHA --capture /private/capture
```

A passing result is explicitly
`build_evidence_checked_native_and_distribution_unqualified`, with literal
`release_authorized: false` and every next gate still required/unexecuted. It
contains both artifact audits, raw metadata/inventory hashes, all conservative
reconciliation differences, retained readiness blockers and source-contract
hashes. `--distribution-info` has no tone field: this report's tone identity
comes only from source/command/graph/compiled artifacts. The future extracted
runtime handshake must still independently attest Swift-only/offline/Windows
identity. No runtime handshake is inferred from a fixture.

## Honest inventory binding

A caller-supplied hash and recollected notices do not prove that metadata is
complete. Therefore the auditor independently re-runs the exact locked/offline
metadata and tree commands, and requires byte-for-byte equality to captured raw
stdout. It verifies package/version/source identities and registry checksums
against the pinned lockfile; every compiled package and source target must match
metadata, and every compiled feature must belong to the exact app graph.
Conservative-only features cannot authorize additional compiled features. Removing a conservative-only package or
stripping conservative features fails even if the caller regenerates notices
and recomputes every receipt hash. Source and all captured inputs, including every packaged notice through the
unchanged readiness checker, are checked again at the end.

Only after authoritative recapture does subset reconciliation explain workspace
feature unification. All extra conservative packages/features stay in the raw
metadata and collected inventory. No field is rewritten to make LUT exclusion
pass. Any LUT feature in the analytical metadata closure is a collection-design
blocker, even if the executable search is negative. Notice recollection must
match the entire supplied inventory; actual packaged notice hashes are checked
by unchanged readiness validation. Integrity blockers fail. Review blockers stay
open. The collector is not a reviewer and `not_reviewed` cannot be relabeled.

Real locked/offline collection during preparation found 347 exact target graph
packages and 359 conservative app normal/build metadata packages. All three LUT
asset records apply to the ordinary control; only Fira applies to analytical
metadata. These are collection observations, not combined-feature compilation
or artifact exclusion proof. `constgebra 0.1.4` and `hexf-parse 0.2.1` remain;
no package-removal claim follows. Production collection must be repeated on the
exact future source and target. Counts are observations, not acceptance rules.

The additive source contract binds whole manifest, lock, camera, guard/plugin,
regression, validator, policy and recipe files. Its baseline pointer identifies
the inherited accepted source; exact hashes bind this additive tooling. The
ordinary replay contract remains byte-for-byte unchanged: 130 reviewed source
pins and 102 independent anchors. Pin updates require an independent boundary
review and cannot be auto-refreshed to hide implementation drift.

## Required next qualification gates

1. Run actual combined release/MSVC app/render regressions and Clippy, real
   render `tonemapping_modes` with analytical render selection, ordinary controls,
   and mixed/neither-mode rejection. Execute the existing fingerprint test and
   three exact legacy acceptance tests under combined features, requiring one
   executed passing test each. Then freeze and audit both exact build trees.
2. Use the unchanged sole stager, complete source notices and Swift-only allowlist.
   Validate exact package/archive/extracted bytes and reject renamed excluded
   assets. Run from unrelated CWD with poisoned asset environment. Runtime must
   show Swift model plus 7.12 m fit and decoded PNG/exit 0; absent Light must exit
   2; legacy mismatch must exit 2; explicit no-model legacy must produce a PNG
   and exit 0 with the original fingerprint and partial-identity disclosure.
   No missing PNG, shader error or other integrity failure can become success.
   No diagnostic retry belongs to this recipe.
3. Inspect actual extracted analytical Windows daylight, low sun/night,
   atmosphere, fog/clouds, water, cockpit/HUD and camera/map/lifecycle appearance.
   Accept the darker Reinhard tradeoff explicitly; prior Linux evidence cannot
   attest Windows or cure the ordinary readback failure.
4. Obtain genuine whole-dependency/platform/runtime review bound to the exact
   inventory and hashed authoritative evidence. LUT embedding exclusions, once
   proven, do not close constgebra/hexf, fonts, shaders, MSVC/native imports,
   Rust/native contributions, terrain/climate/Copernicus, marks or store duties.
   Source publication permission is not third-party rights evidence.
5. Preserve the already-granted standing incremental-release instruction. The
   missing genuine exact-inventory publication receipt is a separate repository
   requirement, not withheld user permission. Store/Steam decisions and required
   agreements remain separate. No receipt is fabricated here.

No new workflow or export path exists. Future engineering export must remain
validated text/Swift PNG only; executable/archive/targets/notices/raw metadata/
replay stay private. This preparation grants no broader export route. Existing
ordinary candidate/release/readiness/authorization commands and accepted source
remain authoritative within their unchanged scopes.
