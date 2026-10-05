# Analytical Swift native MSVC build capture

This additive runner executes only the build/artifact portion of the existing
[strict prepared recipe](analytical-swift-recipe.md). That earlier preparation
document and its source pins are unchanged. This document describes the new
runner; it does not reinterpret a prior preparation result as execution.

The manual `Analytical Swift MSVC build evidence` workflow also runs on a push to
`main` that changes exactly one or more of these trigger paths:

- `.github/workflows/analytical-swift-msvc-build.yml`
- `scripts/capture-analytical-swift-msvc.py`
- `scripts/analytical-swift-capture-contract.json`

Other changes do not trigger this workflow. There is no pull-request,
`workflow_run`, ordinary candidate, release, or general CI integration. Every
invocation checks out its exact `github.sha`. The additive capture source
contract requires independent review; no tool refreshes any contract pins.
Both inherited source contracts, including all 130 reviewed replay pins and
102 independent anchors, remain byte-for-byte unchanged.

## What actually runs

On a fresh native x64 `windows-2022` runner, install official Rust 1.93.0 MSVC
and Python 3.12. Run pure Python capture/export boundary tests. The production
entry rejects a non-Windows machine, a non-native compiler host, inherited
compiler/profile overrides, and ancestor Cargo configuration. It creates a
private fresh Cargo home and performs one locked dependency fetch. It then
executes the existing recipe's exact rustc/tree/build/metadata commands with
locked/offline release, jobs 2, warnings denied, and incremental compilation off.

Analytical app selection is exactly `analytic-tonemapping,commercial-staging`
with no defaults. The ordinary positive control uses ordinary defaults with no
commercial feature. Their target directories are separate and fresh. There are
no app invocations, screenshots, readback diagnostics, staging, binary uploads,
release operations, dependency approval, credential or permission changes.

Each command writes exact binary stdout/stderr files and its real exit status
to private journals. A timeout terminates the subprocess tree and records the
actual resulting status; a launch failure has no invented exit code. Failures
stop subsequent commands. Failed output is never rewritten as a success receipt.
The unchanged notice collector runs on each complete raw metadata capture.
The unchanged auditor then runs on this same machine/source/target/Cargo home.
It independently recaptures locked/offline metadata and graph evidence and
validates profile/features, selected artifacts, fingerprints, dep-info, payload
absence/presence, inventory, notice integrity, and preserved readiness blockers.
Entire target trees are hashed at freeze and compared after audit. Nothing
compiles into a frozen tree. Exact source is rechecked before completion.

## Export and remaining gates

Only `build-evidence.json`, at most 32 KiB, may leave the runner. Its exact-key
canonical ASCII JSON schema permits fixed identifiers/enums, hashes, bounded
counts, actual command statuses and the full list of unexecuted recipe gates.
It contains no arbitrary strings, URLs, paths, command output, metadata,
notices, packages, replay, image, executable or archive bytes. Revalidation
rejects extra files, links/reparse points, hard-linked export files, oversized
or noncanonical data, invocation mismatch and changes from the private result.
The workflow uploads only the single constant filename after validation.
No cache or wildcard/private-directory upload exists. Raw failure detail stays
private and is discarded with the ephemeral runner; the exported failure stage
and byte/hash/status bindings are intentionally limited diagnostic evidence.

Passing status remains
`build_evidence_checked_native_and_distribution_unqualified`, with literal
`release_authorized: false`. All seven recipe gates remain unexecuted, including
combined release/MSVC app/render regressions and Clippy, real analytical render
`tonemapping_modes`, ordinary regression controls, mixed/neither compile guards,
the exact fingerprint and three exact legacy acceptance tests (one executed
passing test each). The ordinary positive-control build is artifact evidence,
not execution of those ordinary regression controls. Separate future test trees
must be used; these frozen trees must never be reused. Native extracted-bundle
acceptance, appearance, platform/dependency review and the exact publication
receipt remain separate. The closed ordinary diagnostic is never reopened.

## Resource limits and local verification

This job allows 300 minutes, with 120 minutes per cold release build, 20 minutes
for fetch, and 30 minutes for audit. No wall-time performance result is claimed
before a real Windows run. Two serial cold release builds with thin LTO and
codegen-units 1 may take roughly 1–3 hours; that is a planning estimate only.
Start requires 12 GiB free on the private build volume; each new command needs
at least 2 GiB free. Disk/resource failures remain failures, never qualification.
Both full target trees and registry source are retained until the audit ends.
GitHub documents public standard Windows runners as 4 CPUs/16 GB RAM/14 GB SSD
and private ones as 2 CPUs/8 GB RAM/14 GB SSD, so disk and linking memory are real
constraints: https://docs.github.com/en/actions/reference/runners/github-hosted-runners

Local Linux verification is pure Python only:

```sh
python -m unittest discover -s scripts/tests -p 'test_analytical_swift_capture.py' -v
python -m unittest discover -s scripts/tests -p 'test_analytical_swift_recipe.py' -v
```

Subprocess tests execute real small Python programs to test byte preservation,
nonzero exit propagation, process-tree timeout and launch failure. Export tests
use explicitly synthetic schema fixtures; they never assert a real Windows build
or create a passing native receipt. Actual MSVC execution is still required.
