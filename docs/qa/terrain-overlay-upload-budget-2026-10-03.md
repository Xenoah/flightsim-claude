# Atomic ground-overlay upload budget

Scope: renderer-only continuation of the 2026-10-02 regional surface branch.
The accepted ef971772 snapshot policy below is superseded on the isolated
[copy-pacing candidate](terrain-overlay-copy-pacing-2026-10-03.md); its focused
functional gates passed, while measured/native acceptance remains pending.
No changes to physical terrain, authored lifts, clipping or optional priority.

## Contract

- Terrain surfaces, bridges, overlay snapshot-copy attempts and overlay upload
  attempts share the existing mesh-count allowance. Failed snapshot attempts
  consume an attempt; zero remaining allowance cannot upload a completed job.
- A completed clipping job is moved into new Mesh assets across updates. Those
  assets are not referenced by a rendered entity until all replacement outputs
  have been uploaded. The old terrain cut, root Mesh3d handles and visibility
  remain in place until the shared atomic commit. Commit changes handles and
  visibility, without inserting/mutating all overlay Mesh assets again.
- Normal aggregate upload target is 65,536 actual vertices per update. One
  indivisible output larger than that target is admitted only as the first
  upload, with a hard 524,288-vertex maximum. Further nonempty uploads wait.
  This is explicitly not a sub-65k hard upload guarantee or a frame-time claim.
- The accepted ef971772 snapshot policy copies at most 262,144 actual surface
  vertices and two sources per update, under the remaining shared attempt
  allowance. Its per-source index cap is 6 times that vertex cap, so two scans
  imply a 3,145,728-index bound. The isolated copy-pacing candidate preserves
  the hard vertex/index-work bounds and per-source/transaction admission limits,
  while changing per-update source count to pack small snapshots under
  8,450-vertex / 52,224-index normal targets. It separately reports charged and
  actually scanned indices; see its pending acceptance record. Both paths avoid
  an intermediate copy of the complete index buffer.
- Every generated asset has a non-rendered ownership child under its original
  caller-owned root. Root despawn frees current and unpublished generated mesh
  assets even though the caller caches only its original handle. The original
  handle may still be retained/removed by the caller, but its original asset is
  retired at the first successful commit to avoid duplicate full buffers.
  Unregister/optional clear
  also queue generated ownership for safe cleanup on the next terrain advance,
  permitting retained roots to be reused without accumulating generated assets.
- Cancellation invalidates the generation immediately. Unpublished assets of
  remaining registered roots can be drained by explicit terrain reset; removed
  roots are reclaimed by root despawn or safe deferred cleanup on next advance.
  Reset never returns already-despawned children of removed optional roots.
- There are no detached disposal tasks. CPU result/mesh destruction, metadata
  commit and retirement remain synchronous, bounded by admitted cohorts. The
  implementation moves complete meshes rather than cloning them. Logical
  asset residency and deterministic work counts are not global process-memory,
  GPU-driver-memory or latency guarantees.

`TerrainOverlayUsage.frame_work` and `StitchProgress.overlay_work` expose per-update
copy attempts/vertices and upload attempts/meshes/vertices. Pending uploaded
mesh/vertex counts are separate from these frame counters. Assets become available
to Bevy's render preparation; these counters are not GPU-completion fences.

## Regression coverage

- Zero and one mesh-attempt budgets; target-exact, target+1 and maximum-size
  indivisible output, without starvation of the next output
- Actual snapshot-vertex cap and charged rejected excess/missing sources
- Three-output production transaction: old root handles and terrain stay visible
  through multiple upload updates and a zero-budget interruption, then switch
  together; replaced generated assets return to baseline
- Cancellation at every upload step, including ready-to-commit; restart before
  deferred entity commands flush; repeated cancel and retained-root replacement
- Unregister with current and unpublished generated assets; optional clear
  followed by terrain reset with mixed required/optional staging
- Existing optional omission/recovery, source replacement, precision/rebase,
  source-generation invalidation and owner-retirement tests

Test commands/results and runtime captures are recorded with the implementation
handoff. This document does not substitute numeric tests for actual-scene visual
or frame-time measurement; integration captures remain a separate gate.

## Local implementation gates (2026-10-03)

Using the existing shared build cache, locked/offline dependencies, two build
jobs and `RUSTFLAGS=-D warnings`:

- `cargo test -p flightsim-render -p flightsim-app`: passed 271 renderer unit,
  72 renderer integration and 178 app tests. Three existing manual/optional
  fixture tests ignored. The slowest renderer unit group took 278.66 s; this is
  test-run duration, not rendering latency.
- After the final generated-handle alias admission guard: all 16 overlay unit
  and 10 production overlay transaction tests passed. The full app suite was
  rerun and passed 178 tests with its one optional real-region test ignored.
- `cargo clippy -p flightsim-render --all-targets -- -D warnings`: passed after
  replacing a test-helper complex tuple signature with a named alias.
- Strict `cargo doc -p flightsim-render --no-deps`, workspace formatting check,
  dependency architecture check and native `cargo build -p flightsim-app`: passed.
- Independent static review: no remaining must-fix findings. Actual native
  captures, integrated per-frame log measurements and driver/FPS claims are not
  part of these local implementation gates.
