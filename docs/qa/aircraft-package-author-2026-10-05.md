# Aircraft package author validation, 2026-10-05

Author branch based on `fff0f6d2c5433248090867391f4cca94075ffbff`.
This records the pure content/schema author lane only. Integration/default and
commercial app tests, actual CLI/native load, platform evidence and protected
source-contract migration are distinct lead-owned gates, not inferred passes.

## Implemented boundary

Independent aircraft manifest/schema 1, a closed static untextured GLB parser,
byte-preserving original Swift package/builder, uncommittable byte stage followed
by app-supplied exact profile validation, immutable basic import and complete
installed inspection. No executable content, network, picker/activation change,
physics/model/replay reinterpretation or release permission.

The shared private ZIP/path/publication extraction retains terrain's public
schema, file types, limits and DEM decoder. It additionally rejects Windows
reparse metadata and filesystem reparse points, and multiply linked Unix files.
Fresh extraction creates new regular files. Unix hardlink rejection was tested
on Linux only; no Windows-hardlink rejection or qualified Windows-import claim
is made. No Cargo manifest/lock/dependency change. Protected old source changes are only
app `main.rs` (offline CLI dispatch/help) and `aircraft_profile.rs` (dispatcher
visibility). Existing source contracts were deliberately not re-pinned by the
author. Original Swift profile and GLB are byte-for-byte unchanged.

## Passed author checks

Official Rust 1.93.0; offline/locked, two jobs, incremental disabled, debug info
zero, `RUSTFLAGS=-D warnings`, existing shared target. No app/Bevy build or GPU
execution was run by the author.

- `cargo test --offline --locked -j 2 -p flightsim-content`: 36 passed,
  comprising 13 aircraft tests and all 23 existing hostile terrain tests
- `cargo clippy --offline --locked -j 2 -p flightsim-content --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `python3 schemas/tests/test_aircraft_package_schema.py`: 4 passed
- `bash scripts/check-architecture.sh`
- Built content-only `validate_aircraft_package` example; original ZIP validates
  and extraction cancellation leaves no stage/publication
- Fixed Swift builder reproduces the checked-in ZIP byte-for-byte and refuses
  to overwrite an existing destination
- `git diff --check`

The new tests cover original Stored/Deflated identity, semantic-gate rejection,
dropped stage/no overwrite/nonblocking lock, phase cancellation, reinspection,
hash/size/file-set/schema/path/type separation, source and installed links,
hardlinks/reparse metadata, hash-valid hostile GLB features/URI/topology/binary
floats/bounds/degenerate triangles and materialized-directory budgets.

The independent hostile-input review caught a stage/inspection disagreement in
implicit directory counting and excess Scene0 root-vector expansion. The final
implementation explicitly caps materialized path prefixes including the manifest,
and caps/uniquifies root nodes before allocating traversal work. These have
regression coverage. Final independent review binds its own source snapshot.

## Original fixture identity

- ZIP: 248,548 bytes; SHA-256
  `96aada29c5d8f0bb8900ea0b3c830a01202e679838885df83a931e4726b5f72e`
- Manifest SHA-256:
  `6773f1c9cd95881d53c4fe0a18f6a6c4ff94bfe485cc93b3285ca1c68d209cc1`
- Profile: 2,717 bytes; SHA-256
  `319257c8363cf5b0d480b914796f43bbec8b7935adc32d095c706c86b8c3e663`
- GLB: 229,192 bytes; SHA-256
  `9f30f6f9babe87a54d0f1f5da104f719d7e99cb05aada2848f8ea88b0bf9e3b1`
- Decoded Scene0: 31 nodes, 31 primitives, 5,389 vertices, 25,212 indices;
  approximately 7.12 × 2.62 × 9.52 authored model units

Actual profile semantics use the unchanged app family dispatcher. New app tests
cover original-byte v1–v4 decoding, mismatched manifest/version/model/fit,
duplicate/fractional version tokens, exclusive commands and commercial rejection
before file access. They were authored but not executed in this pure lane.
The existing manual ordinary startup is not the in-flight scene-readiness
transaction; a later successful native Swift observation must remain scoped to
that exact package/build/case.
