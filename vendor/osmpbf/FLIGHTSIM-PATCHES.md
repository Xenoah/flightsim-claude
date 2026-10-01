# Local safety patch to osmpbf 0.3.7

Upstream: https://github.com/b-r-u/osmpbf
Source: https://static.crates.io/crates/osmpbf/osmpbf-0.3.7.crate
Archive SHA-256: 51b6d8d4418d577b7a07c2113955cf1aec96d8b528ddbeaeb2e6299b9691d712
License: MIT OR Apache-2.0; both upstream licenses are retained.

Issue: https://github.com/Xenoah/flightsim-claude/issues/23

The upstream infallible element iterators are unchanged. Construction of an
immutable PrimitiveBlock now validates every delta accumulation, coordinate/time
scale and offset, parallel-array length, UTF-8 string and string-table index, and
relation member enum. Invalid input returns an InvalidData error before an
iterator can overflow, panic, wrap, or silently truncate malformed tags.

Blob readers reject partial prefixes, truncated frames, negative/oversized blob
lengths, invalid raw_size and decompressed-size mismatches. Zlib data is decoded
to a bounded buffer before protobuf parsing, so a valid protobuf prefix cannot
hide truncated or oversized compressed data. Memory-map iteration now terminates
after errors and validates offsets and blob lengths too.

No catch_unwind boundary or global panic-hook changes are used. The indexed
reader and valid-element ordering are preserved. Airport-level semantic errors
(e.g. missing referenced nodes) still follow the original reason-specific skip
reports. This is a structural safety boundary, not a promise that arbitrary
input has zero allocation cost or that OS exhaustion can be recovered.

Only the upstream source, build script, README and licenses are vendored;
upstream examples/benchmarks/test datasets and their development dependencies
are omitted. Regression fixtures use independently encoded protobuf bytes in
flightsim-tilegen/tests/pbf_hostile_probe.rs, in debug and release profiles.
Revisit the fork only after an upstream version passes those same regressions.

Only the active pure-Rust zlib backend is exposed by this local fork. Optional
native zlib/zlib-ng feature switches are omitted to avoid adding unused native
build dependencies to the workspace lockfile. Decoding behavior of the existing
default rust-zlib feature is unchanged.

The fork is stored at repository-root vendor/osmpbf, outside workspace member directories. Cargo issue #6745 makes excludes nested below a member ineffective; cargo metadata confirms this fork is not an application workspace member. The inherited Some(None) rustdoc link was corrected to a code span.
