# Windows supplier environment probe

The `Windows supplier environment probe` workflow runs on `windows-latest`
only for pushes to `qualification/windows-supplier-probe`. It installs the authorized official Rust
`1.93.0-x86_64-pc-windows-msvc` minimal toolchain and reuses the existing runtime
collector. It does not compile or run the application, create a release, change
qualification gates, accept new terms, or separately download supplier notices.

The collector runs only its source-defined Rust identity/sysroot/cfg print
queries and the installed official `vswhere` query. The separate `-D warnings`
cfg query is hypothetical; it is not an observed application compilation.
Installed VS products, toolsets, SDK candidates and bounded known notice
locations are observed without executing a candidate linker. Missing facts
remain missing or `not_established`; discovery is not claimed to be exhaustive.

## Public artifact

Only these two validated JSON files are uploaded, with seven-day retention:

- `runtime-facts.json`: the unchanged collector's closed-schema projection,
  bounded to 512 KiB. It contains candidate identities, file sizes/hashes and
  query-receipt hashes. No paths, raw logs or notice bytes are exported.
- `environment-only.json`: a closed source-generated receipt, bounded to 4 KiB.
  It labels the scope `environment_only`, records application build inputs as
  `not_supplied`, binds the exact runtime JSON bytes, and records only bounded
  `ImageOS`/`ImageVersion` tokens provided by the runner. Missing image metadata
  is null. `requested_runner_label` is the workflow label, not a claim about a
  fixed runner image; these environment tokens are not a host attestation.

`source_sha` and `probe_source_sha` identify the checked-out probe scripts, not
an application build. The workflow verifies that the probe commit has the
already-public `e60d5944c7e201b4b3b760f55616e890cb241d17` as its immediate
predecessor and `55094fa928fbb8f907a2103740ee6824171c26e7` as its first-parent grandparent;
`probe_base_sha` records that baseline. Environment facts may inform later
prospective supplier review, but do not qualify any later application source. There is no linker trace, build fingerprint, executable,
selected SDK, linked static-membership proof, rights approval or same-build
supplier association. `release_authorized`, `dependency_review_approved` and
`native_runtime_coverage_complete` remain false. This evidence cannot replace
the corresponding qualification or distribution review.

The raw discovery receipts and notice snapshots remain private on the ephemeral
runner. Before upload, the wrapper replays the installed-file discovery,
rechecks byte bindings and requires exact public-directory membership and
canonical bytes. Any collection or validation failure prevents the upload.
Failures print only fixed source-defined stage/error labels, never exception
text, paths or raw query output. The admitted `win25-vs2026` image token is
verified against the [official image source](https://github.com/actions/runner-images/blob/win25-vs2026/20260925.250/helpers/GenerateResourcesAndImage.ps1).
The first native probe failed with an opaque exception; this token mismatch is
a confirmed compatibility defect, not proof that it was the only failure.

## Local checks

These tests use synthetic files and queries; passing them is not a native
Windows capture:

```sh
python -m unittest discover -s scripts/tests -p test_windows_supplier_environment.py -v
python -m unittest discover -s scripts/tests -p test_analytical_runtime_facts.py -v
```

A genuine observation requires publishing the reviewed probe commit on its
dedicated branch and reviewing the resulting workflow run. No pull request or
main push is needed; existing main/PR CI is outside this probe.
