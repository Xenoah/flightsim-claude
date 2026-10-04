# Data-only local package foundation QA — 2026-10-04

Scope: new `flightsim-content` library, strict prepared ZIP/manifest format,
immutable local installation and runtime DEM verification. No app/UI activation,
network downloader, Windows runtime test, publication or release is covered here.

## Verified

- `cargo test -j 2 -p flightsim-content`: 22 integration tests passed after the
  follow-up header/cancellation fixes below.
- `cargo test -j 2 -p flightsim-world terrain::tests`: 16 terrain tests passed,
  including the new oversized sparse-file/trailing-data regression.
- `cargo clippy -j 2 -p flightsim-content -p flightsim-world --all-targets -- -D warnings`: passed.
- `bash scripts/check-architecture.sh`: passed with `content` included in the
  engine-independent and one-way dependency checks.
- `cargo fmt --all -- --check`: passed. The final store-cap regression refinement
  was formatted, rerun separately and passed crate all-targets Clippy.

Used the checkout's `build-env.sh`, Rust 1.93.0, two Cargo jobs and the coordinated
shared build cache. This is focused library validation, not a workspace test pass.

The first compile found a Degrees-newtype conversion error and was corrected.
The first expanded test run had a failing compression-ratio fixture: its declared
manifest length was below the configured 1,024:1 boundary. The fixture now sets an
explicit 2,048:1 forged declaration; the final run passes. These were not omitted
or represented as successful initial runs.

## Security and lifecycle cases

Tests cover Store/Deflate imports and exact byte/manifest identity preservation;
normal world-reader sampling; global fallback composition and antimeridian bounds;
unknown schema/kinds/files and ordinary repository/raw DEM ZIP rejection; traversal,
absolute/drive/UNC/backslash/ADS/device-name/trailing-dot/Unicode paths; repeated and
case-colliding paths at file/directory levels; ZIP links and special file modes;
encryption and unsupported compression; excess archive/file/manifest sizes and
counts; actual inflation and compression-ratio caps; CRC, SHA-256, size and local
header mismatch; truncated ZIPs; canonical DEM paths/IDs, dimensions, trailing bytes,
non-finite/unsupported heights and coverage; installed symlinks/extras/tampering;
source ZIP mutation after a private snapshot; cancellation at every import phase
including Ready; dropping a staged package; interrupted staging leftovers;
nonblocking lock contention; concurrent publication; duplicate version refusal;
and store capacity without hiding existing versions or leaving a new ID directory.

A read-only independent source review found three issues, all addressed:

1. Implicit directory prefixes now count toward the same 8,192 materialized-tree
   cap used by later inspection, before extraction.
2. rawzip 0.5.1's unchecked prefixed ZIP64 offset adjustment is unreachable for
   accepted inputs: a strict ZIP32 envelope preflight rejects prefixes, ZIP64
   central fields/locator, mismatched central boundaries and excessive metadata.
   A dedicated malicious offset regression returns an error rather than panicking.
   Preflight and decoding operate on the same private, bounded snapshot.
3. Commit checks the 256-version store capacity under the nonblocking OS lock,
   before creating a new ID directory. This matches listing's limit and prevents
   unsuccessful imports from accumulating empty IDs.

## Limits and integration gates

- The caller must use a trusted local store. Concurrent hostile writes to the
  application's own store/lock/snapshot files are outside the threat model.
- Complete directory rename is atomic under the cooperating importer lock;
  power-loss durability of directory metadata is not claimed.
- Full payload inspection is required before a new-flight activation. The runtime
  package source rechecks declared hashes; copying a bare terrain directory into a
  normal DiskTileSource does not provide that package integrity guarantee.
- Package-backed replay v1/v2 is explicitly unsupported. The app must gate both
  recording and playback; no existing replay bytes or global fingerprints changed.
- Rights/source records are inert data, not publisher authentication, user legal
  acceptance or distribution clearance. New registry dependency records are in
  `docs/release/content-package-dependencies.json`; full target/feature-specific
  notice inventories need regeneration after app integration.
- Prepared ZIP32 packages only; no promise of generic GitHub/raw terrain support.
  UI progress, drag/drop, cancellation presentation, selecting a package and new
  flight activation remain app integration work and require their own verification.


## Follow-up: hostile geometric-error headers and empty Deflate blocks

A subsequent independent review identified two gaps; both are fixed at the
content boundary without modifying legacy FSDM reader/writer bytes or replay data.

- A finite `geometric_error = f64::MAX` header could pass a correctly rehashed ZIP
  import, then overflow the automatic skirt calculation. Package validation now
  rejects a declared error larger than the already validated decoded elevation
  range plus 10⁻⁶ m. The standard writer derives its error from bilinear coarse-grid
  interpolation of those same samples, so its error cannot exceed their range
  apart from roundoff. Tests regenerate valid manifest SHA-256 and ZIP CRC after
  mutating the header, and reject `f64::MAX`, a value beyond f32 range, and an
  unsupported 1 m error on a flat tile. Legitimate writer outputs with 2, 3, 4, 17
  and 65 sample edges spanning the supported height extrema still import and
  produce finite positions, normals and slopes with default automatic skirts.
- A single Deflate decoder read can consume many empty nonfinal blocks without
  producing output. Cancellation now also runs on each compressed input read,
  capped at 32 KiB, while keeping the existing output-chunk checks. A valid 160 KiB
  empty-block stream followed by the declared license text imports in its control
  test, then a cancellation run stops after four zero-output progress callbacks
  and removes its own staging/snapshot files. This is a bounded structural
  regression, not a timing benchmark.

Final follow-up validation: all 22 package tests, content all-targets Clippy with
warnings denied, and workspace formatting passed. Independent read-only review of
these changes found no further concrete issue. Full app/UI and Windows runtime
validation remain separate integration work.
