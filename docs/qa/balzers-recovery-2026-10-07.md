# Local recovery and Balzers content verification, 2026-10-07

## Source recovery

The source baseline was reconstructed from preserved source archives and the
public Git tree of `0c77b5a3925ba4d7002d51e0ded8a34cf08fc568`. Every one of its
2,383 file contents was checked against its Git blob SHA, both in the restored
worktree and in the recovery ZIP. Local Git history is newly reconstructed; this
does not claim the original repository history was recovered. Original tracked
files that match ignore patterns were explicitly restored to the index.

The initial Python baseline exposed that ignored-but-originally-tracked fixtures
were missing from the new index, although their bytes existed in the worktree.
The index was corrected from the exact remote manifest and the complete Python
suite rerun successfully. No test or fixture expectation was relaxed.

Rust 1.93.0, Cargo, rustfmt and Clippy were installed from the official Rust
distribution. Required native development headers/link metadata were restored
into a workspace-local Debian sysroot with signed-release and package checksum
verification. The operating system, credentials and security settings were not
modified.

## Rebuilt content

See the [local Balzers package](../examples/terrain-packages/balzers/README.md)
for reproducible input/output hashes, exact bounds, notices, source limitations
and commands. This is `balzers-glo90-rebuilt@1.0.0`, a new artifact, not a claim
that the lost later draft was recovered. Its 170 DEMs are byte-for-byte subsets
of the verified 765-tile source, with complete L10–13 descendants and unchanged
original source/license records.

The additive code consists of an offline fixed-input Python package recipe,
focused tests and a GUI-independent Rust validation/import example. Runtime
package schemas, network/cache policies, activation, FDM, replay, aircraft,
release allowlists and licensing gates are unchanged. No cache receipt or public
download URL is fabricated. Optional catalog metadata requires an explicitly
supplied valid GitHub source; availability and publication remain separate.

## Passed locally

- Original headless group with content downloads, all targets: 1,373 Rust tests,
  zero failures; 47 Criterion smoke cases (not performance measurements)
- Six headless doctests; full headless Clippy with warnings denied; architecture
  dependency checks and workspace formatting
- Native app/render/input/UI/audio group, all targets with `region-downloads`:
  1,302 tests, zero failures, three declared ignored cases (optional external
  scenery, normalized Haneda DEM, and a manual quiet-window latency measurement)
- Separate default/offline app configuration, all targets: 394 tests, zero
  failures, one ignored optional external scenery fixture
- Full-workspace Clippy with `region-downloads` and all targets: warnings denied
- Full default-feature workspace documentation, including private items:
  `RUSTDOCFLAGS=-D warnings`, successful
- Three added real Balzers integration tests: exact ZIP/manifest/identity pins,
  all tile loads and payload hashes, original notice hashes, immutable duplicate
  install, later payload corruption, missing-tile miss, all cancellation phases,
  dropped staging and retained replay refusal
- Production CLI `validate`, `cancel`, `install`, `inspect`: successful against
  the exact sample; no flight activated
- Rebuilt Linux application binary: `--import-region` and `--list-regions`
  succeeded with the real ZIP; repeat import exited 1 with `AlreadyInstalled`
- Content Clippy, downloads enabled and all targets: warnings denied
- Complete Python script suite including the generator: 315 tests, one intentional
  skip, no failures; the focused generator suite also passed all 20 tests
- Global terrain packer Python suite: five tests, no failures
- Aircraft profile schemas: three tests, no failures
- Two actual-source Balzers generations: ZIP and all three sidecars byte-identical
- Independent artifact/review pass: ZIP CRC/SHA, FSDM checksum/grid/range,
  173 inherited payloads/records, complete child groups, whole-tile bounds,
  documentation links and SHA256SUMS; no blocking findings

## Not established by these results

The first native aggregate attempt built the application but exhausted the 32 GiB
workspace while linking many debug-heavy test binaries. The linker failed and
queued full-workspace Clippy could not allocate its output directory. This attempt
did not pass. Generated compiler output was removed; source and the durable
checkpoint were preserved. The successful native-suite retry uses `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_INCREMENTAL=0`, with the same tests,
optimization policy, assertions and warnings-as-errors. The workspace used about
8.2 GiB after successful native/default app, full lint and documentation checks,
leaving about 22 GiB free. No test threshold was relaxed.

The existing GPU-free aircraft scene-hierarchy test emitted 46 Bevy B0004
insertion-hook warnings in each configuration. Its documented post-readiness
checks of every spawned entity/parent passed. Warnings were neither suppressed
nor treated as proof of a complete rendered scene; the test and model code were
unchanged by this work.

No fresh rendered-flight or Windows runtime acceptance is claimed
here. The pre-existing Windows screenshot timeout is not a compile failure and
has not been retried or bypassed as part of this recovery. Software content
validation does not establish surveyed terrain accuracy, GPU performance,
controller/audio acceptance, regional replay support or commercial rights.

At the recovery checkpoint, no GitHub write or binary release had been made.
The user subsequently authorized publication of the latest recovered source on
2026-10-07. That new source-publication instruction does not execute or approve an
unknown older pending payload. Existing release qualification and rights blockers
remain in force.
