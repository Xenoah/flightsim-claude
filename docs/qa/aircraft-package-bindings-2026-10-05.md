# Reviewed aircraft-package source bindings, 2026-10-05

This is the independent source-boundary migration for the ordinary-build offline
[aircraft data package](../aircraft-packages.md) source frozen at
`1427ac12d0cfa23e62737172a8e1bfd6209c44fc`, tree
`5256f5d7de870accaf37f4a25063be05c2dd541b`. It extends source protection and
changes no package, app, FDM, replay, renderer or input implementation.
The baseline is `fff0f6d2c5433248090867391f4cca94075ffbff`.

## Reviewed boundary and preserved meaning

Of 130 old replay source pins, 128 retain exactly their previous SHA-256 values.
Only `crates/flightsim-app/src/main.rs` and `aircraft_profile.rs` change:

- Main registers an explicit package CLI, dispatches it before startup and adds
  command help. The commands validate, import or inspect, then exit. They do not
  activate a flight or add a picker entry.
- `SelectedAircraftProfile::from_bytes` changes visibility to `pub(super)`.
  Its exact-byte decoder, version branches and physical semantics are unchanged.
  The new CLI calls this same decoder on original package bytes and binds the
  resulting version, model path and original axes/fit.

All 102 independent anchors remain byte-for-byte unchanged. Legacy source
provenance, baseline `5c5b2a3057549c7429236b93aa0cdc99e2de38d1`, fingerprint
`0505e6644bb29a53`, missing-yaw notice, explicit legacy opt-in, old codecs,
physical laws, controller interpretation and retained FDM module guard remain
unchanged. Package SHA-256 identifies manifest/member integrity only; it cannot
replace physical identity, replay evidence, publisher authentication or rights.

Nineteen additive whole-file pins bring the replay boundary to 149 files:

- The actual app package CLI, including commercial rejection before I/O and
  original v1/v2/v3/v4 decoder/binding witnesses
- Content Cargo.toml and its lib/archive/install/manifest files, plus the three
  aircraft modules owning manifest, GLB, staging and inspection
- Aircraft package tests, their fixture ZIP builder helper and the retained
  terrain package regression file, because archive/install mechanics are shared
- The unchanged renderer `model.rs`, whose existing ModelAxis/ModelFit methods
  participate in package geometry admission
- The unchanged original Swift GLB and five embedded fixed-package fixture inputs:
  ZIP, manifest, MIT and Apache license texts, and provenance

The original Swift profile was already pinned, and the v2/v3/v4 numerical profile
fixtures remain among the old independent anchors. Complete files are protected;
there are no text projections, hash normalization or ignored implementations.

## Minimal dependent contracts

The analytical source-path set is unchanged. Its only changed pinned inputs are
main, the candidate validator, replay contract, the analytical validator's source
count comment, and its test's source count assertion. It retains all other old
hashes. The replay and analytical reviewed-source fields identify the frozen
package source above, rather than relabeling an earlier build as its evidence.

The capture runner changes only its two literal inherited contract hashes.
The capture contract receives those same values and the runner's new whole-file
hash. Its four-file boundary, original capture provenance
`75753f5e1dfe8d56d321775cb800b29bf058b3e3`, other three hashes, modes, commands,
platform restrictions, private/export separation and native/distribution
non-qualification stay unchanged. No prior run or artifact is evidence for the
new source unless independently executed and bound to it.

AST comparisons confirm that candidate execution/validation logic is unchanged
apart from adding the protected paths, the analytical validator's executable AST
is unchanged, and capture logic differs only in inherited hash literals.
Swift-only distribution, stager copy set, readiness, authorization, rights and
notice inventories, Cargo.lock/manifests and the MSVC workflow are unchanged.
No ordinary or analytical candidate gate is made optional.

The existing narrow MSVC workflow watches the capture runner and contract on
pushes to main. Eventual publication of this migration therefore triggers a new
build-only run even though the workflow itself is unchanged. This source review
launches no workflow, uploads no executable and grants no release permission.

## Verification and limits

The frozen source independently passed 125 ZIP/GLB/cancellation probes and 18
Linux installed-tree probes after closing the materialized-tree entry-budget and
Scene0 root-vector issues. Its existing terrain and new aircraft binaries passed
23 and 13 tests respectively. Schema tests passed 4/4. Those observations are
content-boundary evidence, not app/native/Windows execution by this review.
Unix hardlinks reject; Windows reparse-point rejection does not establish a
separate Windows hardlink guarantee.

The new candidate regression tests commit 12 concrete bad-source mutations in
disposable fixtures, covering pre-start dispatch, commercial admission, original
profile delegation, model binding, semantic validation, hashes, shared archive
limits, no-overwrite publication, root bounds, axes/fit and the policy witness.
Every mutation must fail before a build. Export tests reject four omissions or
hash substitutions for each of the 19 added sources (76 cases). The pre-existing
all-file drift test also exercises all 149 source pins and 102 anchors.

Baseline Python suites passed 116 tests with one Windows-only junction skip.
Focused updated suites passed 37 tests with the same skip. The complete updated
suites passed 118 tests in 104.494 seconds with that same platform skip. Final
clean-commit source-evidence results are recorded in the associated review
receipt. Source checks never execute a compiled app.
The separate app build lane owns app tests, actual CLI behavior and native
qualification; source contract acceptance does not substitute for those gates.
