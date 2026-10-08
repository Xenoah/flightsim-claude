# Full two-aircraft source candidate

Status: local source preparation; final source/recipe admission is pending.
Base: `d918943a70d01644b5a6bebb8a903a5bbc89139f`.
No main branch, hosted repository, terms acceptance or publication authorization
is changed by preparing this source. The original full two-aircraft goal is
preserved; no Swift-only or Reinhard variant is selected.

## Composition and exact source boundary

- `23f7a04ae4f0f9154d7a67d61987f0f8aba52c8d`: the 31-file independently reviewed
  ordinary package assessment. constgebra's declared Apache alternative and
  hexf-parse's original CC0 route are package-scoped evidence; whole-target and
  native assessment remains required. Historical native inventories remain
  historical and are not rebound to this candidate.
- `74d36d933fe1cffa5d62895f85cf51a6d3ff9760`: the reviewed local Bevy 0.18.1 core
  pipeline source subset. Exact Tony/Filmic payloads and all shaders remain;
  AgX payload/embedding are removed and AgX selection is explicitly rejected.
  Camera, exposure, analytical behavior and material code are unchanged. The
  retained `info.txt` describes the upstream three-LUT recipe as historical source.
- Original high-wing adapter archive SHA-256
  `5517445a00cb6fc49a3dddb0eff17313adc41952dc610b8f45991f7b9b2cc6da`:
  current Light Single GLB is 140,840 bytes with SHA-256
  `b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc`.
  Its reviewed adapter and source snapshots are retained under
  `tools/original-highwing/`; the actual runtime path holds exactly those bytes.
  The old Meshy model remains unresolved historical content, never a claimed
  source of this replacement.

`full-two-aircraft-source-inputs.json` proposes a new, explicit per-file input
boundary and recipe identity `full-two-aircraft-tony-filmic-source-preparation-v1`.
It is a review input, not an admission receipt. The source checker verifies exact
files and rejects undeclared siblings in controlled source directories. It does
not wildcard any existing admitted source map or regenerate replay goldens.
All `crates/` inputs, Light Single and Swift profiles and 102 independently pinned
replay anchors are retained byte-for-byte against the base. Root Cargo.toml and
Cargo.lock change only through the reviewed core-pipeline path patch.

The intended full Windows recipe retains `flightsim-app` default features,
release profile, Rust 1.93.0 and `x86_64-pc-windows-msvc`, with Light Single as the
default and Swift Sport available. Tony remains the ordinary appearance. The
Light Single profile keeps its original numeric dynamics, controls, camera,
model axes/path/length, sound category and replay identity inputs. A different
mesh does not establish new aerodynamic calibration or full visual qualification.

## Current versus historical assertions

The current rights manifest records the new original Light Single bytes and the
two retained LUTs. Old Meshy and AgX records are retained in explicit historical
fields and `history/mesh-and-lut-boundary-d918943.json`; no old right is cleared.
Swift-only commercial asset selection stays separate. Its denial scan still
rejects unresolved historical asset bytes, including renamed copies.

Meadow, Kestrel and Cedar creation-time hash tables remain visible as
`CREATION_BASELINE`. Their current checks explicitly pin the replacement and the
few dependent checker/manifest changes. Every other old assertion remains exact.
A creation-time hash is never relabeled as evidence of unchanged current bytes.
Cedar's policy-manifest pin was already stale at d918943; the current record
explicitly binds the composed manifest rather than repeating that old claim.

The root export exclusion is replaced only for the now-original Light Single
path. The archive checker requires the explicit `original-highwing-v1` policy,
exact new GLB and provenance, and continues to reject the historical Meshy hash
at every path and inside nested archives. Legacy exclusion mode remains the
default. See [source archive policy](source-archive-policy.md).

## Existing safeguards and required next evidence

The old three-LUT build audit, analytical ordinary positive control, legacy
source maps, release authorization checker and workflow are unchanged. They
cannot admit this different recipe; an old positive receipt must not be reused.
The new source checker reports preparation only and always rejects
`--require-release-admission` because no release admission exists in this schema.

A genuine new admission requires independently reviewed final source/recipe;
fresh exact target graph, Cargo messages, fingerprints, dep-info and binary
Tony/Filmic presence/AgX absence checks; complete dependency/native review and
notice retention; any required native terms adoption; final inventory binding;
extracted two-aircraft Windows runtime, screenshots, switching, replay and clean
exit evidence; final local plus hosted commit/tag archive checks; and an exact
inventory-bound publication authorization. None is marked complete here.

The separate linker-output association repair `4282bee` was inspected and kept
outside this composition. Fresh native collection can adopt it after an explicit
review of that collector boundary; no new collection or Windows run is claimed.
The separate high-wing simulator witness must be reviewed and bound to its own
exact source and emitted model; Blender previews alone are not simulator proof.

## Local checks

```sh
python scripts/check-full-two-aircraft-source.py
python scripts/check-full-two-aircraft-source.py --require-release-admission
python tools/original-highwing/check_rejection_cases.py
python tools/blender/validate_meadow_trainer.py
python tools/blender/validate_kestrel_jet_trainer.py
python tools/blender/validate_cedar_turboprop.py
python -m unittest discover -s scripts -p test_source_archive.py
```

The second command must fail even when static source checks pass. No Rust build
or shared target use is part of this composition; earlier component type checks
remain bounded evidence, not a fresh whole-candidate test.
